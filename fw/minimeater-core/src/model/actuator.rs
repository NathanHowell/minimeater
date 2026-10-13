//! From [`Actuation`](super::airflow::Actuation) to what the drivers (`.4.8`) apply: flap angle and the
//! instantaneous fan speed, with sub-minimum speeds produced by cycling the fan.

use libm::{acos, cos};

/// Butterfly flap in a round duct: angle from closed (degrees, 0..90) that exposes `open_frac` of the
/// tube area, from the projected open area `1 - cos(theta)`. [est] until CFD / bench (`.2.13`) gives
/// the real curve, which then replaces this function.
pub fn flap_angle_deg(open_frac: f64) -> f64 {
    acos(1.0 - open_frac.clamp(0.0, 1.0)).to_degrees()
}

/// Inverse of [`flap_angle_deg`]: the open-area fraction at a flap angle (degrees from closed).
pub fn open_frac(angle_deg: f64) -> f64 {
    1.0 - cos(angle_deg.clamp(0.0, 90.0).to_radians())
}

/// Produces fan speeds below the fan's minimum continuous speed by switching between off and the
/// minimum, as a first-order delta-sigma modulator with a minimum dwell so the fan neither chatters
/// nor stalls mid-start. The time-average speed equals the command; the model (`AirPath::air_flow`)
/// assumes exactly this averaging.
#[derive(Clone, Debug)]
pub struct FanCycler {
    /// Minimum time in either state, s.
    pub min_dwell_s: f64,
    on: bool,
    dwell_s: f64,
    /// Wanted minus delivered on-time, s.
    debt_s: f64,
}

impl Default for FanCycler {
    fn default() -> Self {
        Self {
            min_dwell_s: 10.0,
            on: false,
            dwell_s: f64::INFINITY,
            debt_s: 0.0,
        }
    }
}

impl FanCycler {
    /// Call every `dt` seconds (e.g. 1 s) with the commanded speed fraction; returns the speed to apply.
    pub fn tick(&mut self, s_cmd: f64, s_min: f64, dt: f64) -> f64 {
        if s_cmd <= 0.0 || s_cmd >= s_min {
            self.debt_s = 0.0;
            let on = s_cmd > 0.0;
            if on != self.on {
                self.on = on;
                self.dwell_s = 0.0;
            }
            self.dwell_s += dt;
            return s_cmd.max(0.0);
        }
        let duty = s_cmd / s_min;
        let can_switch = self.dwell_s >= self.min_dwell_s;
        if can_switch && self.on != (self.debt_s > 0.0) {
            self.on = !self.on;
            self.dwell_s = 0.0;
        }
        self.dwell_s += dt;
        self.debt_s += duty * dt - if self.on { dt } else { 0.0 };
        let cap = 2.0 * self.min_dwell_s;
        self.debt_s = self.debt_s.clamp(-cap, cap);
        if self.on { s_min } else { 0.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flap_angle_endpoints() {
        assert_eq!(flap_angle_deg(0.0), 0.0);
        assert!((flap_angle_deg(1.0) - 90.0).abs() < 1e-9);
        assert!((flap_angle_deg(0.5) - 60.0).abs() < 1e-9);
        assert!((open_frac(flap_angle_deg(0.3)) - 0.3).abs() < 1e-12);
    }

    #[test]
    fn cycler_average_matches_command_and_respects_dwell() {
        for duty in [0.05, 0.2, 0.5, 0.9] {
            let (s_min, dt) = (0.2, 1.0);
            let mut c = FanCycler::default();
            // the run before the first switch is the start-up state, not a dwell
            let (mut sum, mut last, mut run, mut min_run, mut first) = (0.0, None, 0.0, f64::INFINITY, true);
            let n = 20_000;
            for _ in 0..n {
                let s = c.tick(duty * s_min, s_min, dt);
                sum += s;
                if last.is_some_and(|l| l != s) {
                    if !first {
                        min_run = min_run.min(run);
                    }
                    first = false;
                    run = 0.0;
                }
                run += dt;
                last = Some(s);
            }
            let avg = sum / n as f64 / s_min;
            assert!((avg - duty).abs() < 0.005, "duty {duty}: got {avg}");
            assert!(min_run >= c.min_dwell_s, "duty {duty}: run of {min_run} s");
        }
    }

    #[test]
    fn cycler_passes_through_continuous_speeds() {
        let mut c = FanCycler::default();
        assert_eq!(c.tick(0.6, 0.2, 1.0), 0.6);
        assert_eq!(c.tick(0.0, 0.2, 1.0), 0.0);
    }
}
