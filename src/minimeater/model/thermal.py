"""Lumped thermal model of the egg: gas node, inner ceramic node, outer ceramic node.

The same equations serve the steady-state requirement calculation (derivatives
zero) and the dynamic simulation, so the two cannot disagree.

  fire F --(1-r)--> gas --hA_in--> inner wall --G--> outer wall --> ambient
          \\---r----------------->/
  gas also loses m_flue*cp*(Tg-Ta) out the top vent, plus food load.
"""

from dataclasses import dataclass

from scipy.optimize import fsolve

from .params import Ambient, Egg
from .units import SIGMA


def h_conv_out(dt_k: float, wind_m_s: float) -> float:
    """Outer convection coefficient: max of turbulent natural and McAdams forced."""
    natural = 1.31 * max(dt_k, 0.0) ** (1.0 / 3.0)
    forced = 5.7 + 3.8 * wind_m_s if wind_m_s > 0 else 0.0
    return max(natural, forced)


def q_out(egg: Egg, amb: Ambient, t_out_k: float) -> float:
    """Heat lost from the outer shell surface (convection + radiation), W."""
    dt = t_out_k - amb.t_k
    conv = h_conv_out(dt, amb.wind_m_s) * dt
    rad = egg.emissivity_out * SIGMA * (t_out_k**4 - amb.t_k**4)
    return egg.outer_area_m2 * (conv + rad)


def g_wall(egg: Egg) -> float:
    """Conductance between inner and outer ceramic nodes (half-wall each), W/K."""
    a_mean = 0.5 * (egg.inner_area_m2 + egg.outer_area_m2)
    return egg.ceramic_k * a_mean / (egg.wall_m / 2.0)


def heat_release(egg: Egg, m_air_kg_s: float, lit: float = 1.0) -> float:
    """Air-limited heat release from the charcoal bed, capped by how much of it is lit, W."""
    m_fuel = m_air_kg_s / (egg.afr_stoich * egg.excess_air)
    return min(egg.combustion_eff * egg.lhv_j_kg * m_fuel, egg.fire_max_w * lit)


def flue_mass(egg: Egg, m_air_kg_s: float) -> float:
    return m_air_kg_s * (1.0 + 1.0 / (egg.afr_stoich * egg.excess_air))


@dataclass(frozen=True)
class SteadyState:
    t_gas_k: float
    t_wall_in_k: float
    t_wall_out_k: float
    m_air_kg_s: float
    fire_w: float
    shell_loss_w: float
    flue_loss_w: float


def fuel_rate_kg_h(egg: Egg, ss: SteadyState) -> float:
    return ss.fire_w / (egg.combustion_eff * egg.lhv_j_kg) * 3600.0


def steady_state(egg: Egg, amb: Ambient, t_gas_k: float, food_load_w: float = 0.0) -> SteadyState:
    """Air flow and wall temperatures needed to hold the gas node at t_gas_k."""
    ha = egg.h_in * egg.inner_area_m2
    g = g_wall(egg)

    def residual(x):
        m_air = x[0] * 1e-3  # solve in g/s for conditioning
        t_wi, t_wo = x[1], x[2]
        f = egg.combustion_eff * egg.lhv_j_kg * m_air / (egg.afr_stoich * egg.excess_air)
        flue = flue_mass(egg, m_air) * egg.cp_flue * (t_gas_k - amb.t_k)
        return [
            (1 - egg.rad_fraction) * f - ha * (t_gas_k - t_wi) - flue - food_load_w,
            egg.rad_fraction * f + ha * (t_gas_k - t_wi) - g * (t_wi - t_wo),
            g * (t_wi - t_wo) - q_out(egg, amb, t_wo),
        ]

    x0 = [0.5, t_gas_k, 0.5 * (t_gas_k + amb.t_k)]
    x, _, ok, msg = fsolve(residual, x0, full_output=True)
    if ok != 1:
        raise RuntimeError(f"steady_state did not converge: {msg}")
    m_air = x[0] * 1e-3
    f = egg.combustion_eff * egg.lhv_j_kg * m_air / (egg.afr_stoich * egg.excess_air)
    return SteadyState(
        t_gas_k=t_gas_k,
        t_wall_in_k=x[1],
        t_wall_out_k=x[2],
        m_air_kg_s=m_air,
        fire_w=f,
        shell_loss_w=q_out(egg, amb, x[2]),
        flue_loss_w=flue_mass(egg, m_air) * egg.cp_flue * (t_gas_k - amb.t_k),
    )


def time_constant_h(egg: Egg) -> float:
    """Rough shell time constant: total ceramic heat capacity / small-signal loss conductance."""
    c = (egg.shell_mass_kg + egg.firebox_mass_kg) * egg.ceramic_cp
    ua = 1.0 / (1.0 / g_wall(egg) * 2 + 1.0 / (egg.outer_area_m2 * 10.0))
    return c / ua / 3600.0

