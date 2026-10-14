//! Autotune (`.4.4`): closed-loop, hands-off identification of the FOPDT model, which never fails.
//!
//! A failed or aborted autotune at 3 a.m. ruins a cook, so this design has no failure path at all:
//! - **The pit controller stays in charge.** Nothing here commands the actuators. The identifier
//!   only watches the airflow the controller actually applied and the pit it produced.
//! - **Learning is continuous.** An output-error fit runs on a sliding 60 min window of closed-loop
//!   data. "Autotune" only adds excitation: a small square-wave dither on the setpoint (default
//!   +/-3F), which switches itself off once the model at that setpoint is well determined, or after
//!   a time limit.
//! - **Bad data is dropped, not reported as an error.** Lid events, lid recovery, probe faults and
//!   setpoint changes restart the window. A fit that is noisy, poorly excited or implausible is
//!   discarded. The worst case is that the schedule does not change.
//! - **Updates are bounded.** An accepted fit moves the schedule entry at the nearest scheduling
//!   setpoint part of the way, and never by more than a fixed ratio per update, so no single
//!   window can wreck the controller.
//! - **Power loss loses at most the window in progress.** The schedule is the only learned state;
//!   the caller persists it whenever [`Autotune::observe`] returns an update (`.4.9`).
//!
//! The fit: for fixed tau and dead time d (whole control periods), the FOPDT
//! `x[k+1] = a x[k] + (1 - a) (K u[k-d] + c + drift t)`, `y = x + noise`, is linear in the initial
//! state, K, c and drift, so a 4x4 least-squares solve gives the best fit and its residual. tau and d
//! are found by a grid and a local search (variable projection). Python reference for the models it
//! should find: `control.identify` and `docs/modeling/control.md`.

use libm::{exp, round, sqrt};

use super::{Fopdt, Obs, Schedule};
use crate::numeric::yield_now;

/// Window length in samples (60 min at the 10 s control period).
pub const WINDOW: usize = 360;
/// Longest dead time considered, in samples (900 s at 10 s).
pub const MAX_DELAY: usize = 90;
// the fit's residual variance divides by WINDOW - 4 (four parameters)
const _: () = assert!(WINDOW > 4);

const TAU_MIN: f64 = 60.0;
const TAU_MAX: f64 = 6000.0;
const TAU_GRID: usize = 24;
use crate::consts::{BUILT_TAU_GRID, BUILT_TAU_MAX, BUILT_TAU_MIN, LN_TAU_MAX, LN_TAU_MIN, TAU_COARSE};
// build.rs computed the logs and the grid from these: a change here must be made there too
const _: () = assert!(TAU_MIN == BUILT_TAU_MIN && TAU_MAX == BUILT_TAU_MAX && TAU_GRID == BUILT_TAU_GRID);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutotuneConfig {
    /// Fit every this many samples once the window is full.
    pub fit_every: usize,
    /// Accept a fit only if its residual RMS is below this (probe noise is about 0.3F).
    pub max_rms_f: f64,
    /// ... and the gain's relative standard error is below this.
    pub max_k_rel_se: f64,
    /// ... and the pit stayed within this of the setpoint (the model is local to a setpoint, and a
    /// climb or recovery carries slow dynamics an FOPDT cannot represent).
    pub max_excursion_f: f64,
    /// ... and the pit has been within `max_excursion_f` of this setpoint for this long, s. After a
    /// climb the ceramic keeps soaking for over an hour; that slow settling biases the gain by 30%.
    pub min_settled_s: f64,
    /// ... and the window was excited: the pit's standard deviation (F; noise alone is ~0.3F) ...
    pub min_y_std_f: f64,
    /// ... and the airflow's relative standard deviation reached these. The gain's standard error
    /// alone is not enough: it ignores the tau / dead-time trade-off and is far too optimistic on a
    /// flat window.
    pub min_u_rel_std: f64,
    /// Smallest share of the way an accepted fit moves the schedule entry. The n-th accepted fit at
    /// a setpoint (since start-up or the last setpoint change) moves it by max(1/n, this): a running
    /// mean that still follows a slowly changing egg.
    pub min_blend: f64,
    /// Largest ratio change of any parameter per accepted fit.
    pub max_ratio: f64,
    /// Setpoint dither amplitude while exciting, F.
    pub dither_f: f64,
    /// Dither half-period, s (about two plant time constants).
    pub dither_half_s: f64,
    /// Excitation switches off after this long even if the model is not yet well determined.
    pub excite_max_s: f64,
    /// ... or once this many fits have been accepted at the current setpoint.
    pub excite_until_fits: u32,
}

impl Default for AutotuneConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl AutotuneConfig {
    pub const DEFAULT: Self = Self {
        fit_every: 30,
        max_rms_f: 1.5,
        max_k_rel_se: 0.08,
        max_excursion_f: 8.0,
        min_settled_s: 6300.0,
        min_y_std_f: 1.0,
        min_u_rel_std: 0.01,
        min_blend: 0.2,
        max_ratio: 1.5,
        dither_f: 3.0,
        dither_half_s: 1800.0,
        excite_max_s: 5.0 * 3600.0,
        excite_until_fits: 4,
    };
}

/// One fitted window, accepted or not (telemetry).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Estimate {
    pub model: Fopdt,
    pub rms_f: f64,
    /// Relative standard error of the gain.
    pub k_rel_se: f64,
    pub accepted: bool,
}

/// What the learner has done so far.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Status {
    pub exciting: bool,
    pub fits: u32,
    pub accepted: u32,
    pub last: Option<Estimate>,
}

/// Sliding window of closed-loop samples and the fit over it.
#[derive(Clone, Debug)]
pub struct Identifier {
    /// Applied airflow, CFM; the first `MAX_DELAY` entries precede the pit window.
    u: [f32; WINDOW + MAX_DELAY],
    /// Pit, F.
    y: [f32; WINDOW],
    n: usize,
    since_fit: usize,
    sp_f: f64,
    dt: f64,
}

impl Default for Identifier {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Identifier {
    /// Copies `o` into this one, field by field: no window-sized temporary on the stack.
    pub fn copy_from(&mut self, o: &Self) {
        // destructured so a new field cannot be missed
        let Self {
            u,
            y,
            n,
            since_fit,
            sp_f,
            dt,
        } = o;
        self.u.copy_from_slice(u);
        self.y.copy_from_slice(y);
        (self.n, self.since_fit, self.sp_f, self.dt) = (*n, *since_fit, *sp_f, *dt);
    }

    pub const DEFAULT: Self = Self {
        u: [0.0; WINDOW + MAX_DELAY],
        y: [0.0; WINDOW],
        n: 0,
        since_fit: 0,
        sp_f: f64::NAN,
        dt: 0.0,
    };
}

impl Identifier {
    pub fn reset(&mut self) {
        self.n = 0;
        self.since_fit = 0;
    }

    /// Share of a full window collected.
    pub fn fill(&self) -> f64 {
        self.n as f64 / (WINDOW + MAX_DELAY) as f64
    }

    /// Adds one control period: the pit measured at its start and the airflow applied during it.
    /// Returns `true` when a full window is due for a fit.
    pub fn push(&mut self, pit_f: f64, q_cfm: f64, sp_f: f64, dt: f64, fit_every: usize) -> bool {
        if (sp_f - self.sp_f).abs() > 0.5 || dt != self.dt {
            self.reset();
            self.sp_f = sp_f;
            self.dt = dt;
        }
        // y[k] pairs with u[MAX_DELAY + k]: the input applied after that measurement.
        self.u.copy_within(1.., 0);
        self.u[WINDOW + MAX_DELAY - 1] = q_cfm as f32;
        self.y.copy_within(1.., 0);
        self.y[WINDOW - 1] = pit_f as f32;
        self.n = (self.n + 1).min(WINDOW + MAX_DELAY);
        self.since_fit += 1;
        if self.n == WINDOW + MAX_DELAY && self.since_fit >= fit_every {
            self.since_fit = 0;
            return true;
        }
        false
    }

    /// Best fit over the current window; `None` if the data are degenerate. Runs to the end without yielding; see
    /// [`Self::fit_async`].
    pub fn fit(&self) -> Option<Fit> {
        crate::numeric::finish(self.fit_async())
    }

    /// [`Self::fit`] giving the executor a turn after every least-squares solve, so a task sharing the executor
    /// waits for one solve at most.
    pub async fn fit_async(&self) -> Option<Fit> {
        let (l0, l1) = (LN_TAU_MIN, LN_TAU_MAX);
        let mut best: Option<(f64, f64, usize)> = None;
        // coarse grid: tau log-spaced (exp of each point from the build), dead time every 3 samples
        for (i, &tau) in TAU_COARSE.iter().enumerate() {
            let lt = l0 + (l1 - l0) * i as f64 / TAU_GRID as f64;
            for d in (0..=MAX_DELAY).step_by(3) {
                if let Some(s) = self.solve(tau, d)
                    && best.is_none_or(|b| s.sse < b.0)
                {
                    best = Some((s.sse, lt, d));
                }
                yield_now().await;
            }
        }
        let (_, lt_grid, d_grid) = best?;
        // tau and dead time trade off along a valley, so profile: for each dead time near the grid
        // optimum, golden-section search log tau within one grid cell either side
        let cell = (l1 - l0) / TAU_GRID as f64;
        let (mut sse, mut lt, mut d) = (f64::INFINITY, lt_grid, d_grid);
        for nd in d_grid.saturating_sub(4)..=(d_grid + 4).min(MAX_DELAY) {
            let (l, e) = golden(
                |l| self.solve(exp(l), nd).map_or(f64::INFINITY, |s| s.sse),
                (lt_grid - cell).max(l0),
                (lt_grid + cell).min(l1),
            )
            .await;
            if e < sse {
                (sse, lt, d) = (e, l, nd);
            }
        }
        if !sse.is_finite() {
            return None;
        }
        let tau = exp(lt);
        let s = self.solve(tau, d)?;
        let sigma2 = s.sse / (WINDOW - 4) as f64;
        let ys = self.y.iter().map(|&v| f64::from(v));
        Some(Fit {
            k: s.p[1],
            tau_s: tau,
            theta_s: d as f64 * self.dt,
            rms_f: sqrt(s.sse / WINDOW as f64),
            k_rel_se: sqrt(sigma2 * s.cov_kk) / s.p[1].abs(),
            tau_at_bound: (lt - l0).abs() < 1e-6 || (lt - l1).abs() < 1e-6,
            max_excursion_f: ys.clone().map(|v| (v - self.sp_f).abs()).fold(0.0, f64::max),
            y_std_f: std(ys),
            u_rel_std: {
                let us = self.u[MAX_DELAY..].iter().map(|&v| f64::from(v));
                let mean = us.clone().sum::<f64>() / WINDOW as f64;
                std(us) / mean.max(1e-6)
            },
        })
    }

    /// Least squares for fixed (tau, d) over the basis [a^k, filtered u, 1 - a^k, filtered t].
    fn solve(&self, tau: f64, d: usize) -> Option<Solve> {
        let a = exp(-self.dt / tau);
        let mut ata = [[0.0f64; 4]; 4];
        let mut aty = [0.0f64; 4];
        let mut yy = 0.0;
        let (mut free, mut fu, mut fc, mut ft) = (1.0f64, 0.0f64, 0.0f64, 0.0f64);
        for k in 0..WINDOW {
            let row = [free, fu, fc, ft];
            let y = f64::from(self.y[k]);
            for r in 0..4 {
                for c in r..4 {
                    ata[r][c] += row[r] * row[c];
                }
                aty[r] += row[r] * y;
            }
            yy += y * y;
            // advance one period: the input applied after sample k, d periods earlier
            let u = f64::from(self.u[MAX_DELAY + k - d]);
            let hours = k as f64 * self.dt / 3600.0;
            free *= a;
            fu = a * fu + (1.0 - a) * u;
            fc = a * fc + (1.0 - a);
            ft = a * ft + (1.0 - a) * hours;
        }
        // mirror the upper triangle
        for (r, c) in [(1, 0), (2, 0), (2, 1), (3, 0), (3, 1), (3, 2)] {
            ata[r][c] = ata[c][r];
        }
        let inv = invert4(ata)?;
        let mut p = [0.0; 4];
        for (r, pr) in p.iter_mut().enumerate() {
            *pr = (0..4).map(|c| inv[r][c] * aty[c]).sum();
        }
        let sse = (yy - (0..4).map(|r| p[r] * aty[r]).sum::<f64>()).max(0.0);
        Some(Solve {
            sse,
            p,
            cov_kk: inv[1][1],
        })
    }
}

struct Solve {
    sse: f64,
    /// [initial state, K, c, drift per hour]
    p: [f64; 4],
    cov_kk: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    pub k: f64,
    pub tau_s: f64,
    pub theta_s: f64,
    pub rms_f: f64,
    pub k_rel_se: f64,
    pub tau_at_bound: bool,
    pub max_excursion_f: f64,
    pub y_std_f: f64,
    pub u_rel_std: f64,
}

fn std(xs: impl Iterator<Item = f64> + Clone) -> f64 {
    let n = xs.clone().count().max(1) as f64;
    let mean = xs.clone().sum::<f64>() / n;
    sqrt(xs.map(|x| (x - mean) * (x - mean)).sum::<f64>() / n)
}

/// Minimum of a unimodal `f` on [lo, hi] (golden-section, 30 iterations: 1e-6 of the interval).
async fn golden(f: impl Fn(f64) -> f64, mut lo: f64, mut hi: f64) -> (f64, f64) {
    const R: f64 = 0.618_033_988_749_895;
    let (mut x1, mut x2) = (hi - R * (hi - lo), lo + R * (hi - lo));
    let (mut f1, mut f2) = (f(x1), f(x2));
    for _ in 0..30 {
        if f1 <= f2 {
            (hi, x2, f2) = (x2, x1, f1);
            x1 = hi - R * (hi - lo);
            f1 = f(x1);
        } else {
            (lo, x1, f1) = (x1, x2, f2);
            x2 = lo + R * (hi - lo);
            f2 = f(x2);
        }
        yield_now().await;
    }
    if f1 <= f2 { (x1, f1) } else { (x2, f2) }
}

/// Inverse of a symmetric positive-definite 4x4 by Gauss-Jordan with partial pivoting.
fn invert4(mut a: [[f64; 4]; 4]) -> Option<[[f64; 4]; 4]> {
    let mut inv = [[0.0; 4]; 4];
    for (i, row) in inv.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    let scale = (0..4).map(|i| a[i][i].abs()).fold(0.0, f64::max);
    for c in 0..4 {
        let piv = (c..4).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        if a[piv][c].abs() <= 1e-12 * scale {
            return None;
        }
        a.swap(c, piv);
        inv.swap(c, piv);
        let d = a[c][c];
        for k in 0..4 {
            a[c][k] /= d;
            inv[c][k] /= d;
        }
        for r in 0..4 {
            if r != c {
                let f = a[r][c];
                for k in 0..4 {
                    a[r][k] -= f * a[c][k];
                    inv[r][k] -= f * inv[c][k];
                }
            }
        }
    }
    Some(inv)
}

/// The learner: identifier, acceptance and bounded schedule update, and the optional excitation.
#[derive(Clone, Debug)]
pub struct Autotune {
    pub config: AutotuneConfig,
    id: Identifier,
    status: Status,
    /// Excitation time left, s; 0 = not exciting.
    excite_left_s: f64,
    excite_t_s: f64,
    excite_sp_f: f64,
    accepted_here: u32,
    /// Time the pit has been near the user's setpoint, s.
    settled_s: f64,
    settled_sp_f: f64,
}

impl Default for Autotune {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Autotune {
    pub const DEFAULT: Self = Self {
        config: AutotuneConfig::DEFAULT,
        id: Identifier::DEFAULT,
        status: Status {
            exciting: false,
            fits: 0,
            accepted: 0,
            last: None,
        },
        excite_left_s: 0.0,
        excite_t_s: 0.0,
        excite_sp_f: 0.0,
        accepted_here: 0,
        settled_s: 0.0,
        settled_sp_f: 0.0,
    };

    pub fn new(config: AutotuneConfig) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    pub fn status(&self) -> Status {
        Status {
            exciting: self.excite_left_s > 0.0,
            ..self.status
        }
    }

    pub fn window_fill(&self) -> f64 {
        self.id.fill()
    }

    /// The user's "autotune" button: excite at the current setpoint until the model there is well
    /// determined. Safe at any time; it only dithers the setpoint by a few F.
    pub fn start(&mut self) {
        self.excite_left_s = self.config.excite_max_s;
        self.excite_t_s = 0.0;
        self.excite_sp_f = f64::NAN;
        self.accepted_here = 0;
    }

    pub fn stop(&mut self) {
        self.excite_left_s = 0.0;
    }

    /// The setpoint to hand the controller this period: the user's, plus the dither while exciting.
    /// A user setpoint change restarts the excitation count at the new setpoint.
    pub fn reference(&mut self, sp_f: f64, dt: f64) -> f64 {
        if self.excite_left_s <= 0.0 {
            return sp_f;
        }
        if sp_f != self.excite_sp_f {
            self.excite_sp_f = sp_f;
            self.excite_t_s = 0.0;
            self.accepted_here = 0;
        }
        self.excite_left_s -= dt;
        self.excite_t_s += dt;
        let half = (self.excite_t_s / self.config.dither_half_s) as u64;
        sp_f + if half.is_multiple_of(2) {
            self.config.dither_f
        } else {
            -self.config.dither_f
        }
    }

    /// Once per control period after the controller ran. `user_sp_f` is the user's setpoint (not the
    /// dithered one); `q_applied` the airflow commanded this period; `undisturbed` is false during
    /// lid-open and lid recovery, probe faults and manual overrides, which restart the window.
    ///
    /// Returns the schedule entry it changed, which the caller should persist. This runs the fit
    /// inline (~1300 small solves); the firmware splits it with [`Self::sample`] and [`Self::apply`]
    /// so the fit never runs in the control period.
    pub fn observe(
        &mut self,
        obs: &Obs,
        user_sp_f: f64,
        q_applied: f64,
        undisturbed: bool,
        dt: f64,
        schedule: &mut Schedule,
    ) -> Option<Fopdt> {
        let due = self.sample(obs, user_sp_f, q_applied, undisturbed, dt)?;
        let fit = self.window().fit();
        self.apply(due, fit, schedule)
    }

    /// The cheap half of [`Self::observe`]: records the period, and returns `Some` when the window
    /// is due for a fit. The caller then fits a copy of [`Self::window`] wherever it likes and hands
    /// the result to [`Self::apply`], however many periods later.
    pub fn sample(&mut self, obs: &Obs, user_sp_f: f64, q_applied: f64, undisturbed: bool, dt: f64) -> Option<Due> {
        if !undisturbed || obs.lid || !obs.pit_f.is_finite() || !q_applied.is_finite() {
            self.id.reset();
            return None;
        }
        if user_sp_f != self.settled_sp_f || (obs.pit_f - user_sp_f).abs() > self.config.max_excursion_f {
            self.settled_s = 0.0;
            self.settled_sp_f = user_sp_f;
        }
        self.settled_s += dt;
        self.id
            .push(obs.pit_f, q_applied, user_sp_f, dt, self.config.fit_every)
            .then_some(Due {
                sp_f: user_sp_f,
                settled: self.settled_s >= self.config.min_settled_s,
            })
    }

    /// The window [`Self::sample`] said is due.
    pub fn window(&self) -> &Identifier {
        &self.id
    }

    /// Takes a fit of the window `due` refers to (`None`: the data were degenerate) into the schedule,
    /// if it passes. Returns the schedule entry it changed, which the caller should persist.
    pub fn apply(&mut self, due: Due, fit: Option<Fit>, schedule: &mut Schedule) -> Option<Fopdt> {
        let fit = fit?;
        self.status.fits += 1;
        let c = self.config;
        let accepted = fit.rms_f <= c.max_rms_f
            && fit.k > 0.0
            && fit.k_rel_se <= c.max_k_rel_se
            && !fit.tau_at_bound
            && fit.max_excursion_f <= c.max_excursion_f
            && due.settled
            && fit.y_std_f >= c.min_y_std_f
            && fit.u_rel_std >= c.min_u_rel_std;
        let at = scheduling_point(due.sp_f);
        let est = Fopdt {
            sp_f: at,
            k: fit.k,
            tau_s: fit.tau_s,
            theta_s: fit.theta_s,
        };
        self.status.last = Some(Estimate {
            model: est,
            rms_f: fit.rms_f,
            k_rel_se: fit.k_rel_se,
            accepted,
        });
        if !accepted {
            return None;
        }
        self.status.accepted += 1;
        self.accepted_here += 1;
        if self.accepted_here >= c.excite_until_fits {
            self.excite_left_s = 0.0;
        }
        let w = (1.0 / f64::from(self.accepted_here)).max(c.min_blend);
        let new = blend(schedule.at(at), est, w, c.max_ratio);
        schedule.insert(new);
        Some(new)
    }
}

/// A window due for a fit ([`Autotune::sample`]): the user's setpoint it was taken at, and whether
/// the pit had settled there long enough to trust it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Due {
    pub sp_f: f64,
    pub settled: bool,
}

/// Moves `old` towards `est` by `w`, each parameter by at most `max_ratio` (the dead time also by
/// 10 s, since a fit may put it at 0).
fn blend(old: Fopdt, est: Fopdt, w: f64, max_ratio: f64) -> Fopdt {
    let mv = |o: f64, n: f64, slack: f64| (o + w * (n - o)).clamp(o / max_ratio - slack, o * max_ratio + slack);
    Fopdt {
        sp_f: est.sp_f,
        k: mv(old.k, est.k, 0.0),
        tau_s: mv(old.tau_s, est.tau_s, 0.0),
        theta_s: mv(old.theta_s, est.theta_s, 10.0).max(0.0),
    }
}

/// Learned models are stored at 25F steps across the 150-325F scope (8 entries = schedule capacity).
pub fn scheduling_point(sp_f: f64) -> f64 {
    (round(sp_f / 25.0) * 25.0).clamp(150.0, 325.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    use libm::log;

    #[test]
    fn built_constants_match_libm_bit_for_bit() {
        assert_eq!(LN_TAU_MIN.to_bits(), log(TAU_MIN).to_bits());
        assert_eq!(LN_TAU_MAX.to_bits(), log(TAU_MAX).to_bits());
        for (i, tau) in TAU_COARSE.iter().enumerate() {
            let lt = log(TAU_MIN) + (log(TAU_MAX) - log(TAU_MIN)) * i as f64 / TAU_GRID as f64;
            assert_eq!(tau.to_bits(), exp(lt).to_bits(), "grid point {i}");
        }
    }

    /// A discrete FOPDT driven by `input`, with deterministic pseudo-noise, recorded into a window.
    fn record(k: f64, tau: f64, d: usize, input: impl Fn(usize) -> f64, noise: f64) -> Identifier {
        let dt = 10.0;
        let a = exp(-dt / tau);
        let mut id = Identifier::default();
        let mut hist = [1.0f64; MAX_DELAY + 1];
        let mut x = 225.0;
        let mut seed = 7u32;
        for i in 0..(WINDOW + MAX_DELAY) {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let n = (f64::from(seed >> 8) / f64::from(1u32 << 24) - 0.5) * 2.0 * noise;
            let u = input(i);
            id.push(x + n, u, 225.0, dt, usize::MAX);
            hist.copy_within(1.., 0);
            hist[MAX_DELAY] = u;
            x = a * x + (1.0 - a) * (225.0 + k * (hist[MAX_DELAY - d] - 1.0));
        }
        id
    }

    /// Square-wave-ish input with a slow wander, like the MPC's under setpoint dither.
    fn dithered(i: usize) -> f64 {
        1.0 + if (i / 90).is_multiple_of(2) { 0.08 } else { -0.08 } + 0.02 * libm::sin(i as f64 / 37.0)
    }

    /// Polls `fut` to its end, counting the times it gave the executor a turn.
    fn turns<T>(fut: impl core::future::Future<Output = T>) -> (T, usize) {
        let mut fut = core::pin::pin!(fut);
        let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
        let mut n = 0;
        loop {
            if let core::task::Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                return (v, n);
            }
            n += 1;
        }
    }

    #[test]
    fn the_fit_yields_between_solves_and_gives_the_same_answer() {
        let id = record(116.0, 505.0, 11, dithered, 0.5);
        let (stepwise, n) = turns(id.fit_async());
        // 25 tau x 31 dead times on the grid, then 30 iterations at each of nine dead times
        assert!(n >= 25 * 31 + 9 * 30, "{n}");
        assert_eq!(stepwise, id.fit());
    }

    #[test]
    fn the_airflow_map_yields_after_every_solve_and_gives_the_same_table() {
        let (path, amb) = (
            crate::model::airflow::AirPath::default(),
            crate::model::Ambient::default(),
        );
        let (stepwise, n) = turns(crate::model::airflow::AirflowMap::build_async(&path, amb));
        assert_eq!(n, 41 * 41);
        let whole = crate::model::airflow::AirflowMap::build(&path, amb);
        for t in [150.0, 225.0, 325.0] {
            assert_eq!(stepwise.limits(t), whole.limits(t));
        }
    }

    #[test]
    fn fits_closed_loop_like_inputs() {
        for (k, tau, d) in [(116.0, 505.0, 11), (48.0, 444.0, 13), (157.0, 546.0, 10)] {
            let f = record(k, tau, d, dithered, 0.5).fit().unwrap();
            assert!((f.k / k - 1.0).abs() < 0.03, "{f:?}");
            assert!((f.tau_s / tau - 1.0).abs() < 0.05, "{f:?}");
            assert!((f.theta_s - d as f64 * 10.0).abs() <= 10.0, "{f:?}");
            assert!(f.k_rel_se < 0.05 && f.rms_f < 0.4, "{f:?}");
        }
    }

    #[test]
    fn unexcited_window_is_not_trusted() {
        if let Some(f) = record(116.0, 505.0, 11, |_| 1.0, 0.5).fit() {
            assert!(f.k_rel_se > AutotuneConfig::default().max_k_rel_se, "{f:?}");
        }
    }

    #[test]
    fn lid_restarts_the_window() {
        let mut at = Autotune::default();
        let mut sched = Schedule::default();
        let obs = Obs {
            pit_f: 225.0,
            sp_f: 225.0,
            lid: false,
            q_min: 0.0,
            q_max: 4.0,
        };
        for _ in 0..100 {
            at.observe(&obs, 225.0, 1.0, true, 10.0, &mut sched);
        }
        assert!(at.window_fill() > 0.2);
        at.observe(&Obs { lid: true, ..obs }, 225.0, 0.0, true, 10.0, &mut sched);
        assert_eq!(at.window_fill(), 0.0);
        at.observe(&obs, 225.0, 1.0, false, 10.0, &mut sched);
        assert_eq!(at.window_fill(), 0.0, "lid recovery is not plant data either");
    }

    #[test]
    fn updates_are_blended_and_bounded() {
        let old = Schedule::default().at(225.0);
        let wild = Fopdt {
            sp_f: 225.0,
            k: 10_000.0,
            tau_s: 1.0,
            theta_s: 0.0,
        };
        let new = blend(old, wild, 1.0, 1.5);
        assert!((new.k - old.k * 1.5).abs() < 1e-9);
        assert!((new.tau_s - old.tau_s / 1.5).abs() < 1e-9);
        assert!(new.theta_s >= old.theta_s / 1.5 - 10.0);
        let close = Fopdt { k: old.k * 1.1, ..old };
        assert!((blend(old, close, 0.5, 1.5).k - old.k * 1.05).abs() < 1e-9);
    }

    #[test]
    fn dither_runs_then_switches_off() {
        let mut at = Autotune::new(AutotuneConfig {
            excite_max_s: 2000.0,
            ..AutotuneConfig::default()
        });
        assert_eq!(at.reference(225.0, 10.0), 225.0);
        at.start();
        assert_eq!(at.reference(225.0, 10.0), 228.0);
        let mut seen_low = false;
        for _ in 0..300 {
            seen_low |= at.reference(225.0, 10.0) == 222.0;
        }
        assert!(seen_low);
        assert_eq!(at.reference(225.0, 10.0), 225.0, "time limit ends excitation");
        assert!(!at.status().exciting);
    }

    #[test]
    fn scheduling_points() {
        assert_eq!(scheduling_point(230.0), 225.0);
        assert_eq!(scheduling_point(240.0), 250.0);
        assert_eq!(scheduling_point(100.0), 150.0);
        assert_eq!(scheduling_point(400.0), 325.0);
    }
}
