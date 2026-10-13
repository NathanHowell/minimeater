"""1D pressure-drop model of the air path: our box -> intake tube -> grate -> bed -> dome -> daisy wheel.

Flows are expressed as ambient-density volumetric flow (what a fan datasheet
CFM means). Pressures in Pa. Positive `system_drop` is resistance the fan must
overcome; the chimney (stack) effect is returned separately because it helps.
"""

import math

from .params import Egg, Intake
from .units import G, air_density, air_viscosity


def _tube_area(intake: Intake) -> float:
    return math.pi * intake.tube_id_m**2 / 4.0


def tube_velocity(intake: Intake, q_m3_s: float) -> float:
    return q_m3_s / _tube_area(intake)


def _friction_factor(re: float) -> float:
    if re < 1.0:
        return 0.0
    if re < 2300.0:
        return 64.0 / re
    return 0.316 / re**0.25  # Blasius, smooth tube


def intake_drop(intake: Intake, q_m3_s: float, t_amb_k: float, include_box: bool = True) -> float:
    """Box internals + tube friction + entry/exit losses, cold air."""
    rho = air_density(t_amb_k)
    v = tube_velocity(intake, q_m3_s)
    re = rho * v * intake.tube_id_m / air_viscosity(t_amb_k)
    k = intake.k_entry + intake.k_exit + _friction_factor(re) * intake.tube_len_m / intake.tube_id_m
    if include_box:
        k += intake.k_box
    return k * 0.5 * rho * v * v


def orifice_drop(m_kg_s: float, rho: float, cd: float, area_m2: float) -> float:
    return (m_kg_s / (cd * area_m2)) ** 2 / (2.0 * rho)


def ergun_drop(egg: Egg, m_kg_s: float) -> float:
    """Packed-bed (lump charcoal) pressure drop at the bed gas temperature."""
    rho = air_density(egg.bed_t_k)
    mu = air_viscosity(egg.bed_t_k)
    v = m_kg_s / (rho * egg.bed_area_m2)  # superficial velocity
    e, d = egg.bed_void, egg.lump_d_m
    per_m = 150.0 * mu * (1 - e) ** 2 * v / (e**3 * d**2) + 1.75 * rho * (1 - e) * v * v / (e**3 * d)
    return per_m * egg.bed_depth_m


def stack_draft(egg: Egg, t_amb_k: float, t_pit_k: float) -> float:
    """Chimney effect pressure available from the hot dome column, Pa (>=0 when pit is hot)."""
    return G * egg.stack_height_m * (air_density(t_amb_k) - air_density(t_pit_k))


def egg_drop(egg: Egg, m_air_kg_s: float, t_amb_k: float, t_pit_k: float, top_area_m2: float) -> float:
    """Resistance inside the egg: grate + bed + top vent (flue gas incl. fuel mass)."""
    m_flue = m_air_kg_s * (1.0 + 1.0 / (egg.afr_stoich * egg.excess_air))
    return (
        orifice_drop(m_air_kg_s, air_density(t_amb_k), egg.grate_cd, egg.grate_area_m2)
        + ergun_drop(egg, m_air_kg_s)
        + orifice_drop(m_flue, air_density(t_pit_k), egg.top_cd, top_area_m2)
    )


def system_drop(
    egg: Egg,
    intake: Intake,
    q_m3_s: float,
    t_amb_k: float,
    t_pit_k: float,
    top_area_m2: float,
    include_box: bool = True,
) -> float:
    """Total pressure the fan must supply at flow q (before subtracting stack draft)."""
    m = q_m3_s * air_density(t_amb_k)
    return intake_drop(intake, q_m3_s, t_amb_k, include_box) + egg_drop(egg, m, t_amb_k, t_pit_k, top_area_m2)


def breakdown(egg: Egg, intake: Intake, q_m3_s: float, t_amb_k: float, t_pit_k: float, top_area_m2: float) -> dict:
    """Per-element pressure drop, for the report."""
    rho_a = air_density(t_amb_k)
    m = q_m3_s * rho_a
    m_flue = m * (1.0 + 1.0 / (egg.afr_stoich * egg.excess_air))
    box = intake_drop(intake, q_m3_s, t_amb_k, True) - intake_drop(intake, q_m3_s, t_amb_k, False)
    return {
        "box (transition+flap+grille)": box,
        "intake tube": intake_drop(intake, q_m3_s, t_amb_k, False),
        "fire grate": orifice_drop(m, rho_a, egg.grate_cd, egg.grate_area_m2),
        "charcoal bed": ergun_drop(egg, m),
        "top vent": orifice_drop(m_flue, air_density(t_pit_k), egg.top_cd, top_area_m2),
        "stack draft (helps)": -stack_draft(egg, t_amb_k, t_pit_k),
    }
