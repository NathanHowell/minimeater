//! What each scenario of the suite (`.4.16.3`) must achieve: absolute bounds on the control
//! metrics, and the events it must or must not raise. These are the product requirements. Closer to
//! the actual numbers, `baselines.json` holds what the controller does today, and the suite fails
//! if a run gets noticeably worse than that even while inside these bounds.

use crate::metrics::Metrics;

/// Upper bounds (lower is better) unless named otherwise. `None` is not checked; a metric that is
/// `NaN`/null in a run (it never reached, a lid never recovered) fails any bound it has.
#[derive(Clone, Copy, Debug)]
pub struct Expect {
    pub reach_min: Option<f64>,
    pub overshoot_f: Option<f64>,
    pub hold_rms_f: Option<f64>,
    pub hold_max_f: Option<f64>,
    pub lid_recovery_min: Option<f64>,
    pub lid_overshoot_f: Option<f64>,
    pub servo_deg_per_h: Option<f64>,
    /// Fault time must fall in this range, min.
    pub fault_min: (f64, f64),
    /// Events that must happen at least once.
    pub requires: &'static [&'static str],
    /// Events that must not: the faults and fire alarms, unless the scenario is about them.
    pub forbids: &'static [&'static str],
}

const ALARMS: &[&str] = &["fire_low", "fire_out", "fault"];

impl Expect {
    /// A routine cook: reaches 250F inside 45 min with at most 15F over, holds within 3F.
    const ROUTINE: Expect = Expect {
        reach_min: Some(45.0),
        overshoot_f: Some(15.0),
        hold_rms_f: Some(1.0),
        hold_max_f: Some(3.0),
        lid_recovery_min: None,
        lid_overshoot_f: None,
        servo_deg_per_h: Some(120.0),
        fault_min: (0.0, 0.0),
        requires: &[],
        forbids: ALARMS,
    };

    pub fn of(name: &str) -> Option<Expect> {
        let r = Self::ROUTINE;
        Some(match name {
            "kamado-brisket" => Expect {
                hold_rms_f: Some(0.5),
                hold_max_f: Some(1.5),
                lid_recovery_min: Some(25.0),
                lid_overshoot_f: Some(3.0),
                requires: &["lid_open", "stall_start", "stall_end", "alarm"],
                ..r
            },
            "kamado-hold" | "hot-day" => r,
            "stall-notify" | "stall-bump" | "stall-wrap" => Expect {
                hold_rms_f: Some(0.5),
                hold_max_f: Some(1.5),
                lid_recovery_min: Some(25.0),
                // the bumped fire is 25F hotter when the lid closes
                lid_overshoot_f: Some(4.0),
                requires: match name {
                    "stall-wrap" => &["stall_start", "stall_end", "wrap_reminder"],
                    "stall-bump" => &["stall_start", "stall_end", "stall_bump", "stall_restore"],
                    _ => &["stall_start", "stall_end"],
                },
                ..r
            },
            "chili-150" => Expect {
                reach_min: Some(35.0),
                overshoot_f: Some(10.0),
                hold_max_f: Some(2.5),
                lid_recovery_min: Some(10.0),
                lid_overshoot_f: Some(3.0),
                ..r
            },
            "hot-fast-325" => Expect {
                overshoot_f: Some(10.0),
                hold_rms_f: Some(2.0),
                hold_max_f: Some(6.0),
                lid_recovery_min: Some(15.0),
                // a hot lid-up fire flares when the lid closes: watch this one
                lid_overshoot_f: Some(20.0),
                servo_deg_per_h: Some(150.0),
                ..r
            },
            "step-up" => Expect {
                reach_min: Some(30.0),
                overshoot_f: Some(5.0),
                hold_max_f: Some(2.5),
                ..r
            },
            "step-down" => Expect {
                reach_min: Some(20.0),
                overshoot_f: Some(8.0),
                hold_max_f: Some(2.5),
                ..r
            },
            "lid-30s" | "lid-2min" => Expect {
                reach_min: Some(5.0),
                overshoot_f: Some(2.0),
                hold_rms_f: Some(0.5),
                hold_max_f: Some(1.0),
                lid_recovery_min: Some(15.0),
                lid_overshoot_f: Some(4.0),
                requires: &["lid_open", "lid_closed"],
                ..r
            },
            "cold-front" => Expect {
                hold_rms_f: Some(4.0),
                hold_max_f: Some(9.0),
                ..r
            },
            "fuel-burnout" => Expect {
                requires: &["fire_out"],
                forbids: &["fault"],
                ..r
            },
            "probe-unplug" => Expect {
                fault_min: (3.0, 8.0),
                requires: &["fault", "fault_clear"],
                forbids: &["fire_low", "fire_out"],
                ..r
            },
            "sensor-noise" => Expect {
                servo_deg_per_h: Some(220.0),
                forbids: &["fire_low", "fire_out", "fault", "lid_open"],
                ..r
            },
            "adc-outage" => Expect {
                fault_min: (1.5, 5.0),
                requires: &["fault", "fault_clear"],
                forbids: &["fire_low", "fire_out"],
                ..r
            },
            "fan-jam" => Expect {
                overshoot_f: Some(25.0),
                hold_max_f: Some(2.5),
                requires: &["fault", "fault_clear"],
                forbids: &["fire_low", "fire_out"],
                ..r
            },
            "servo-stuck" => Expect {
                forbids: &["fire_low", "fire_out", "fault"],
                ..r
            },
            "ambient-nack" => Expect {
                requires: &["fault", "fault_clear"],
                forbids: &["fire_low", "fire_out"],
                ..r
            },
            // the same cooks with no outside-air reading (the 20C fallback); the fault is the sensor's, and the cook must
            // still meet the bounds of its sighted twin
            n if n.ends_with("-blind") => {
                let sighted = Self::of(n.trim_end_matches("-blind"))?;
                Expect {
                    requires: &["fault"],
                    forbids: &["fire_low", "fire_out"],
                    ..sighted
                }
            }
            _ => return None,
        })
    }

    /// What is wrong with `m` and the events counted in a run, if anything.
    pub fn check(&self, m: &Metrics, events: &[(String, usize)]) -> Vec<String> {
        let mut bad = Vec::new();
        let mut upper = |name: &str, v: f64, bound: Option<f64>| {
            // NaN (never happened) is not within any bound
            if let Some(b) = bound
                && (v.is_nan() || v > b)
            {
                bad.push(format!("{name} {v:.2} over {b}"));
            }
        };
        upper("reach_min", m.reach_min, self.reach_min);
        upper("overshoot_f", m.overshoot_f, self.overshoot_f);
        upper("hold_rms_f", m.hold_rms_f, self.hold_rms_f);
        upper("hold_max_f", m.hold_max_f, self.hold_max_f);
        upper("lid_recovery_min", m.lid_recovery_min, self.lid_recovery_min);
        upper("lid_overshoot_f", m.lid_overshoot_f, self.lid_overshoot_f);
        upper("servo_deg_per_h", m.servo_deg_per_h, self.servo_deg_per_h);
        let (lo, hi) = self.fault_min;
        if !(lo..=hi).contains(&m.fault_min) {
            bad.push(format!("fault_min {:.2} outside {lo}..{hi}", m.fault_min));
        }
        let count = |n: &str| events.iter().find(|(e, _)| e == n).map_or(0, |(_, c)| *c);
        for n in self.requires {
            if count(n) == 0 {
                bad.push(format!("no {n} event"));
            }
        }
        for n in self.forbids {
            if count(n) > 0 {
                bad.push(format!("{} {n} event(s)", count(n)));
            }
        }
        bad
    }
}
