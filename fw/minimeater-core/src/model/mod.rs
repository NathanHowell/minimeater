//! Firmware-side physical model of the egg + intake + fan: airflow (actuator inversion, `.4.5`) and
//! the steady-state heat balance (feedforward). Port of `src/minimeater/model/{params,flow,fans,thermal}.py`;
//! parameters carry the same confidence tags ([spec], [forum], [est]) as `params.py`.

pub mod actuator;
pub mod airflow;
pub mod flow;
pub mod thermal;

use crate::units::{INCH, MMH2O, c_to_k};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ambient {
    pub t_k: f64,
    pub wind_m_s: f64,
}

impl Default for Ambient {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Ambient {
    pub const DEFAULT: Self = Self {
        t_k: c_to_k(20.0),
        wind_m_s: 0.0,
    };
}

/// Big Green Egg Large. Defaults mirror `params.Egg`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Egg {
    pub outer_area_m2: f64,
    pub inner_area_m2: f64,
    pub wall_m: f64,
    pub ceramic_k: f64,
    pub ceramic_cp: f64,
    pub shell_mass_kg: f64,
    pub firebox_mass_kg: f64,
    pub emissivity_out: f64,
    pub h_in: f64,
    pub rad_fraction: f64,
    pub gas_heat_capacity: f64,
    pub stack_height_m: f64,
    pub lhv_j_kg: f64,
    pub afr_stoich: f64,
    pub excess_air: f64,
    pub combustion_eff: f64,
    pub cp_flue: f64,
    pub grate_area_m2: f64,
    pub grate_cd: f64,
    pub bed_area_m2: f64,
    pub bed_depth_m: f64,
    pub lump_d_m: f64,
    pub bed_void: f64,
    pub bed_t_k: f64,
    pub top_cd: f64,
    pub leak_area_m2: f64,
}

impl Default for Egg {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Egg {
    pub const DEFAULT: Self = Self {
        outer_area_m2: 1.25,       // [est] prolate spheroid ~0.55 x 0.80 m
        inner_area_m2: 0.95,       // [est]
        wall_m: 0.025,             // [est]
        ceramic_k: 1.3,            // [est]
        ceramic_cp: 900.0,         // [est]
        shell_mass_kg: 45.0,       // [est]
        firebox_mass_kg: 17.0,     // [est]
        emissivity_out: 0.9,       // [est]
        h_in: 10.0,                // [est]
        rad_fraction: 0.4,         // [est]
        gas_heat_capacity: 3000.0, // [est]
        stack_height_m: 0.55,      // [est]
        lhv_j_kg: 29e6,            // [est]
        afr_stoich: 10.4,          // [est]
        excess_air: 1.3,           // [est]
        combustion_eff: 0.85,      // [est]
        cp_flue: 1100.0,           // [est]
        grate_area_m2: 30e-4,      // [est]
        grate_cd: 0.6,
        bed_area_m2: 0.07, // [est]
        bed_depth_m: 0.12, // [est]
        lump_d_m: 0.03,    // [est]
        bed_void: 0.45,    // [est]
        bed_t_k: 900.0,    // [est]
        top_cd: 0.6,
        leak_area_m2: 0.3e-4, // [est] closed flap + gasket leakage
    };
}

/// Our box + the egg's stainless intake tube. Defaults mirror `params.Intake`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Intake {
    pub tube_od_m: f64,
    pub tube_id_m: f64,
    pub tube_len_m: f64,
    pub k_entry: f64,
    pub k_exit: f64,
    /// Box internals (transition, open flap, inlet grille), referenced to tube velocity.
    pub k_box: f64,
}

impl Default for Intake {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Intake {
    pub const DEFAULT: Self = Self {
        tube_od_m: 1.375 * INCH, // [forum]
        tube_id_m: 0.0316,       // [forum]
        tube_len_m: 0.10,        // [est]
        k_entry: 0.5,
        k_exit: 1.0,
        k_box: 2.0, // design target, set by CFD
    };
}

/// Daisy-wheel effective open areas, m^2 [est].
pub mod top_vent {
    pub const PETALS_CRACKED: f64 = 3e-4;
    pub const PETALS_OPEN: f64 = 8e-4;
    pub const SLIDER_QUARTER: f64 = 18e-4;
    pub const SLIDER_HALF: f64 = 37e-4;
    pub const FULL_OPEN: f64 = 75e-4;
}

/// Axial or radial fan with an assumed P-Q curve `P = Pmax s^2 (1 - (Q / (s Qmax))^n)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fan {
    pub qmax_cfm: f64,
    pub pmax_mmh2o: f64,
    pub rpm_max: f64,
    /// Lowest continuous speed; below it the firmware cycles the fan (see [`actuator::FanCycler`]).
    pub rpm_min: f64,
    pub open_area_m2: f64,
    /// Stopped-fan obstruction to natural draft, at the velocity through `open_area_m2`.
    pub stopped_k: f64,
    /// Curve exponent: 1 axial, 2 radial.
    pub n: f64,
}

impl Fan {
    /// Noctua NF-A6x25 5V PWM, the provisional pick (`docs/modeling/README.md`). [spec] + rpm_min [est].
    pub const NF_A6X25_5V: Fan = Fan {
        qmax_cfm: 17.19,
        pmax_mmh2o: 2.18,
        rpm_max: 3000.0,
        rpm_min: 600.0,
        open_area_m2: 21.6e-4,
        stopped_k: 1.5,
        n: 1.0,
    };

    pub fn qmax_m3_s(&self) -> f64 {
        self.qmax_cfm * crate::units::CFM
    }

    pub fn pmax_pa(&self) -> f64 {
        self.pmax_mmh2o * MMH2O
    }

    pub fn s_min(&self) -> f64 {
        self.rpm_min / self.rpm_max
    }

    /// Static pressure at flow q and speed fraction s (negative past free-air flow).
    pub fn pressure(&self, q_m3_s: f64, s: f64) -> f64 {
        if s <= 0.0 {
            return if q_m3_s > 0.0 { f64::NEG_INFINITY } else { 0.0 };
        }
        let x = q_m3_s / (s * self.qmax_m3_s());
        if x >= 0.0 {
            self.pmax_pa() * s * s * (1.0 - libm::pow(x, self.n))
        } else {
            self.pmax_pa() * s * s
        }
    }
}
