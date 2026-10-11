//! Food estimator (`.4.22`): heating rate, stall and ETA for one food probe, online.
//!
//! The quantity everything rests on is the **heating ratio** `rho = (dC/dt) / (P - C)` (per hour):
//! the core's rate of rise per degree of gap to the pit. A dry piece of meat in a steady pit heats
//! as a first-order lag after the first half hour, so `rho` is a constant of the cut (convection
//! over heat capacity, ~0.24/h for a packer brisket). Evaporation from the wet surface takes heat
//! that would have reached the core, and grows steeply with surface temperature: `rho` sinks through
//! the ramp and all but vanishes in the stall. Once the bark seals, `rho` recovers to the dry value
//! over a few hours.
//!
//! - **`a`**, the dry ratio, is the largest `rho` seen on the ramp (or the cut's prior until then).
//! - **Stall**: the core rises less than [`STALL_RATE_F_H`] while `rho` is well under `a` (so a core
//!   that is merely closing on the pit temperature is not a stall), above 120F, for 10 min. It ends
//!   when the rate is back above [`STALL_END_F_H`] (and twice the stall's slowest) for 10 min. One
//!   stall per cook.
//! - **ETA**: `rho` recovers from its current value to `a` with time constant `recover_h`, and
//!   `ln((P - C) / (P - T)) = integral of rho dt` gives the time to the target. Before a stall that
//!   a stalling cut has not had yet, its typical length is added. On the plant model this predicts
//!   a brisket's finish at the end of the stall within ~0.1 h at 225-275F.
//!
//! **Lid openings** dent the core's rise for a few minutes (and the truth with it). The caller flags
//! the periods the pit is disturbed (lid open or recovering); the rate is fitted to the rest of its
//! 20 min window, leaving out those samples and the next [`AFTER_N`] minutes (the core lags). With
//! too little left the estimator holds its rate, ratio and stall timers and counts the ETA down.
//!
//! Learned from the curve, not from a class label: the cut only sets priors (and the protocol's
//! default target). The structure follows `minimeat-sim`'s two-node evaporative meat model.

use libm::{exp, log};

/// Stall onset and end thresholds on the core's rate of rise, F/h.
pub const STALL_RATE_F_H: f64 = 5.0;
pub const STALL_END_F_H: f64 = 5.0;

/// Sampling period of the estimator, s; [`FoodEstimator::update`] may be called more often.
const SAMPLE_S: f64 = 60.0;
/// The rate is the regression slope over this many samples (20 min).
const SLOPE_N: usize = 20;
/// Persistence of the stall start/end conditions, s.
const PERSIST_S: f64 = 600.0;
/// Pit smoothing, s (rides through lid openings).
const PIT_TAU_S: f64 = 900.0;
/// Samples left out of the rate after the pit was disturbed (the core's lag).
const AFTER_N: usize = 5;
/// A reading this far below the last means a new piece of meat (or the probe moved): start over.
const RESET_DROP_F: f64 = 20.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cut {
    #[default]
    Other,
    Brisket,
    PorkShoulder,
    Ribs,
    Poultry,
}

impl Cut {
    pub const ALL: [Cut; 5] = [Cut::Other, Cut::Brisket, Cut::PorkShoulder, Cut::Ribs, Cut::Poultry];

    /// Wire name (`probe/<n>/cut`, docs/protocol.md).
    pub fn name(self) -> &'static str {
        match self {
            Cut::Other => "other",
            Cut::Brisket => "brisket",
            Cut::PorkShoulder => "pork_shoulder",
            Cut::Ribs => "ribs",
            Cut::Poultry => "poultry",
        }
    }

    pub fn from_name(s: &str) -> Option<Cut> {
        Self::ALL.into_iter().find(|c| c.name() == s)
    }

    /// Default target (the probe's `hi` alarm when none is set), F.
    pub fn target_f(self) -> Option<f64> {
        match self {
            Cut::Other => None,
            Cut::Brisket | Cut::PorkShoulder => Some(203.0),
            Cut::Ribs => Some(195.0),
            Cut::Poultry => Some(165.0),
        }
    }

    /// Prior dry heating ratio `a`, per hour, until the ramp shows the real one. [est]
    fn prior_rho_h(self) -> f64 {
        match self {
            Cut::Ribs => 0.45,
            Cut::Poultry => 0.35,
            _ => 0.25,
        }
    }

    /// Typical stall length for the cuts that stall, h (added to the ETA before the stall). [est]
    fn stall_h(self) -> f64 {
        match self {
            Cut::Brisket => 4.0,
            Cut::PorkShoulder => 3.5,
            _ => 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stall {
    #[default]
    None,
    /// In the stall since this many seconds of estimator time.
    In {
        since_s: u32,
    },
    Done,
}

/// One probe's food state (`probes[n].food` in docs/protocol.md).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Food {
    /// Core rate of rise, F/h; `None` for the first 20 min.
    pub rate_f_h: Option<f64>,
    /// Time to the target, s; `None` without a target, a rate, or a pit hot enough to reach it.
    pub eta_s: Option<f64>,
    pub stall: Stall,
}

#[derive(Clone, Debug)]
pub struct FoodEstimator {
    pub cut: Cut,
    /// Time constant of the post-stall recovery of the heating ratio, h. [est: plant model]
    pub recover_h: f64,
    t_s: f64,
    acc_s: f64,
    hist: [f64; SLOPE_N],
    n: usize,
    pit: Option<f64>,
    /// Learned dry ratio, per hour; `None` until the ramp has shown one.
    a_h: Option<f64>,
    cond_s: f64,
    /// Slowest rate seen in the stall, F/h.
    stall_min: f64,
    out: Food,
    /// Absent this long, s: the estimate starts over after 10 min.
    gone_s: f64,
    /// Which of `hist` are clean (not disturbed, nor within [`AFTER_N`] samples after).
    clean: [bool; SLOPE_N],
    /// Samples since the pit was last disturbed.
    since_n: usize,
    disturbed: bool,
}

impl FoodEstimator {
    pub fn new(cut: Cut) -> Self {
        Self {
            cut,
            recover_h: 3.0,
            t_s: 0.0,
            acc_s: 0.0,
            hist: [0.0; SLOPE_N],
            n: 0,
            pit: None,
            a_h: None,
            cond_s: 0.0,
            stall_min: f64::INFINITY,
            out: Food::default(),
            gone_s: 0.0,
            clean: [true; SLOPE_N],
            since_n: usize::MAX,
            disturbed: false,
        }
    }

    pub fn food(&self) -> Food {
        self.out
    }

    /// The dry heating ratio in use, per hour (learned, else the cut's prior).
    pub fn dry_ratio_h(&self) -> f64 {
        self.a_h.unwrap_or(self.cut.prior_rho_h())
    }

    /// Heat the food draws from the pit, W, given its mass: the dry-surface convection, which the
    /// stall splits between the core and evaporation. `None` until there is a pit reading.
    pub fn load_w(&self, mass_kg: f64, core_f: f64) -> Option<f64> {
        let p = self.pit?;
        // m cp a (P - C), cp ~3.5 kJ/kg/K, F -> K, per hour -> per second
        Some(mass_kg * 3500.0 * self.dry_ratio_h() * (p - core_f).max(0.0) * (5.0 / 9.0) / 3600.0)
    }

    /// Every control period: the food probe's core reading and the pit (`None` when not `ok`), the
    /// target (the probe's `hi`, or the cut's default), and whether the pit is disturbed this period
    /// (lid open or recovering).
    pub fn update(
        &mut self,
        core_f: Option<f64>,
        pit_f: Option<f64>,
        target_f: Option<f64>,
        disturbed: bool,
        dt: f64,
    ) -> Food {
        let (Some(c), true) = (core_f.filter(|c| c.is_finite()), dt > 0.0) else {
            self.gone_s += dt;
            if self.gone_s > PERSIST_S {
                *self = Self {
                    recover_h: self.recover_h,
                    ..Self::new(self.cut)
                };
            }
            return self.out;
        };
        self.gone_s = 0.0;
        self.disturbed |= disturbed;
        // the pit the food sees on average: a lid's dip is not part of it
        if let Some(p) = pit_f.filter(|p| p.is_finite() && !disturbed) {
            let w = (dt / PIT_TAU_S).min(1.0);
            self.pit = Some(self.pit.map_or(p, |q| q + w * (p - q)));
        }
        self.acc_s += dt;
        if self.acc_s < SAMPLE_S {
            return self.out;
        }
        self.acc_s -= SAMPLE_S;
        self.t_s += SAMPLE_S;
        if self.n > 0 && c < self.hist[(self.n - 1) % SLOPE_N] - RESET_DROP_F {
            *self = Self {
                recover_h: self.recover_h,
                pit: self.pit,
                ..Self::new(self.cut)
            };
            self.t_s = SAMPLE_S;
        }
        self.since_n = if core::mem::take(&mut self.disturbed) {
            0
        } else {
            self.since_n.saturating_add(1)
        };
        self.hist[self.n % SLOPE_N] = c;
        self.clean[self.n % SLOPE_N] = self.since_n > AFTER_N;
        self.n += 1;
        if self.n < SLOPE_N {
            return self.out;
        }
        if self.clean.iter().filter(|&&k| k).count() < SLOPE_N / 2 {
            // hold through the lid; the finish moves on with the clock, and a stall start or end that
            // was building keeps building (the first clean sample checks it again)
            self.out.eta_s = self.out.eta_s.map(|e| (e - SAMPLE_S).max(0.0));
            if self.cond_s > 0.0 {
                self.cond_s += SAMPLE_S;
            }
            return self.out;
        }
        let (rate, mid) = self.slope_f_h();
        self.out.rate_f_h = Some(rate);
        let Some(p) = self.pit else {
            return self.out;
        };
        let gap = p - c;
        // the slope belongs to the middle of its samples, and so does its gap
        let rho = if p - mid > 10.0 { rate / (p - mid) } else { f64::NAN };

        // the dry ratio: the ramp after the first 30 min (the core lags the surface at first), before
        // evaporation takes over
        if rho.is_finite() && self.t_s >= 1800.0 && c < 140.0 && self.out.stall == Stall::None {
            self.a_h = Some(self.a_h.map_or(rho, |a| a.max(rho)));
        }
        let a = self.dry_ratio_h();

        self.out.stall = match self.out.stall {
            Stall::None => {
                let stalling = rate < STALL_RATE_F_H && rho < 0.4 * a && c >= 120.0 && gap > 30.0;
                self.cond_s = if stalling { self.cond_s + SAMPLE_S } else { 0.0 };
                if self.cond_s >= PERSIST_S {
                    self.cond_s = 0.0;
                    Stall::In {
                        since_s: (self.t_s - PERSIST_S) as u32,
                    }
                } else {
                    Stall::None
                }
            }
            Stall::In { since_s } => {
                // a stall that sets in slowly hovers about the onset rate: it is over once the rate
                // is clearly back up, not at the first wobble
                self.stall_min = self.stall_min.min(rate);
                self.cond_s = if rate > STALL_END_F_H.max(2.0 * self.stall_min) {
                    self.cond_s + SAMPLE_S
                } else {
                    0.0
                };
                if self.cond_s >= PERSIST_S {
                    Stall::Done
                } else {
                    Stall::In { since_s }
                }
            }
            Stall::Done => Stall::Done,
        };

        self.out.eta_s = target_f.and_then(|t| {
            if c >= t {
                return Some(0.0);
            }
            if p <= t + 2.0 || !rho.is_finite() {
                return None;
            }
            let need = log((p - c) / (p - t));
            let h = time_to(need, rho.max(0.0), a, self.recover_h)?;
            let ahead = if self.out.stall == Stall::None {
                self.cut.stall_h()
            } else {
                0.0
            };
            Some((h + ahead) * 3600.0)
        });
        self.out
    }

    /// Least-squares slope of the clean samples among the last [`SLOPE_N`], F/h, and their mean.
    fn slope_f_h(&self) -> (f64, f64) {
        // oldest first
        let pts = (0..SLOPE_N)
            .map(|i| (self.n + i) % SLOPE_N)
            .enumerate()
            .filter(|&(_, j)| self.clean[j])
            .map(|(i, j)| (i as f64, self.hist[j]));
        let (mut n, mut sx, mut sy) = (0.0, 0.0, 0.0);
        for (x, y) in pts.clone() {
            n += 1.0;
            sx += x;
            sy += y;
        }
        let (mx, my) = (sx / n, sy / n);
        let (mut sxy, mut sxx) = (0.0, 0.0);
        for (x, y) in pts {
            sxy += (x - mx) * (y - my);
            sxx += (x - mx) * (x - mx);
        }
        (sxy / sxx * 3600.0 / SAMPLE_S, my)
    }
}

/// Hours until `integral of rho dt` reaches `need`, with `rho` recovering from `rho0` to `a` with
/// time constant `tau_h`; `None` beyond two days.
fn time_to(need: f64, rho0: f64, a: f64, tau_h: f64) -> Option<f64> {
    let f = |t: f64| a * t - (a - rho0) * tau_h * (1.0 - exp(-t / tau_h)) - need;
    let (mut lo, mut hi) = (0.0, 48.0);
    if f(hi) < 0.0 {
        return None;
    }
    for _ in 0..50 {
        let mid = 0.5 * (lo + hi);
        if f(mid) < 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_order_heating_gives_its_own_eta() {
        // a dry 0.3/h first-order lag in a 250F pit, from 40F
        let mut e = FoodEstimator::new(Cut::Other);
        let (mut c, a) = (40.0, 0.3);
        let mut last = Food::default();
        for k in 0..(6 * 360) {
            let t_h = k as f64 * 10.0 / 3600.0;
            c += (250.0 - c) * a * 10.0 / 3600.0;
            last = e.update(Some(c), Some(250.0), Some(203.0), false, 10.0);
            if (t_h - 2.0).abs() < 1e-9 {
                let truth = log((250.0 - c) / 47.0) / a;
                let eta = last.eta_s.unwrap() / 3600.0;
                assert!((eta - truth).abs() < 0.1, "eta {eta} vs {truth}");
                assert!((e.dry_ratio_h() - a).abs() < 0.01, "{}", e.dry_ratio_h());
            }
        }
        assert_eq!(last.stall, Stall::None);
        assert_eq!(last.eta_s, Some(0.0));
    }

    #[test]
    fn a_new_piece_of_meat_starts_over() {
        let mut e = FoodEstimator::new(Cut::Brisket);
        for k in 0..400 {
            e.update(Some(100.0 + k as f64 * 0.05), Some(250.0), None, false, 10.0);
        }
        assert!(e.food().rate_f_h.is_some());
        e.update(Some(40.0), Some(250.0), None, false, 60.0);
        assert_eq!(e.food().rate_f_h, None);
    }

    #[test]
    fn time_to_matches_constant_ratio() {
        let t = time_to(log(2.0), 0.25, 0.25, 3.0).unwrap();
        assert!((t - log(2.0) / 0.25).abs() < 1e-9);
        assert!(time_to(1.0, 0.0, 0.0, 3.0).is_none());
    }
}
