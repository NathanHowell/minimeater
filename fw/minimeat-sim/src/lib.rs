//! Dynamic plant model of a Big Green Egg Large for the host digital twin (`.4.16.2`) and the
//! scenario suite (`.4.16.3`). Port of `src/minimeat/model/sim.py` (3-node thermal, air-limited fire
//! with lit-area spread, 1-D flow path with stack draft, proportional flap with leak, fan curve),
//! parity-tested against it, plus what the prototype lacks: fuel depletion, fire going out, and a
//! food model with an evaporative stall. Those extensions are off by default so parity holds.
//!
//! Scope is the BGE Large (project scope: 150-325F kamado cooking); other cookers are not modelled.
#![cfg_attr(not(test), no_std)]

pub mod food;
pub mod hal;
pub mod rng;

use minimeat_core::model::airflow::AirPath;
use minimeat_core::model::flow::flue_mass;
use minimeat_core::model::thermal::{g_wall, q_out, steady_state};
use minimeat_core::model::{Ambient, Egg};
use minimeat_core::units::{CFM, air_density, f_to_k, k_to_f};

pub use food::Meat;
pub use rng::Rng;

/// Lid up: effective top opening, extra gas->ambient conductance, air reaching the fire from above. [est]
pub const LID_OPEN_TOP_AREA_M2: f64 = 0.15;
pub const LID_OPEN_G_W_K: f64 = 80.0;
pub const LID_OPEN_EXTRA_AIR_KG_S: f64 = 1.5e-3;

/// Fire and sensor dynamics (`params.Egg` fields the steady-state model does not need).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dynamics {
    /// Heat-release lag to air changes, s. [est]
    pub fire_tau_s: f64,
    /// Bed-limited heat release when fully lit, W. [est]
    pub fire_max_w: f64,
    /// Fraction of the bed lit by a starter cube. [est]
    pub lit_initial: f64,
    /// Logistic spread time constant of the lit area, s. [est]
    pub ignition_tau_s: f64,
    /// Pit probe first-order lag, s. [est]
    pub probe_tau_s: f64,
}

impl Default for Dynamics {
    fn default() -> Self {
        Self {
            fire_tau_s: 120.0,
            fire_max_w: 20e3,
            lit_initial: 0.03,
            ignition_tau_s: 300.0,
            probe_tau_s: 20.0,
        }
    }
}

/// Fuel and fire-out behaviour. The defaults (endless fuel, a fire that never goes out) match the
/// Python prototype; scenarios turn them on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fuel {
    /// Lump charcoal left, kg (a full BGE Large firebox holds about 4 kg). [est]
    pub kg: f64,
    /// Below this the shrinking bed caps heat release proportionally. [est]
    pub low_kg: f64,
    /// Air-limited release per unit lit area below which the bed smoulders and dies back, W; 0 = never.
    pub sustain_w: f64,
    /// Die-back time constant of the lit area when starved, s. [est]
    pub out_tau_s: f64,
}

impl Default for Fuel {
    fn default() -> Self {
        Self {
            kg: f64::INFINITY,
            low_kg: 0.4,
            sustain_w: 0.0,
            out_tau_s: 900.0,
        }
    }
}

/// What the actuators are doing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Actuators {
    /// Flap open-area fraction, 0..1.
    pub damper: f64,
    /// Fan speed fraction, 0..1 (below the fan minimum this is the time-average of cycling).
    pub fan: f64,
}

/// Weather and user actions, set by the scenario.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Environment {
    pub ambient: Ambient,
    /// Daisy-wheel open area actually set on the egg, m^2.
    pub top_area_m2: f64,
    pub lid_open: bool,
    /// Extra constant heat load on the gas node (food not modelled by [`Meat`]), W.
    pub food_load_w: f64,
}

#[derive(Clone, Debug)]
pub struct Plant {
    pub path: AirPath,
    pub dynamics: Dynamics,
    pub fuel: Fuel,
    /// Food on the grate, read by the food probes.
    pub meats: [Option<Meat>; 3],
    t_gas_k: f64,
    t_wall_in_k: f64,
    t_wall_out_k: f64,
    fire_w: f64,
    lit: f64,
    probe_k: f64,
    air_m3_s: f64,
}

impl Plant {
    /// Cold egg at ambient with a starter cube lit.
    pub fn cold(path: AirPath, dynamics: Dynamics, ambient: &Ambient) -> Self {
        let t = ambient.t_k;
        Self::with_state(path, dynamics, [t, t, t], 0.0, dynamics.lit_initial)
    }

    /// Egg in thermal equilibrium at `t_f` (fully lit), as after a long hold.
    pub fn steady(path: AirPath, dynamics: Dynamics, ambient: &Ambient, t_f: f64, food_load_w: f64) -> Option<Self> {
        let t_g = f_to_k(t_f);
        let ss = steady_state(&path.egg, ambient, t_g, food_load_w)?;
        Some(Self::with_state(
            path,
            dynamics,
            [t_g, ss.t_wall_in_k, ss.t_wall_out_k],
            ss.fire_w,
            1.0,
        ))
    }

    fn with_state(path: AirPath, dynamics: Dynamics, t: [f64; 3], fire_w: f64, lit: f64) -> Self {
        Self {
            path,
            dynamics,
            fuel: Fuel::default(),
            meats: [None; 3],
            t_gas_k: t[0],
            t_wall_in_k: t[1],
            t_wall_out_k: t[2],
            fire_w,
            lit,
            probe_k: t[0],
            air_m3_s: 0.0,
        }
    }

    /// Advance by `dt` seconds (explicit Euler; 2 s matches the reference).
    pub fn step(&mut self, dt: f64, act: Actuators, env: &Environment) {
        let egg: &Egg = &self.path.egg;
        let d = &self.dynamics;
        let amb = &env.ambient;
        // the probe lags the gas temperature from the start of the step, as in the reference
        self.probe_k += (self.t_gas_k - self.probe_k) * dt / d.probe_tau_s;

        let top = if env.lid_open {
            LID_OPEN_TOP_AREA_M2
        } else {
            env.top_area_m2
        };
        let path = AirPath {
            top_area_m2: top,
            ..self.path
        };
        let q = path.air_flow(amb, act.fan, act.damper, self.t_gas_k);
        self.air_m3_s = q;
        let m_air = q * air_density(amb.t_k) + if env.lid_open { LID_OPEN_EXTRA_AIR_KG_S } else { 0.0 };

        let air_limited = egg.combustion_eff * egg.lhv_j_kg * m_air / (egg.afr_stoich * egg.excess_air);
        let starved = self.fuel.sustain_w > 0.0 && air_limited < self.fuel.sustain_w * self.lit;
        self.lit += if starved {
            -dt * self.lit / self.fuel.out_tau_s
        } else {
            dt * self.lit * (1.0 - self.lit) / d.ignition_tau_s
        };
        let bed = if self.fuel.kg.is_finite() {
            (self.fuel.kg / self.fuel.low_kg).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let release = air_limited.min(d.fire_max_w * self.lit * bed);
        self.fire_w += (release - self.fire_w) * dt / d.fire_tau_s;
        if self.fuel.kg.is_finite() {
            self.fuel.kg = (self.fuel.kg - self.fire_w / (egg.combustion_eff * egg.lhv_j_kg) * dt).max(0.0);
        }

        let t_g = self.t_gas_k;
        let mut food_w = env.food_load_w;
        for meat in self.meats.iter_mut().flatten() {
            food_w += meat.step(dt, t_g, amb);
        }
        let flue = flue_mass(egg, m_air) * egg.cp_flue * (t_g - amb.t_k);
        let lid_loss = if env.lid_open {
            LID_OPEN_G_W_K * (t_g - amb.t_k)
        } else {
            0.0
        };
        let ha = egg.h_in * egg.inner_area_m2;
        let q_gw = ha * (t_g - self.t_wall_in_k);
        let q_wall = g_wall(egg) * (self.t_wall_in_k - self.t_wall_out_k);
        let c_wi = (egg.firebox_mass_kg + egg.shell_mass_kg / 2.0) * egg.ceramic_cp;
        let c_wo = (egg.shell_mass_kg / 2.0) * egg.ceramic_cp;
        self.t_gas_k +=
            dt * ((1.0 - egg.rad_fraction) * self.fire_w - q_gw - flue - lid_loss - food_w) / egg.gas_heat_capacity;
        self.t_wall_in_k += dt * (egg.rad_fraction * self.fire_w + q_gw - q_wall) / c_wi;
        self.t_wall_out_k += dt * (q_wall - q_out(egg, amb, self.t_wall_out_k)) / c_wo;
    }

    /// Pit probe reading (lagged, noise-free), F.
    pub fn pit_probe_f(&self) -> f64 {
        k_to_f(self.probe_k)
    }

    pub fn gas_f(&self) -> f64 {
        k_to_f(self.t_gas_k)
    }

    pub fn shell_f(&self) -> f64 {
        k_to_f(self.t_wall_out_k)
    }

    /// Air drawn in over the last step, CFM.
    pub fn air_cfm(&self) -> f64 {
        self.air_m3_s / CFM
    }

    pub fn fire_w(&self) -> f64 {
        self.fire_w
    }

    pub fn lit(&self) -> f64 {
        self.lit
    }
}
