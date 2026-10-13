//! Lumped steady-state heat balance of the egg (gas, inner ceramic, outer ceramic). Port of
//! `thermal.py`; the dynamic version is the digital twin's plant (`.4.2`).

use libm::cbrt;

use super::flow::flue_mass;
use super::{Ambient, Egg};
use crate::numeric::brent;
use crate::units::SIGMA;

/// Outer convection coefficient: max of turbulent natural and McAdams forced, W/m^2/K.
fn h_conv_out(dt_k: f64, wind_m_s: f64) -> f64 {
    let natural = 1.31 * cbrt(dt_k.max(0.0));
    let forced = if wind_m_s > 0.0 { 5.7 + 3.8 * wind_m_s } else { 0.0 };
    natural.max(forced)
}

/// Heat lost from the outer shell surface (convection + radiation), W.
pub fn q_out(egg: &Egg, amb: &Ambient, t_out_k: f64) -> f64 {
    let dt = t_out_k - amb.t_k;
    let conv = h_conv_out(dt, amb.wind_m_s) * dt;
    let (t4, a4) = (
        t_out_k * t_out_k * t_out_k * t_out_k,
        amb.t_k * amb.t_k * amb.t_k * amb.t_k,
    );
    egg.outer_area_m2 * (conv + egg.emissivity_out * SIGMA * (t4 - a4))
}

/// Conductance between the inner and outer ceramic nodes (half-wall each), W/K.
pub fn g_wall(egg: &Egg) -> f64 {
    0.5 * (egg.inner_area_m2 + egg.outer_area_m2) * egg.ceramic_k / (egg.wall_m / 2.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SteadyState {
    pub t_wall_in_k: f64,
    pub t_wall_out_k: f64,
    pub m_air_kg_s: f64,
    pub fire_w: f64,
}

/// Air flow and wall temperatures that hold the gas node at `t_gas_k`.
///
/// Heat release and flue loss are both linear in the air flow (alpha m, beta m), so the three
/// balance equations reduce exactly to one unknown, the outer wall temperature:
///   (alpha - beta) m = q_out(T_wo) + food;   T_wi = (r alpha m + hA T_g + G T_wo) / (hA + G);
///   G (T_wi - T_wo) = q_out(T_wo),
/// solved by Brent on [T_amb, T_gas]. `None` when the gas can't be held there (flue loss exceeds
/// the heat a kg of air releases, or the pit is at or below ambient).
pub fn steady_state(egg: &Egg, amb: &Ambient, t_gas_k: f64, food_load_w: f64) -> Option<SteadyState> {
    let ha = egg.h_in * egg.inner_area_m2;
    let g = g_wall(egg);
    let alpha = egg.combustion_eff * egg.lhv_j_kg / (egg.afr_stoich * egg.excess_air);
    let beta = flue_mass(egg, 1.0) * egg.cp_flue * (t_gas_k - amb.t_k);
    if alpha <= beta || t_gas_k <= amb.t_k {
        return None;
    }
    let m_of = |t_wo: f64| (q_out(egg, amb, t_wo) + food_load_w) / (alpha - beta);
    let t_wi_of = |t_wo: f64| (egg.rad_fraction * alpha * m_of(t_wo) + ha * t_gas_k + g * t_wo) / (ha + g);
    let t_wo = brent(|t| g * (t_wi_of(t) - t) - q_out(egg, amb, t), amb.t_k, t_gas_k, 1e-9)?;
    let m = m_of(t_wo);
    Some(SteadyState {
        t_wall_in_k: t_wi_of(t_wo),
        t_wall_out_k: t_wo,
        m_air_kg_s: m,
        fire_w: alpha * m,
    })
}
