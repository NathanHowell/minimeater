//! 1D pressure-drop model of the air path: box -> intake tube -> grate -> bed -> dome -> daisy wheel.
//! Flows are ambient-density volumetric (fan-datasheet CFM); pressures in Pa. Port of `flow.py`.

use core::f64::consts::PI;

use libm::pow;

use super::{Egg, Intake};
use crate::units::{G, air_density, air_viscosity};

fn tube_area(intake: &Intake) -> f64 {
    PI * intake.tube_id_m * intake.tube_id_m / 4.0
}

pub fn tube_velocity(intake: &Intake, q_m3_s: f64) -> f64 {
    q_m3_s / tube_area(intake)
}

fn friction_factor(re: f64) -> f64 {
    if re < 1.0 {
        0.0
    } else if re < 2300.0 {
        64.0 / re
    } else {
        0.316 / pow(re, 0.25) // Blasius, smooth tube
    }
}

/// Box internals + tube friction + entry/exit losses, cold air.
pub fn intake_drop(intake: &Intake, q_m3_s: f64, t_amb_k: f64) -> f64 {
    let rho = air_density(t_amb_k);
    let v = tube_velocity(intake, q_m3_s);
    let re = rho * v * intake.tube_id_m / air_viscosity(t_amb_k);
    let k = intake.k_entry + intake.k_exit + friction_factor(re) * intake.tube_len_m / intake.tube_id_m + intake.k_box;
    k * 0.5 * rho * v * v
}

fn orifice_drop(m_kg_s: f64, rho: f64, cd: f64, area_m2: f64) -> f64 {
    let x = m_kg_s / (cd * area_m2);
    x * x / (2.0 * rho)
}

/// Packed-bed (lump charcoal) pressure drop at the bed gas temperature.
fn ergun_drop(egg: &Egg, m_kg_s: f64) -> f64 {
    let rho = air_density(egg.bed_t_k);
    let mu = air_viscosity(egg.bed_t_k);
    let v = m_kg_s / (rho * egg.bed_area_m2);
    let (e, d) = (egg.bed_void, egg.lump_d_m);
    let per_m =
        150.0 * mu * (1.0 - e) * (1.0 - e) * v / (e * e * e * d * d) + 1.75 * rho * (1.0 - e) * v * v / (e * e * e * d);
    per_m * egg.bed_depth_m
}

/// Chimney-effect pressure from the hot dome column, Pa (>= 0 when the pit is hot).
pub fn stack_draft(egg: &Egg, t_amb_k: f64, t_pit_k: f64) -> f64 {
    G * egg.stack_height_m * (air_density(t_amb_k) - air_density(t_pit_k))
}

/// Flue gas mass flow: air plus the fuel it burns.
pub fn flue_mass(egg: &Egg, m_air_kg_s: f64) -> f64 {
    m_air_kg_s * (1.0 + 1.0 / (egg.afr_stoich * egg.excess_air))
}

/// Grate + bed + top vent.
fn egg_drop(egg: &Egg, m_air_kg_s: f64, t_amb_k: f64, t_pit_k: f64, top_area_m2: f64) -> f64 {
    orifice_drop(m_air_kg_s, air_density(t_amb_k), egg.grate_cd, egg.grate_area_m2)
        + ergun_drop(egg, m_air_kg_s)
        + orifice_drop(
            flue_mass(egg, m_air_kg_s),
            air_density(t_pit_k),
            egg.top_cd,
            top_area_m2,
        )
}

/// Total pressure the fan must supply at flow q, before subtracting the stack draft.
pub fn system_drop(egg: &Egg, intake: &Intake, q_m3_s: f64, t_amb_k: f64, t_pit_k: f64, top_area_m2: f64) -> f64 {
    let m = q_m3_s * air_density(t_amb_k);
    intake_drop(intake, q_m3_s, t_amb_k) + egg_drop(egg, m, t_amb_k, t_pit_k, top_area_m2)
}
