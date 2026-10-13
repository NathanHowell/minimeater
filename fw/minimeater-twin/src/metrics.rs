//! Control metrics of a twin run (`.4.16.3`), the ones `src/minimeater/model/control_report.py` uses
//! for the Python models: how fast and how far the pit goes to the setpoint, how well it holds,
//! what a lid costs, how hard the actuators work and how long the controller was in fault.

use minimeater_app::pit::Phase;

use crate::{Report, Scenario};

/// Within this of the setpoint counts as there, F.
const BAND_F: f64 = 5.0;
/// A lid's overshoot is looked for this long after it closes, s.
const LID_AFTER_S: f64 = 1800.0;

/// One run's numbers. `NaN` where a metric does not apply (no lid) or did not happen (never
/// reached the setpoint).
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    /// Minutes from the start of the approach window to first being within 5F of its setpoint.
    pub reach_min: f64,
    /// Highest the pit went above that setpoint in the approach window, F.
    pub overshoot_f: f64,
    /// RMS and worst |error| over the hold windows, F.
    pub hold_rms_f: f64,
    pub hold_max_f: f64,
    /// Worst time for the pit to be back within 5F after a lid closes, min.
    pub lid_recovery_min: f64,
    /// Worst overshoot in the half hour after a lid closes, F.
    pub lid_overshoot_f: f64,
    /// Total flap travel, degrees per hour (servo wear).
    pub servo_deg_per_h: f64,
    /// Share of control periods with the fan running, %.
    pub fan_on_pct: f64,
    /// Minutes in the `fault` phase.
    pub fault_min: f64,
}

impl Metrics {
    pub const NAMES: [&'static str; 9] = [
        "reach_min",
        "overshoot_f",
        "hold_rms_f",
        "hold_max_f",
        "lid_recovery_min",
        "lid_overshoot_f",
        "servo_deg_per_h",
        "fan_on_pct",
        "fault_min",
    ];

    pub fn values(&self) -> [f64; 9] {
        [
            self.reach_min,
            self.overshoot_f,
            self.hold_rms_f,
            self.hold_max_f,
            self.lid_recovery_min,
            self.lid_overshoot_f,
            self.servo_deg_per_h,
            self.fan_on_pct,
            self.fault_min,
        ]
    }

    /// Event counts by wire name, for the events the run raised.
    pub fn event_counts(r: &Report) -> Vec<(&'static str, usize)> {
        let mut counts: Vec<(&'static str, usize)> = Vec::new();
        for (_, e) in &r.events {
            match counts.iter_mut().find(|(n, _)| *n == e.name()) {
                Some((_, c)) => *c += 1,
                None => counts.push((e.name(), 1)),
            }
        }
        counts.sort();
        counts
    }

    pub fn of(sc: &Scenario, r: &Report) -> Self {
        let s = &r.samples;
        let dt = s.windows(2).map(|w| w[1].t_s - w[0].t_s).next().unwrap_or(10.0);
        let sp = |i: usize| s[i].status.cook.sp_f;

        // approach: the setpoint at its end is the one to get to
        let (a0, a1) = sc.approach;
        let idx = |from: f64, to: f64| {
            s.iter()
                .enumerate()
                .filter(move |(_, x)| x.t_s >= from && x.t_s < to)
                .map(|(i, _)| i)
        };
        let sp_end = idx(a0, a1).next_back().map(sp);
        let reach_min = sp_end
            .and_then(|sp_end| idx(a0, a1).find(|&i| (s[i].pit_f - sp_end).abs() < BAND_F))
            .map_or(f64::NAN, |i| (s[i].t_s - a0) / 60.0);
        // overshoot is what happens after getting there (a step down starts above its setpoint)
        let reached = sp_end.and_then(|sp_end| idx(a0, a1).find(|&i| (s[i].pit_f - sp_end).abs() < BAND_F));
        let overshoot_f = match (sp_end, reached) {
            (Some(sp_end), Some(r)) => idx(a0, a1)
                .filter(|&i| i >= r)
                .map(|i| s[i].pit_f - sp_end)
                .fold(0.0, f64::max),
            _ => f64::NAN,
        };

        // holds
        let errs: Vec<f64> = sc
            .holds
            .iter()
            .flat_map(|&(a, b)| idx(a, b))
            .map(|i| s[i].pit_f - sp(i))
            .collect();
        let n = errs.len().max(1) as f64;
        let hold_rms_f = (errs.iter().map(|e| e * e).sum::<f64>() / n).sqrt();
        let hold_max_f = errs.iter().map(|e| e.abs()).fold(0.0, f64::max);

        // lids
        let (mut rec, mut over) = (f64::NAN, f64::NAN);
        for tc in sc.lid_closes() {
            let back = idx(tc, f64::INFINITY).find(|&i| (s[i].pit_f - sp(i)).abs() < BAND_F);
            let this = back.map_or(f64::INFINITY, |i| (s[i].t_s - tc) / 60.0);
            rec = if rec.is_nan() { this } else { rec.max(this) };
            let o = idx(tc, tc + LID_AFTER_S)
                .map(|i| s[i].pit_f - sp(i))
                .fold(0.0, f64::max);
            over = if over.is_nan() { o } else { over.max(o) };
        }

        let hours = (s.last().map_or(0.0, |x| x.t_s) - s.first().map_or(0.0, |x| x.t_s)) / 3600.0;
        let travel: f64 = s.windows(2).map(|w| (w[1].flap_deg - w[0].flap_deg).abs()).sum();
        let fan_on = s.iter().filter(|x| x.status.out.act.fan > 0.0).count() as f64;
        let fault = s.iter().filter(|x| x.status.out.phase == Phase::Fault).count() as f64;
        Metrics {
            reach_min,
            overshoot_f,
            hold_rms_f,
            hold_max_f,
            lid_recovery_min: rec,
            lid_overshoot_f: over,
            servo_deg_per_h: travel / hours.max(1e-9),
            fan_on_pct: 100.0 * fan_on / s.len().max(1) as f64,
            fault_min: fault * dt / 60.0,
        }
    }

    /// `{"name": value, ...}`, `null` for NaN and "never" (a lid that never recovered).
    pub fn json(&self) -> String {
        let fields: Vec<String> = Self::NAMES
            .iter()
            .zip(self.values())
            .map(|(n, v)| {
                if v.is_finite() {
                    format!("\"{n}\": {v:.4}")
                } else {
                    format!("\"{n}\": null")
                }
            })
            .collect();
        format!("{{{}}}", fields.join(", "))
    }
}
