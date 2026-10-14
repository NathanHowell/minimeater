//! Airflow through the box and egg for a given flap opening and fan speed, its inversion table
//! (controller airflow -> flap + fan), and the model feedforward. Port of `sim.air_flow`,
//! `control.AirflowMap` and `control.FeedForward`.

use core::f64::consts::PI;

use super::flow::{stack_draft, system_drop, tube_velocity};
use super::thermal::steady_state;
use super::{Ambient, Egg, Fan, Intake};
use crate::control::Feedforward;
use crate::numeric::{brent, interp};
use crate::units::{CFM, air_density, f_to_k, k_to_f};

/// Extra loss of a partly open flap beyond the fully-open loss already in `k_box`.
fn damper_drop(egg: &Egg, intake: &Intake, q: f64, t_amb_k: f64, open_frac: f64) -> f64 {
    let a_tube = PI * intake.tube_id_m * intake.tube_id_m / 4.0;
    let a = open_frac.max(egg.leak_area_m2 / a_tube);
    if a >= 1.0 {
        return 0.0;
    }
    let k = (1.0 / (0.6 * a)) * (1.0 / (0.6 * a)) - (1.0 / 0.6) * (1.0 / 0.6);
    let v = tube_velocity(intake, q);
    k * 0.5 * air_density(t_amb_k) * v * v
}

/// The air path: everything needed to evaluate airflow at an operating point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AirPath {
    pub egg: Egg,
    pub intake: Intake,
    pub fan: Fan,
    /// Assumed daisy-wheel open area; the controller's disturbance estimate absorbs the error.
    pub top_area_m2: f64,
}

impl Default for AirPath {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl AirPath {
    pub const DEFAULT: Self = Self {
        egg: Egg::DEFAULT,
        intake: Intake::DEFAULT,
        fan: Fan::NF_A6X25_5V,
        top_area_m2: super::top_vent::SLIDER_QUARTER,
    };
}

impl AirPath {
    /// Ambient-density volumetric air flow into the egg, m^3/s. `s`: fan speed fraction (0 = stopped);
    /// `damper`: flap open-area fraction (0 = closed, leak only). Below the fan's minimum speed the
    /// firmware cycles it between off and minimum, so the flow is the time average of the two.
    pub fn air_flow(&self, amb: &Ambient, s: f64, damper: f64, t_pit_k: f64) -> f64 {
        let fan = &self.fan;
        if 0.0 < s && s < fan.s_min() {
            let duty = s / fan.s_min();
            let off = self.air_flow(amb, 0.0, damper, t_pit_k);
            let on = self.air_flow(amb, fan.s_min(), damper, t_pit_k);
            return (1.0 - duty) * off + duty * on;
        }
        let stack = stack_draft(&self.egg, amb.t_k, t_pit_k);
        let p_sys = |q: f64| {
            system_drop(&self.egg, &self.intake, q, amb.t_k, t_pit_k, self.top_area_m2)
                + damper_drop(&self.egg, &self.intake, q, amb.t_k, damper)
                - stack
        };
        let p_fan = |q: f64| {
            // A slow fan windmills: it never obstructs more than a stopped one.
            let v = q / fan.open_area_m2;
            let stopped = -fan.stopped_k * 0.5 * air_density(amb.t_k) * v * v;
            if s <= 0.0 {
                stopped
            } else {
                fan.pressure(q, s).max(stopped)
            }
        };
        let g = |q: f64| p_fan(q) - p_sys(q);
        if g(0.0) <= 0.0 {
            return 0.0;
        }
        let mut hi = 1e-4;
        while g(hi) > 0.0 {
            hi *= 2.0;
        }
        brent(g, 0.0, hi, 2e-12).unwrap_or(0.0)
    }

    /// Steady-state airflow (CFM) to hold `t_f`; 0 at or below ambient + 5F.
    pub fn feedforward_cfm(&self, amb: &Ambient, t_f: f64) -> f64 {
        if t_f <= k_to_f(amb.t_k) + 5.0 {
            return 0.0;
        }
        steady_state(&self.egg, amb, f_to_k(t_f), 0.0).map_or(0.0, |ss| ss.m_air_kg_s / air_density(amb.t_k) / CFM)
    }
}

/// [`Feedforward`] from the egg model at the current ambient reading.
pub struct ModelFeedforward<'a> {
    pub path: &'a AirPath,
    pub ambient: Ambient,
}

impl Feedforward for ModelFeedforward<'_> {
    fn q_cfm(&self, t_f: f64) -> f64 {
        self.path.feedforward_cfm(&self.ambient, t_f)
    }
}

const N_T: usize = 41;
const N_U: usize = 41;
const T0_F: f64 = 60.0;
const DT_F: f64 = 10.0;

/// Actuator coordinate u in [0, 2]: [0, 1] = flap open fraction with the fan stopped; [1, 2] = flap
/// open and fan speed u - 1 (damper first: the fan only runs once natural draft through the open flap
/// is not enough).
const U_GRID: [f64; N_U] = {
    let mut u = [0.0; N_U];
    let mut j = 0;
    while j < N_U {
        u[j] = if j <= 20 {
            j as f64 / 20.0
        } else {
            1.05 + (j - 21) as f64 * 0.05
        };
        j += 1;
    }
    u
};

/// Flap and fan command for an airflow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Actuation {
    /// Flap open-area fraction, 0..1.
    pub damper: f64,
    /// Fan speed fraction, 0..1 (below `Fan::s_min` it is duty-cycled).
    pub fan: f64,
}

/// Airflow (CFM) <-> actuator table over pit temperature (60..460F) and u, built from the model at
/// one ambient. f32 storage: 6.7 KB. Rebuild when [`AirflowMap::stale`] says the ambient moved.
pub struct AirflowMap {
    table: [[f32; N_U]; N_T],
    ambient: Ambient,
}

impl AirflowMap {
    /// All zeros, to be [`Self::rebuild`] in place: a static or a field fills without a 6.7 KB copy on the stack.
    pub const EMPTY: Self = Self {
        table: [[0.0; N_U]; N_T],
        ambient: Ambient::DEFAULT,
    };

    /// About 1700 root solves: run at boot and on ambient changes, off the control path. Runs to the end without
    /// yielding; see [`Self::build_async`].
    pub fn build(path: &AirPath, ambient: Ambient) -> Self {
        crate::numeric::finish(Self::build_async(path, ambient))
    }

    /// [`Self::build`] giving the executor a turn after every root solve, so a task sharing the executor waits
    /// for one solve at most.
    pub async fn build_async(path: &AirPath, ambient: Ambient) -> Self {
        let mut map = Self::EMPTY;
        map.rebuild(path, ambient).await;
        map
    }

    /// [`Self::build_async`] into this map, in place.
    pub async fn rebuild(&mut self, path: &AirPath, ambient: Ambient) {
        self.ambient = ambient;
        for (i, row) in self.table.iter_mut().enumerate() {
            let t_k = f_to_k(T0_F + i as f64 * DT_F);
            let mut running = 0.0f64;
            for (j, cell) in row.iter_mut().enumerate() {
                let u = U_GRID[j];
                let (damper, s) = if u <= 1.0 { (u, 0.0) } else { (1.0, u - 1.0) };
                // enforce monotonic in u so the table inverts
                running = running.max(path.air_flow(&ambient, s, damper, t_k) / CFM);
                *cell = running as f32;
                crate::numeric::yield_now().await;
            }
        }
    }

    /// The ambient moved enough (2 K or 1 m/s) that the table should be rebuilt.
    pub fn stale(&self, ambient: &Ambient) -> bool {
        (self.ambient.t_k - ambient.t_k).abs() > 2.0 || (self.ambient.wind_m_s - ambient.wind_m_s).abs() > 1.0
    }

    fn row(&self, t_f: f64) -> [f64; N_U] {
        let x = ((t_f - T0_F) / DT_F).clamp(0.0, (N_T - 1) as f64);
        let i = (x as usize).min(N_T - 2);
        let w = x - i as f64;
        core::array::from_fn(|j| (1.0 - w) * self.table[i][j] as f64 + w * self.table[i + 1][j] as f64)
    }

    /// Reachable airflow at pit `t_f`: flap closed + fan off (leak) .. flap open + fan full, CFM.
    pub fn limits(&self, t_f: f64) -> (f64, f64) {
        let row = self.row(t_f);
        (row[0], row[N_U - 1])
    }

    /// The airflow an actuation gives at pit `t_f`, CFM: the inverse of [`Self::actuate`] (manual mode
    /// reports and tracks with it).
    pub fn airflow(&self, t_f: f64, a: Actuation) -> f64 {
        let u = if a.fan > 0.0 {
            1.0 + a.fan.min(1.0)
        } else {
            a.damper.clamp(0.0, 1.0)
        };
        interp(u, &U_GRID, &self.row(t_f))
    }

    pub fn actuate(&self, t_f: f64, q_cfm: f64) -> Actuation {
        let u = interp(q_cfm, &self.row(t_f), &U_GRID);
        if u <= 1.0 {
            Actuation { damper: u, fan: 0.0 }
        } else {
            Actuation {
                damper: 1.0,
                fan: u - 1.0,
            }
        }
    }
}
