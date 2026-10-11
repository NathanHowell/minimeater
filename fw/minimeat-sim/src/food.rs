//! Food on the grate: a two-node meat model (surface layer + core) with evaporative cooling, which
//! produces the stall. The food probe reads the core.
//!
//! Heat reaches the surface by convection from the pit gas; part of it evaporates surface moisture.
//! Evaporation is a Lewis-analogy mass transfer scaled by an effective wet fraction: a fully wet
//! surface would sit at the pit's wet-bulb temperature (~115F in a kamado), but moisture has to
//! reach the surface from inside, which limits the flux. The wet fraction stays at `wet_max` until the
//! surface water reservoir is mostly gone and then tapers (bark), which ends the stall.
//!
//! All parameters [est], tuned for plausibility against the usual cook-log shape for a 13 lb packer
//! brisket (`cargo run -p minimeat-sim --example meat_curve -- <pit F>`):
//!
//! | pit | stall plateau | 203F reached |
//! |---|---|---|
//! | 225F | ~150-155F | ~18 h |
//! | 250F | ~157-163F for ~3 h | ~14 h |
//! | 275F | ~165-170F | ~11 h |

use libm::exp;

use minimeat_core::model::Ambient;

/// Saturation vapour pressure over water (Buck 1996), Pa, t in C.
fn p_sat_pa(t_c: f64) -> f64 {
    611.21 * exp((18.678 - t_c / 234.5) * (t_c / (257.14 + t_c)))
}

/// Water vapour density at partial pressure p, kg/m^3.
fn rho_vapour(p_pa: f64, t_k: f64) -> f64 {
    p_pa / (461.5 * t_k)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Meat {
    pub mass_kg: f64,
    pub area_m2: f64,
    /// Pit-gas to surface convection, W/m^2/K (gentle convection in a kamado). [est]
    pub h_w_m2k: f64,
    /// Surface layer share of the mass. [est]
    pub surface_frac: f64,
    /// Surface-to-core conductance, W/K (~ k A / half-thickness, tuned to the slab's first mode). [est]
    pub g_core_w_k: f64,
    pub cp_j_kgk: f64,
    pub wet_max: f64,
    /// Water that can evaporate before the bark stops it, kg. [est]
    pub water0_kg: f64,
    /// Share of `water0_kg` left when the bark starts to seal the surface; the wet fraction tapers to
    /// zero from there, which breaks the stall. [est]
    pub bark_frac: f64,
    /// Pit dew point, C (ambient humidity + combustion water + the meat's own vapour). [est]
    pub dew_point_c: f64,
    surface_k: f64,
    core_k: f64,
    water_kg: f64,
}

impl Meat {
    /// A 6 kg (13 lb) packer brisket from the fridge (4C): about 35 x 25 x 6 cm.
    pub fn brisket() -> Self {
        Self::new(6.0, 0.25, 4.0)
    }

    /// A 3.5 kg pork shoulder.
    pub fn pork_shoulder() -> Self {
        Self {
            g_core_w_k: 5.0,
            ..Self::new(3.5, 0.13, 4.0)
        }
    }

    /// A 1.2 kg rack of baby backs: thin, so it heats fast, and its little surface water is gone
    /// before it can hold the core (a slowdown around 160F, no plateau; 203F in ~5.7 h at 225F).
    pub fn baby_back_ribs() -> Self {
        Self {
            g_core_w_k: 4.0,
            wet_max: 0.04,
            water0_kg: 0.03,
            ..Self::new(1.2, 0.08, 4.0)
        }
        .refill()
    }

    pub fn new(mass_kg: f64, area_m2: f64, t_c: f64) -> Self {
        let t_k = t_c + 273.15;
        Self {
            mass_kg,
            area_m2,
            h_w_m2k: 7.0,
            surface_frac: 0.3,
            g_core_w_k: 4.5,
            cp_j_kgk: 3500.0,
            wet_max: 0.08,
            water0_kg: 0.17 * mass_kg,
            bark_frac: 0.25,
            dew_point_c: 30.0,
            surface_k: t_k,
            core_k: t_k,
            water_kg: 0.17 * mass_kg,
        }
    }

    /// Reset the surface water to `water0_kg` (after changing it).
    pub fn refill(mut self) -> Self {
        self.water_kg = self.water0_kg;
        self
    }

    /// Core temperature (what a food probe reads), F.
    pub fn core_f(&self) -> f64 {
        minimeat_core::units::k_to_f(self.core_k)
    }

    pub fn surface_f(&self) -> f64 {
        minimeat_core::units::k_to_f(self.surface_k)
    }

    /// Evaporation so far, kg.
    pub fn evaporated_kg(&self) -> f64 {
        self.water0_kg - self.water_kg
    }

    /// Advance by dt in pit gas at `t_gas_k`; returns the heat drawn from the gas, W.
    pub fn step(&mut self, dt: f64, t_gas_k: f64, _amb: &Ambient) -> f64 {
        let q_conv = self.h_w_m2k * self.area_m2 * (t_gas_k - self.surface_k);
        // Lewis analogy: h_m = h / (rho cp Le^(2/3)), with rho cp of hot air ~ 900 J/m^3/K, Le ~ 0.85
        let h_m = self.h_w_m2k / (900.0 * 0.9);
        let rho_s = rho_vapour(p_sat_pa(self.surface_k - 273.15), self.surface_k);
        let rho_inf = rho_vapour(p_sat_pa(self.dew_point_c), t_gas_k);
        let wet = self.wet_max * (self.water_kg / (self.bark_frac * self.water0_kg)).clamp(0.0, 1.0);
        let evap_kg_s = (wet * h_m * self.area_m2 * (rho_s - rho_inf)).max(0.0);
        let evap_kg_s = evap_kg_s.min(self.water_kg / dt);
        self.water_kg -= evap_kg_s * dt;
        let q_evap = 2.4e6 * evap_kg_s;
        let q_core = self.g_core_w_k * (self.surface_k - self.core_k);
        let c = self.mass_kg * self.cp_j_kgk;
        self.surface_k += dt * (q_conv - q_evap - q_core) / (c * self.surface_frac);
        self.core_k += dt * q_core / (c * (1.0 - self.surface_frac));
        q_conv
    }
}
