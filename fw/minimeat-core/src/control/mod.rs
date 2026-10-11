//! Pit controller (control ADR, bead `.4.18`): airflow-commanded offset-free MPC with an FF+PI fallback.
//! Port of `src/minimeat/model/control.py`; `tests/control_replay.rs` replays its trajectories.
//!
//! Controllers command *airflow* in CFM. Turning airflow into flap angle and fan speed (actuator
//! inversion, `.4.5`) and computing the feedforward from the egg model live outside this module; the
//! controller receives the reachable airflow range and a [`Feedforward`] each step.

pub mod autotune;
mod ffpi;
mod mpc;
mod shaper;

pub use ffpi::FfPi;
pub use mpc::Mpc;
pub use shaper::{LidRecovery, RefShaper};

/// What the controller sees each control period.
#[derive(Clone, Copy, Debug)]
pub struct Obs {
    /// Pit probe, F (noisy, offset applied).
    pub pit_f: f64,
    pub sp_f: f64,
    /// Lid-open detector output (`.4.6`).
    pub lid: bool,
    /// Reachable airflow at the current pit temperature: flap closed + fan off .. flap open + fan max.
    pub q_min: f64,
    pub q_max: f64,
}

/// Steady-state airflow (CFM) that holds a pit temperature, from the firmware-side egg model and the
/// ambient sensor.
pub trait Feedforward {
    fn q_cfm(&self, t_f: f64) -> f64;
}

impl<F: Fn(f64) -> f64> Feedforward for F {
    fn q_cfm(&self, t_f: f64) -> f64 {
        self(t_f)
    }
}

/// First-order-plus-dead-time model identified by autotune (`.4.4`) at one setpoint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fopdt {
    pub sp_f: f64,
    /// Process gain, F per CFM.
    pub k: f64,
    pub tau_s: f64,
    pub theta_s: f64,
}

/// Setpoint-indexed FOPDT models, linearly interpolated and clamped at the ends (gain scheduling).
#[derive(Clone, Debug)]
pub struct Schedule {
    models: [Fopdt; Schedule::CAPACITY],
    len: usize,
    /// Bumped by every [`Schedule::insert`], so controllers can tell a learned update from a
    /// setpoint move.
    revision: u32,
}

impl Schedule {
    pub const CAPACITY: usize = 8;

    /// Nominal BGE Large, firmware-side model (`control_report.autotune(DEFAULT_EGG)`); used until the
    /// first autotune on the user's egg.
    pub const NOMINAL: [Fopdt; 4] = [
        Fopdt {
            sp_f: 150.0,
            k: 128.0,
            tau_s: 534.0,
            theta_s: 97.0,
        },
        Fopdt {
            sp_f: 225.0,
            k: 116.3,
            tau_s: 505.0,
            theta_s: 107.0,
        },
        Fopdt {
            sp_f: 275.0,
            k: 109.8,
            tau_s: 493.0,
            theta_s: 111.0,
        },
        Fopdt {
            sp_f: 325.0,
            k: 103.7,
            tau_s: 482.0,
            theta_s: 115.0,
        },
    ];

    /// Models are sorted by setpoint; extra models beyond [`Self::CAPACITY`] are dropped. Panics if empty.
    pub fn new(models: &[Fopdt]) -> Self {
        assert!(!models.is_empty(), "schedule needs at least one model");
        let mut s = Self {
            models: [models[0]; Self::CAPACITY],
            len: 0,
            revision: 0,
        };
        for m in models.iter().take(Self::CAPACITY) {
            s.insert(*m);
        }
        s
    }

    /// Insert or replace (same setpoint) a model, keeping the list sorted; drops the input if full.
    pub fn insert(&mut self, m: Fopdt) {
        self.revision = self.revision.wrapping_add(1);
        if let Some(i) = self.models[..self.len].iter().position(|x| x.sp_f == m.sp_f) {
            self.models[i] = m;
            return;
        }
        if self.len == Self::CAPACITY {
            return;
        }
        let i = self.models[..self.len]
            .iter()
            .position(|x| x.sp_f > m.sp_f)
            .unwrap_or(self.len);
        self.models.copy_within(i..self.len, i + 1);
        self.models[i] = m;
        self.len += 1;
    }

    pub fn revision(&self) -> u32 {
        self.revision
    }

    pub fn models(&self) -> &[Fopdt] {
        &self.models[..self.len]
    }

    pub fn at(&self, sp_f: f64) -> Fopdt {
        let ms = self.models();
        let (first, last) = (ms[0], ms[ms.len() - 1]);
        if sp_f <= first.sp_f {
            return Fopdt { sp_f, ..first };
        }
        if sp_f >= last.sp_f {
            return Fopdt { sp_f, ..last };
        }
        let i = ms.iter().position(|m| m.sp_f > sp_f).unwrap_or(ms.len() - 1);
        let (lo, hi) = (ms[i - 1], ms[i]);
        let w = (sp_f - lo.sp_f) / (hi.sp_f - lo.sp_f);
        let lerp = |a: f64, b: f64| a + w * (b - a);
        Fopdt {
            sp_f,
            k: lerp(lo.k, hi.k),
            tau_s: lerp(lo.tau_s, hi.tau_s),
            theta_s: lerp(lo.theta_s, hi.theta_s),
        }
    }
}

/// Equal models; the revision counter is bookkeeping, not content.
impl PartialEq for Schedule {
    fn eq(&self, other: &Self) -> bool {
        self.models() == other.models()
    }
}

impl Default for Schedule {
    fn default() -> Self {
        Self::new(&Self::NOMINAL)
    }
}

/// The user-selectable controller (`set/controller` in docs/protocol.md).
#[derive(Clone, Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "one static instance; the MPC carries its dead-time buffer"
)]
pub enum PitController {
    Mpc(Mpc),
    FfPi(FfPi),
}

impl PitController {
    /// One control period: returns the airflow command in CFM. At most `obs.q_max`; 0 means flap
    /// closed and fan stopped (lid open).
    pub fn step(&mut self, obs: &Obs, ff: &impl Feedforward, dt: f64) -> f64 {
        match self {
            PitController::Mpc(c) => c.step(obs, ff, dt),
            PitController::FfPi(c) => c.step(obs, ff, dt),
        }
    }

    /// A period in which autotune or manual mode commanded `q_applied` instead (see [`Mpc::track`]).
    pub fn track(&mut self, obs: &Obs, ff: &impl Feedforward, q_applied: f64, dt: f64) {
        match self {
            PitController::Mpc(c) => c.track(obs, ff, q_applied, dt),
            PitController::FfPi(c) => c.track(obs, ff, q_applied, dt),
        }
    }

    pub fn schedule_mut(&mut self) -> &mut Schedule {
        match self {
            PitController::Mpc(c) => &mut c.schedule,
            PitController::FfPi(c) => &mut c.schedule,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_interpolates_and_clamps() {
        let s = Schedule::default();
        assert_eq!(s.at(100.0).k, 128.0);
        assert_eq!(s.at(400.0).tau_s, 482.0);
        let mid = s.at(250.0);
        assert!((mid.k - (116.3 + 109.8) / 2.0).abs() < 1e-12);
    }

    #[test]
    fn schedule_insert_sorts_and_replaces() {
        let mut s = Schedule::new(&[Fopdt {
            sp_f: 275.0,
            k: 1.0,
            tau_s: 1.0,
            theta_s: 1.0,
        }]);
        s.insert(Fopdt {
            sp_f: 150.0,
            k: 2.0,
            tau_s: 1.0,
            theta_s: 1.0,
        });
        s.insert(Fopdt {
            sp_f: 275.0,
            k: 3.0,
            tau_s: 1.0,
            theta_s: 1.0,
        });
        let sps: [f64; 2] = [s.models()[0].sp_f, s.models()[1].sp_f];
        assert_eq!(sps, [150.0, 275.0]);
        assert_eq!(s.models()[1].k, 3.0);
    }
}
