//! Lid-open detection and recovery (`.4.6`).
//!
//! Opening a kamado dumps the hot gas: the pit probe falls 10-30F in the first 20 s and keeps falling
//! at ~1F/s, far faster than anything else the egg does (a setpoint drop, a cold front or a dying
//! fire move it by a few F per minute). Once the lid shuts, the hot ceramic reheats the gas and the
//! pit climbs at 0.15-1F/s; with the lid still up it plateaus or creeps (< 0.05F/s).
//!
//! The detector only sees the drop a sample or two late, and by then the controller has already
//! reacted to it (more air, a disturbance estimate pulled down). [`LidRecovery`] therefore keeps a
//! short history of controller snapshots and hands back the one from before the drop started.

use super::Obs;

/// Samples of history: covers the detection window down to a 2 s period.
const HIST: usize = 16;

/// Lid-open detector on the pit probe alone.
#[derive(Clone, Debug)]
pub struct LidDetector {
    /// Fall below the recent peak (F) and fall rate (F/s, regression over `window_s`) that open it.
    pub drop_f: f64,
    pub open_rate_f_s: f64,
    /// Reheating that closes it again: rising at this fraction per second of the gap to setpoint
    /// (gaps under 10F count as 10F), and this far (F) above the lowest reading.
    pub close_rate_per_s: f64,
    pub close_rise_f: f64,
    /// A shut lid on a fire with no air (the output is off while the lid is up) may barely reheat:
    /// it climbs a few F off the low and sags again. With the lid up the pit only plateaus. Rising
    /// `relapse_rise_f` above the low and then falling `relapse_fall_f` from that peak closes it.
    pub relapse_rise_f: f64,
    pub relapse_fall_f: f64,
    /// With the lid up the pit keeps falling for 2+ min; turning `close_rise_f` back up within this
    /// long of detection means it was a quick peek, s.
    pub quick_s: f64,
    pub window_s: f64,
    /// Give up and resume normal control after this long (probe pulled out, lid left up): a false
    /// detection must never starve the fire for longer.
    pub max_open_s: f64,
    hist: Ring<(f64, f64)>,
    t: f64,
    open: Option<Open>,
}

#[derive(Clone, Copy, Debug)]
struct Open {
    since: f64,
    min_f: f64,
    /// Highest reading since `min_f`.
    peak_f: f64,
}

impl Default for LidDetector {
    fn default() -> Self {
        Self {
            drop_f: 10.0,
            open_rate_f_s: 0.3,
            close_rate_per_s: 0.0025,
            close_rise_f: 3.0,
            relapse_rise_f: 5.0,
            relapse_fall_f: 3.0,
            quick_s: 90.0,
            window_s: 50.0,
            max_open_s: 900.0,
            hist: Ring::default(),
            t: 0.0,
            open: None,
        }
    }
}

impl LidDetector {
    pub fn open(&self) -> bool {
        self.open.is_some()
    }

    /// Feed one pit reading. Returns `Some(n)` on the sample the lid is detected open, where `n` is
    /// how many samples ago the last pre-drop reading was taken (`n >= 1`). Non-finite readings
    /// (probe fault) are ignored.
    pub fn update(&mut self, pit_f: f64, sp_f: f64, dt: f64) -> Option<usize> {
        self.t += dt;
        if !pit_f.is_finite() {
            return None;
        }
        self.hist.push((self.t, pit_f));
        let n = self.recent();
        let slope = self.slope(n);
        if let Some(o) = self.open.as_mut() {
            if pit_f < o.min_f {
                (o.min_f, o.peak_f) = (pit_f, pit_f);
            }
            o.peak_f = o.peak_f.max(pit_f);
            let relapse = o.peak_f - o.min_f >= self.relapse_rise_f && o.peak_f - pit_f >= self.relapse_fall_f;
            let gap = (sp_f - pit_f).max(10.0);
            let reheating =
                slope.is_some_and(|s| s >= self.close_rate_per_s * gap) && pit_f >= o.min_f + self.close_rise_f;
            let up = pit_f >= o.min_f + self.close_rise_f;
            let quick = self.t - o.since <= self.quick_s && up && slope.is_some_and(|s| s > 0.0);
            if reheating || relapse || quick || self.t - o.since >= self.max_open_s {
                self.open = None;
            }
            return None;
        }
        let (Some(slope), true) = (slope, self.hist.len() >= 3) else {
            return None;
        };
        // the pre-drop peak in the window, and how many samples back it is
        let (peak_i, peak) = (0..n)
            .map(|i| (i, self.hist.back(i).1))
            .fold((0, f64::NEG_INFINITY), |a, b| if b.1 > a.1 { b } else { a });
        // the last two samples both fell: one noisy reading cannot open it
        let falling = self.hist.back(0).1 < self.hist.back(1).1 && self.hist.back(1).1 < self.hist.back(2).1;
        if peak - pit_f >= self.drop_f && slope <= -self.open_rate_f_s && falling && peak_i >= 1 {
            self.open = Some(Open {
                since: self.t,
                min_f: pit_f,
                peak_f: pit_f,
            });
            return Some(peak_i);
        }
        None
    }

    /// Samples within the window (at least the latest).
    fn recent(&self) -> usize {
        (0..self.hist.len())
            .take_while(|&i| self.t - self.hist.back(i).0 <= self.window_s + 1e-9)
            .count()
            .max(1)
    }

    /// Least-squares slope over the newest `n` samples, F/s.
    fn slope(&self, n: usize) -> Option<f64> {
        if n < 3 {
            return None;
        }
        let pts = (0..n).map(|i| self.hist.back(i));
        let (st, sy) = pts.clone().fold((0.0, 0.0), |(a, b), (t, y)| (a + t, b + y));
        let (mt, my) = (st / n as f64, sy / n as f64);
        let (sxy, sxx) = pts.fold((0.0, 0.0), |(a, b), (t, y)| {
            (a + (t - mt) * (y - my), b + (t - mt) * (t - mt))
        });
        (sxx > 0.0).then(|| sxy / sxx)
    }
}

/// Controller state at the end of a control period, restored when a detected lid opening turns out
/// to have started before the detector saw it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// Airflow commanded, CFM.
    pub q: f64,
    /// Controller-specific state: the MPC disturbance estimate (F) or the PI integral (CFM).
    pub state: f64,
}

/// What the controller should do this period instead of normal control.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Override {
    pub q: f64,
    /// Set on the period the lid is detected: roll the controller state back to this.
    pub restore: Option<Snapshot>,
}

/// Detects the lid (unless disabled), freezes the output while it is open, then replays the pre-lid
/// airflow until the pit is back near setpoint.
///
/// The ceramic is still hot, so the pre-lid airflow is the right steady command; a feedback
/// controller seeing a 100F+ error would otherwise either flare the fire or recover far too slowly.
#[derive(Clone, Debug)]
pub struct LidRecovery {
    /// `None` when detection is off (`set/lid_detect`): only [`Obs::lid`] opens it.
    pub detector: Option<LidDetector>,
    pub band_f: f64,
    pub max_s: f64,
    /// Multiple of the pre-lid airflow to replay (`set/lid_boost`): ~1.0 -> 12 min / 1-5F overshoot,
    /// 1.5 -> 6 min / 5-9F, 2.0 -> 5 min / 12-16F (model sweep).
    pub boost: f64,
    q_hold: f64,
    active_s: Option<f64>,
    was_lid: bool,
    snaps: Ring<Snapshot>,
}

impl Default for LidRecovery {
    fn default() -> Self {
        Self {
            detector: Some(LidDetector::default()),
            band_f: 20.0,
            max_s: 900.0,
            boost: 1.5,
            q_hold: 0.0,
            active_s: None,
            was_lid: false,
            snaps: Ring::default(),
        }
    }
}

impl LidRecovery {
    /// Call at the start of every period with the controller's state from the end of the previous
    /// one. Returns the airflow to command while the lid is open or recovering, else `None`.
    pub fn update(&mut self, obs: &Obs, dt: f64, prev: Snapshot) -> Option<Override> {
        let onset = self.detector.as_mut().and_then(|d| d.update(obs.pit_f, obs.sp_f, dt));
        let lid = obs.lid || self.detector.as_ref().is_some_and(LidDetector::open);
        if lid {
            let mut restore = None;
            if !self.was_lid {
                // `prev` ends period k-1, the n-th newest snapshot ends period k-n (the last one that
                // had not seen the drop).
                let snap = match onset {
                    Some(n) if n >= 2 && self.snaps.len() > 0 => self.snaps.back((n - 2).min(self.snaps.len() - 1)),
                    _ => prev,
                };
                self.q_hold = snap.q;
                restore = (snap != prev).then_some(snap);
            }
            self.was_lid = true;
            self.active_s = None;
            return Some(Override { q: 0.0, restore });
        }
        self.snaps.push(prev);
        if self.was_lid {
            self.was_lid = false;
            self.active_s = Some(0.0);
        }
        let t = self.active_s.as_mut()?;
        *t += dt;
        if (obs.sp_f - obs.pit_f).abs() < self.band_f || *t > self.max_s {
            self.active_s = None;
            return None;
        }
        Some(Override {
            q: self.q_hold * self.boost,
            restore: None,
        })
    }

    /// True while the controller thinks the lid is open (detected or reported).
    pub fn open(&self) -> bool {
        self.was_lid
    }

    /// True while the lid is open or the post-lid replay is running.
    pub fn active(&self) -> bool {
        self.was_lid || self.active_s.is_some()
    }
}

/// Fixed-capacity history, newest last.
#[derive(Clone, Debug)]
struct Ring<T> {
    buf: [T; HIST],
    head: usize,
    len: usize,
}

impl<T: Copy + Default> Default for Ring<T> {
    fn default() -> Self {
        Self {
            buf: [T::default(); HIST],
            head: 0,
            len: 0,
        }
    }
}

impl<T: Copy> Ring<T> {
    fn push(&mut self, v: T) {
        self.buf[self.head] = v;
        self.head = (self.head + 1) % HIST;
        self.len = (self.len + 1).min(HIST);
    }

    fn len(&self) -> usize {
        self.len
    }

    /// `back(0)` is the newest; `i < len`.
    fn back(&self, i: usize) -> T {
        debug_assert!(i < self.len);
        self.buf[(self.head + HIST - 1 - i) % HIST]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(pit: f64, lid: bool) -> Obs {
        Obs {
            pit_f: pit,
            sp_f: 225.0,
            lid,
            q_min: 0.0,
            q_max: 4.0,
        }
    }

    fn snap(q: f64) -> Snapshot {
        Snapshot { q, state: 0.0 }
    }

    #[test]
    fn reported_lid_replays_boosted_pre_lid_airflow() {
        let mut lr = LidRecovery {
            detector: None,
            ..LidRecovery::default()
        };
        assert_eq!(lr.update(&obs(225.0, true), 10.0, snap(1.0)).unwrap().q, 0.0);
        assert_eq!(lr.update(&obs(150.0, false), 10.0, snap(0.0)).unwrap().q, 1.5);
        assert_eq!(lr.update(&obs(215.0, false), 10.0, snap(0.0)), None);
        assert!(!lr.active());
    }

    /// Readings from the plant model, 225F, lid up for 60 s at t=60 (10 s samples).
    const LID_60S: [f64; 18] = [
        223.0, 223.0, 222.0, 223.0, 223.0, 223.0, 215.0, 198.0, 179.0, 161.0, 146.0, 133.0, 126.0, 123.0, 126.0, 128.0,
        132.0, 137.0,
    ];

    #[test]
    fn detects_open_and_close_and_rolls_back_to_the_pre_drop_state() {
        let mut lr = LidRecovery::default();
        let mut first = None;
        let mut closed = None;
        for (k, &y) in LID_60S.iter().enumerate() {
            // `prev` is the command of period k-1, which reacts to the drop from sample 6 on
            let q = if k <= 6 { 1.0 } else { k as f64 };
            let o = lr.update(&obs(y, false), 10.0, snap(q));
            if first.is_none() && lr.open() {
                first = Some((k, o.unwrap()));
            }
            if first.is_some() && closed.is_none() && !lr.open() {
                closed = Some(k);
            }
        }
        let (k, o) = first.expect("lid detected");
        assert!(k <= 7, "detected at sample {k}");
        assert_eq!(o.q, 0.0);
        assert_eq!(o.restore.map(|s| s.q), Some(1.0), "rolled back to the pre-drop command");
        let closed = closed.expect("close detected");
        assert!((15..=17).contains(&closed), "closed at sample {closed}");
        assert!(lr.active(), "replaying after close");
    }

    #[test]
    fn slow_falls_and_a_single_glitch_do_not_open_it() {
        let mut d = LidDetector::default();
        // a setpoint drop: 3F/min for 30 min
        for k in 0..180 {
            assert!(d.update(325.0 - 0.5 * k as f64, 150.0, 10.0).is_none());
        }
        let mut d = LidDetector::default();
        for k in 0..20 {
            let y = if k == 10 { 200.0 } else { 225.0 };
            assert!(d.update(y, 225.0, 10.0).is_none(), "glitch at {k}");
        }
    }

    #[test]
    fn gives_up_after_max_open() {
        let mut d = LidDetector::default();
        for y in [225.0, 225.0, 225.0, 210.0, 190.0] {
            d.update(y, 225.0, 10.0);
        }
        assert!(d.open());
        // probe pulled out: reads ambient and never reheats
        let mut t = 0.0;
        while d.open() {
            d.update(70.0, 225.0, 10.0);
            t += 10.0;
            assert!(t <= 900.0);
        }
        assert!(t >= 890.0);
    }
}
