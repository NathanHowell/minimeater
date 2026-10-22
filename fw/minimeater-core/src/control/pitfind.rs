//! Pit probe auto-detect and the mismatch watch (ADR 0008, `minimeat-seq.4.36`).
//! Port of `src/minimeater/model/pitprobe.py`.
//!
//! - [`PitFinder`] looks at every jack each control period and names the one that is in the fire's gas: clearly above
//!   the ambient, clearly above every other probe, and rising (or already at the setpoint). It cannot tell a pit
//!   probe from meat when no pit probe is plugged in, which is what [`MismatchWatch`] is for.
//! - [`MismatchWatch`] flags a selected probe that is not behaving like a pit probe: far under the setpoint with the fan
//!   running, or far over it.

/// Samples kept for the rise: `window_s` at the 10 s control period, with room to spare.
const HIST: usize = 24;

#[derive(Clone, Copy, Debug)]
pub struct PitFinder<const N: usize> {
    /// A candidate is this far above the ambient, F.
    pub hot_f: f64,
    /// ... and this far above every other live probe, F.
    pub gap_f: f64,
    /// ... and rises at least this fast over `window_s` (F/min), or is within `near_f` of the setpoint.
    pub rise_f_min: f64,
    pub window_s: f64,
    pub near_f: f64,
    /// ... all held this long, s.
    pub hold_s: f64,
    hist: [(f64, [f64; N]); HIST],
    len: usize,
    cand: Option<usize>,
    since: f64,
}

impl<const N: usize> Default for PitFinder<N> {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl<const N: usize> PitFinder<N> {
    pub const DEFAULT: Self = Self {
        hot_f: 40.0,
        gap_f: 40.0,
        rise_f_min: 1.0,
        window_s: 180.0,
        near_f: 25.0,
        hold_s: 120.0,
        hist: [(0.0, [f64::NAN; N]); HIST],
        len: 0,
        cand: None,
        since: 0.0,
    };

    /// Forgets what it has seen (a new cook, or the selection changed).
    pub fn reset(&mut self) {
        self.len = 0;
        self.cand = None;
    }

    /// Least-squares rise of jack `j` over the window, F/min; 0 without three readings.
    fn slope_f_min(&self, j: usize) -> f64 {
        let pts = || {
            self.hist[..self.len]
                .iter()
                .filter(|(_, r)| r[j].is_finite())
                .map(|(t, r)| (*t, r[j]))
        };
        let n = pts().count();
        if n < 3 {
            return 0.0;
        }
        let n = n as f64;
        let tm = pts().map(|(t, _)| t).sum::<f64>() / n;
        let ym = pts().map(|(_, y)| y).sum::<f64>() / n;
        let var: f64 = pts().map(|(t, _)| (t - tm) * (t - tm)).sum();
        if var == 0.0 {
            return 0.0;
        }
        60.0 * pts().map(|(t, y)| (t - tm) * (y - ym)).sum::<f64>() / var
    }

    /// One control period: each jack's reading (`None` unless `ok`). Returns the jack once one has held the evidence
    /// for `hold_s`; the caller keeps the answer.
    pub fn step(&mut self, t_s: f64, ambient_f: f64, sp_f: f64, temps: &[Option<f64>; N]) -> Option<usize> {
        if self.len == HIST {
            self.hist.copy_within(1.., 0);
            self.len -= 1;
        }
        self.hist[self.len] = (t_s, temps.map(|t| t.unwrap_or(f64::NAN)));
        self.len += 1;
        let keep = self.hist[..self.len].partition_point(|(t, _)| *t <= t_s - self.window_s);
        if keep > 0 {
            self.hist.copy_within(keep..self.len, 0);
            self.len -= keep;
        }

        let top =
            (0..N)
                .filter_map(|j| temps[j].map(|t| (t, j)))
                .fold(None, |best: Option<(f64, usize)>, c| match best {
                    Some(b) if b.0 >= c.0 => Some(b),
                    _ => Some(c),
                });
        let cand = top.and_then(|(top, j)| {
            let gas = self.slope_f_min(j) >= self.rise_f_min || (top - sp_f).abs() <= self.near_f;
            let clear = (0..N)
                .filter(|k| *k != j)
                .filter_map(|k| temps[k])
                .all(|r| top - r >= self.gap_f);
            (top >= ambient_f + self.hot_f && gas && clear).then_some(j)
        });
        if cand.is_none() || cand != self.cand {
            self.cand = cand;
            self.since = t_s;
            None
        } else if t_s - self.since >= self.hold_s {
            cand
        } else {
            None
        }
    }
}

/// What a trip of the [`MismatchWatch`] does (`pit_mismatch`, docs/protocol.md).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MismatchPolicy {
    Off,
    /// Raises the `pit_mismatch` fault and keeps controlling.
    #[default]
    Warn,
    /// Raises it and shuts the flap and fan, as a lost pit probe does.
    Stop,
}

impl MismatchPolicy {
    pub const ALL: [MismatchPolicy; 3] = [MismatchPolicy::Off, MismatchPolicy::Warn, MismatchPolicy::Stop];

    pub fn name(self) -> &'static str {
        match self {
            MismatchPolicy::Off => "off",
            MismatchPolicy::Warn => "warn",
            MismatchPolicy::Stop => "stop",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == s)
    }
}

/// Why the watch tripped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trip {
    /// The fan has been running a long time and the probe is still far under the setpoint.
    Slow,
    /// The probe is far over the setpoint.
    Hot,
}

impl Trip {
    pub fn name(self) -> &'static str {
        match self {
            Trip::Slow => "slow",
            Trip::Hot => "hot",
        }
    }
}

/// Watches the selected pit probe for not behaving like one. A food probe lags the gas by tens of minutes, so the
/// controller drives full air and the real pit runs away unseen. Each signature must hold continuously:
///
/// - **slow**: the fan runs and the probe is `short_f` or more under the setpoint for `slow_s`. A pit probe lights off to
///   within 40 F in under 40 min across the plant band; meat takes longer, or never. A cook that cannot reach its
///   setpoint (out of fuel) trips it too.
/// - **hot**: the probe is `over_f` or more over the setpoint for `over_s`. A pit probe overshoots by 34 F at most in
///   the band; a food probe in a pit left on full air by 100 F.
///
/// The trip latches until [`Self::reset`].
#[derive(Clone, Copy, Debug)]
pub struct MismatchWatch {
    pub short_f: f64,
    pub slow_s: f64,
    pub over_f: f64,
    pub over_s: f64,
    tripped: Option<Trip>,
    slow_since: Option<f64>,
    hot_since: Option<f64>,
}

impl Default for MismatchWatch {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl MismatchWatch {
    pub const DEFAULT: Self = Self {
        short_f: 40.0,
        slow_s: 3600.0,
        over_f: 50.0,
        over_s: 120.0,
        tripped: None,
        slow_since: None,
        hot_since: None,
    };

    pub fn tripped(&self) -> Option<Trip> {
        self.tripped
    }

    pub fn reset(&mut self) {
        self.tripped = None;
        self.slow_since = None;
        self.hot_since = None;
    }

    pub fn step(
        &mut self,
        policy: MismatchPolicy,
        t_s: f64,
        probe_f: f64,
        sp_f: f64,
        fan_running: bool,
    ) -> Option<Trip> {
        if policy == MismatchPolicy::Off || self.tripped.is_some() {
            return self.tripped;
        }
        let held = |since: &mut Option<f64>, on: bool| {
            *since = on.then(|| since.unwrap_or(t_s));
            since.map(|s| t_s - s)
        };
        let slow = held(&mut self.slow_since, fan_running && probe_f <= sp_f - self.short_f);
        let hot = held(&mut self.hot_since, probe_f >= sp_f + self.over_f);
        if slow.is_some_and(|s| s >= self.slow_s) {
            self.tripped = Some(Trip::Slow);
        } else if hot.is_some_and(|s| s >= self.over_s) {
            self.tripped = Some(Trip::Hot);
        }
        self.tripped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AMB: f64 = 68.0;

    fn ramp(n: usize, start: f64, end: f64, over_s: f64) -> impl Iterator<Item = f64> {
        (0..n).map(move |i| (start + (end - start) * i as f64 * 10.0 / over_s).min(end))
    }

    /// Feeds rows (one per 10 s period); the time it named a jack.
    fn run<const N: usize>(f: &mut PitFinder<N>, rows: &[[Option<f64>; N]], sp: f64) -> Option<(f64, usize)> {
        rows.iter().enumerate().find_map(|(i, r)| {
            let t = i as f64 * 10.0;
            f.step(t, AMB, sp, r).map(|j| (t, j))
        })
    }

    #[test]
    fn the_pit_among_meat_is_named_in_any_jack() {
        for jack in 0..4 {
            let pit: Vec<f64> = ramp(180, AMB, 220.0, 1200.0).collect();
            let meat: Vec<f64> = ramp(180, 38.0, 45.0, 3600.0).collect();
            let rows: Vec<[Option<f64>; 4]> = (0..180)
                .map(|i| core::array::from_fn(|k| Some(if k == jack { pit[i] } else { meat[i] })))
                .collect();
            let (_, j) = run(&mut PitFinder::<4>::default(), &rows, 225.0).expect("named");
            assert_eq!(j, jack);
        }
    }

    #[test]
    fn a_lone_probe_is_named_once_hot_and_rising() {
        let rows: Vec<[Option<f64>; 4]> = ramp(180, AMB, 220.0, 1200.0)
            .map(|t| [Some(t), None, None, None])
            .collect();
        let (t, j) = run(&mut PitFinder::<4>::default(), &rows, 225.0).expect("named");
        assert_eq!(j, 0);
        assert!(rows[(t / 10.0) as usize][0].unwrap() >= AMB + 40.0);
    }

    #[test]
    fn a_cold_probe_and_two_hot_probes_are_not_named() {
        let cold: Vec<[Option<f64>; 4]> = ramp(180, 38.0, 60.0, 1800.0)
            .map(|t| [Some(t), None, None, None])
            .collect();
        assert!(run(&mut PitFinder::<4>::default(), &cold, 225.0).is_none());
        let two: Vec<[Option<f64>; 4]> = ramp(180, AMB, 220.0, 1200.0)
            .map(|t| [Some(t), Some(t - 10.0), None, None])
            .collect();
        assert!(run(&mut PitFinder::<4>::default(), &two, 225.0).is_none());
    }

    #[test]
    fn a_flat_hot_probe_is_named_only_near_the_setpoint() {
        let flat = vec![[Some(150.0), None, None, None]; 60];
        assert!(run(&mut PitFinder::<4>::default(), &flat, 225.0).is_none());
        assert!(run(&mut PitFinder::<4>::default(), &flat, 160.0).is_some());
    }

    #[test]
    fn a_pit_probe_lighting_off_slowly_does_not_trip() {
        let mut m = MismatchWatch::default();
        for (i, t) in ramp(720, AMB, 320.0, 2700.0).enumerate() {
            assert_eq!(m.step(MismatchPolicy::Warn, i as f64 * 10.0, t, 325.0, true), None);
        }
    }

    #[test]
    fn meat_on_full_air_trips_slow_and_a_far_hot_probe_trips_hot() {
        let mut m = MismatchWatch::default();
        let at = (0..720).find(|i| {
            let t = 38.0 + 112.0 * *i as f64 / 720.0;
            m.step(MismatchPolicy::Warn, *i as f64 * 10.0, t, 225.0, true).is_some()
        });
        assert_eq!((at, m.tripped()), (Some(360), Some(Trip::Slow)));
        let mut m = MismatchWatch::default();
        let at = (0..30).find(|i| {
            m.step(MismatchPolicy::Stop, *i as f64 * 10.0, 290.0, 225.0, true)
                .is_some()
        });
        assert_eq!((at, m.tripped()), (Some(12), Some(Trip::Hot)));
        m.reset();
        assert_eq!(m.tripped(), None);
    }

    #[test]
    fn no_fan_a_blip_or_off_does_not_trip() {
        let mut m = MismatchWatch::default();
        assert!((0..720).all(|i| {
            m.step(MismatchPolicy::Warn, i as f64 * 10.0, 100.0, 225.0, false)
                .is_none()
        }));
        let mut m = MismatchWatch::default();
        for i in 0..30 {
            let t = if (5..8).contains(&i) { 300.0 } else { 230.0 };
            assert_eq!(m.step(MismatchPolicy::Warn, i as f64 * 10.0, t, 225.0, true), None);
        }
        let mut m = MismatchWatch::default();
        assert!((0..720).all(|i| {
            m.step(MismatchPolicy::Off, i as f64 * 10.0, 100.0, 225.0, true)
                .is_none()
        }));
    }
}
