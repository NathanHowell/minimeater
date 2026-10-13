//! Fire-out / fuel-low detection (`.4.19`).
//!
//! A fire with fuel answers more air with more heat; the ceramic's thermal mass makes the answer
//! slow (minutes), but it comes. Two signs that it no longer does, using only what every controller
//! has (pit, setpoint, commanded and reachable airflow, the feedforward), so MPC and FF+PI share them:
//!
//! - **Out**: the pit is well below setpoint and has been falling, without slowing down, for a
//!   sustained period while the controller gives it clearly more air than the feedforward says the
//!   setpoint needs. A cold load (food, a front) also drops the pit at first, but the extra air
//!   catches it within a few minutes and the fall bottoms out; a dead fire keeps cooling at the
//!   ceramic's pace. On the plant model a 400 W cold load (two pork butts straight from the fridge)
//!   meets the condition for up to ~7 min and a dead fire within ~5.5 min, so `out_after_s` = 9 min
//!   reports every fire-out within 15 min. The calibration cook (`.1.10`) should revisit these.
//! - **Low**: the controller has been at full air for a while and the pit still sits below setpoint.
//!   The fuel is running low (or the egg cannot make this temperature in this weather, which needs the
//!   same attention).

/// `fire` in the status (docs/protocol.md).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fire {
    /// Light-off, a setpoint increase or the lid: an egg below setpoint at full air is expected.
    #[default]
    Unknown,
    Ok,
    /// At full air and still below setpoint.
    Low,
    /// More air, and the pit keeps falling.
    Out,
}

/// One control period, after the controller ran.
#[derive(Clone, Copy, Debug)]
pub struct FireObs {
    pub pit_f: f64,
    pub sp_f: f64,
    /// Airflow commanded, the feedforward's steady airflow for the setpoint, and the reachable
    /// maximum, CFM.
    pub q: f64,
    pub q_ff: f64,
    pub q_max: f64,
    /// Light-off / setpoint-increase shaping, or the lid open or recovering: being below setpoint is
    /// expected, so `Low` is not raised (a fire dying during light-off is still `Out`).
    pub expected_low: bool,
}

/// Pit history: `fall_s` back down to a 5 s period.
const HIST: usize = 128;

#[derive(Clone, Debug)]
pub struct FireWatch {
    /// `Out` needs the pit at least `out_deficit_f` under setpoint, `out_fall_f` lower than `fall_s`
    /// ago, falling over the last half of that at least `out_keep` times as fast as over the first
    /// half, and the command at least `out_air` times the feedforward, all for `out_after_s`.
    pub out_deficit_f: f64,
    pub out_fall_f: f64,
    pub fall_s: f64,
    pub out_keep: f64,
    pub out_air: f64,
    pub out_after_s: f64,
    /// A command at or above this fraction of the reachable maximum counts as full air; full air for
    /// `low_after_s` with the pit `low_deficit_f` under setpoint raises `Low`.
    pub full_frac: f64,
    pub low_after_s: f64,
    pub low_deficit_f: f64,
    /// `Low`/`Out` clear once the pit has been within `clear_band_f` of setpoint below full air for
    /// `clear_s` (more fuel, or a relit fire).
    pub clear_band_f: f64,
    pub clear_s: f64,
    state: Fire,
    t: f64,
    dying_s: f64,
    full_s: f64,
    clear_t: f64,
    hist: [(f64, f64); HIST],
    head: usize,
    len: usize,
}

impl Default for FireWatch {
    fn default() -> Self {
        Self {
            out_deficit_f: 15.0,
            out_fall_f: 5.0,
            fall_s: 600.0,
            out_keep: 0.3,
            out_air: 1.2,
            out_after_s: 540.0,
            full_frac: 0.9,
            low_after_s: 600.0,
            low_deficit_f: 10.0,
            clear_band_f: 5.0,
            clear_s: 300.0,
            state: Fire::Unknown,
            t: 0.0,
            dying_s: 0.0,
            full_s: 0.0,
            clear_t: 0.0,
            hist: [(0.0, 0.0); HIST],
            head: 0,
            len: 0,
        }
    }
}

impl FireWatch {
    pub fn state(&self) -> Fire {
        self.state
    }

    /// A non-finite pit (probe fault) leaves the state as it is.
    pub fn update(&mut self, o: &FireObs, dt: f64) -> Fire {
        self.t += dt;
        if !o.pit_f.is_finite() {
            return self.state;
        }
        self.hist[self.head] = (self.t, o.pit_f);
        self.head = (self.head + 1) % HIST;
        self.len = (self.len + 1).min(HIST);

        let y = o.pit_f;
        let dying = match (self.ago(self.fall_s / 2.0), self.ago(self.fall_s)) {
            (Some(mid), Some(then)) => {
                y <= o.sp_f - self.out_deficit_f
                    && y <= then - self.out_fall_f
                    && y < mid
                    && mid - y >= self.out_keep * (then - mid)
                    && o.q >= self.out_air * o.q_ff
            }
            _ => false,
        };
        self.dying_s = if dying { self.dying_s + dt } else { 0.0 };

        let full = o.q_max > 0.0 && o.q >= self.full_frac * o.q_max;
        self.full_s = if full { self.full_s + dt } else { 0.0 };

        let out = self.dying_s >= self.out_after_s;
        let low = self.full_s >= self.low_after_s && o.sp_f - y >= self.low_deficit_f && !o.expected_low;

        let settled = !full && (y - o.sp_f).abs() <= self.clear_band_f;
        self.clear_t = if settled { self.clear_t + dt } else { 0.0 };

        // alarms latch until the egg is back at setpoint; Low can still escalate to Out
        self.state = if out {
            Fire::Out
        } else if matches!(self.state, Fire::Out | Fire::Low) && self.clear_t < self.clear_s {
            self.state
        } else if low {
            Fire::Low
        } else if o.expected_low {
            Fire::Unknown
        } else {
            Fire::Ok
        };
        self.state
    }

    /// The newest reading at least `s` seconds old, once the history reaches that far back.
    fn ago(&self, s: f64) -> Option<f64> {
        (0..self.len)
            .map(|i| self.hist[(self.head + HIST - 1 - i) % HIST])
            .find(|&(t, _)| self.t - t >= s - 1e-9)
            .map(|(_, y)| y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(pit_f: f64, q: f64, expected_low: bool) -> FireObs {
        FireObs {
            pit_f,
            sp_f: 225.0,
            q,
            q_ff: 1.0,
            q_max: 4.0,
            expected_low,
        }
    }

    #[test]
    fn falling_on_more_air_is_out_and_a_relight_clears_it() {
        let mut w = FireWatch::default();
        for _ in 0..60 {
            assert_eq!(w.update(&obs(225.0, 1.0, false), 10.0), Fire::Ok);
        }
        // the fire dies: the pit cools 1F/min while the controller opens up
        let (mut pit, mut out_at) = (225.0, None);
        for k in 0..180 {
            pit -= 1.0 / 6.0;
            let q = (1.0 + k as f64 / 30.0).min(4.0);
            if w.update(&obs(pit, q, false), 10.0) == Fire::Out && out_at.is_none() {
                out_at = Some(k);
            }
        }
        let out_at = out_at.expect("out") as f64 / 6.0;
        // 15F under setpoint after 15 min, then 9 min of the same
        assert!(out_at <= 25.0, "out after {out_at} min");
        for _ in 0..40 {
            w.update(&obs(225.0, 1.5, false), 10.0);
        }
        assert_eq!(w.state(), Fire::Ok);
    }

    #[test]
    fn a_dip_that_bottoms_out_is_not_out() {
        let mut w = FireWatch::default();
        for k in 0..240 {
            // a cold load: 30F dip over ~8 min, caught by more air
            let t = k as f64 * 10.0;
            let pit = 225.0 - 30.0 * (t / 480.0).min(1.0) + 30.0 * ((t - 900.0) / 900.0).clamp(0.0, 1.0);
            assert_ne!(w.update(&obs(pit, 2.0, false), 10.0), Fire::Out, "at {t}s");
        }
    }

    #[test]
    fn full_air_below_setpoint_is_low_except_during_light_off() {
        let mut w = FireWatch::default();
        for k in 0..360 {
            let pit = 70.0 + 0.4 * k as f64;
            assert_eq!(w.update(&obs(pit, 4.0, true), 10.0), Fire::Unknown);
        }
        let mut w = FireWatch::default();
        let states: Vec<Fire> = (0..120).map(|_| w.update(&obs(210.0, 4.0, false), 10.0)).collect();
        assert_eq!(states[50], Fire::Ok);
        assert_eq!(states[119], Fire::Low);
    }
}
