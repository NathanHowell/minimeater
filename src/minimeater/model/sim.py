"""Closed-loop dynamic simulation of the egg + intake + fan against a pluggable controller.

Prototype for the Rust plant model (minimeat-seq.4.2): same node structure as
thermal.steady_state, explicit Euler at a fixed step.
"""

import math
from dataclasses import dataclass, field
from typing import Protocol

import numpy as np
from scipy.optimize import brentq

from . import flow, thermal
from .fans import Fan
from .params import (
    LID_OPEN_EXTRA_AIR_KG_S,
    LID_OPEN_G_W_K,
    LID_OPEN_TOP_AREA_M2,
    Ambient,
    Egg,
    Intake,
)
from .units import CFM, air_density, f_to_k, k_to_f


def _stopped_fan_drop(fan: Fan, q: float, t_amb_k: float) -> float:
    """Resistance of a stopped fan to air pulled through it by the egg's convective draft."""
    v = q / fan.open_area_m2
    return fan.stopped_k * 0.5 * air_density(t_amb_k) * v * v


def damper_drop(egg: Egg, intake: Intake, q: float, t_amb_k: float, open_frac: float) -> float:
    """Extra loss of a partly open flap beyond the fully-open loss already in k_box."""
    a_tube = math.pi * intake.tube_id_m**2 / 4.0
    a = max(open_frac, egg.leak_area_m2 / a_tube)
    if a >= 1.0:
        return 0.0
    k = (1.0 / (0.6 * a)) ** 2 - (1.0 / 0.6) ** 2
    v = flow.tube_velocity(intake, q)
    return k * 0.5 * air_density(t_amb_k) * v * v


def air_flow(
    egg: Egg,
    intake: Intake,
    fan: Fan,
    amb: Ambient,
    s: float,
    damper: float,
    t_pit_k: float,
    top_area_m2: float,
) -> float:
    """Ambient-density volumetric air flow into the egg, m^3/s.

    s: fan speed fraction (0 = stopped), damper: flap open-area fraction (0 = closed, leak only).
    Below the fan's minimum speed the firmware cycles it between off and minimum, so the flow
    is the time-average of those two states.
    """
    if 0.0 < s < fan.s_min:
        duty = s / fan.s_min
        off = air_flow(egg, intake, fan, amb, 0.0, damper, t_pit_k, top_area_m2)
        on = air_flow(egg, intake, fan, amb, fan.s_min, damper, t_pit_k, top_area_m2)
        return (1 - duty) * off + duty * on
    stack = flow.stack_draft(egg, amb.t_k, t_pit_k)

    def p_sys(q):
        return (
            flow.system_drop(egg, intake, q, amb.t_k, t_pit_k, top_area_m2)
            + damper_drop(egg, intake, q, amb.t_k, damper)
            - stack
        )

    def p_fan(q):
        # A slow fan windmills: it never obstructs more than a stopped one.
        stopped = -_stopped_fan_drop(fan, q, amb.t_k)
        return stopped if s <= 0.0 else max(fan.pressure(q, s), stopped)

    return _root_from_zero(lambda q: p_fan(q) - p_sys(q))


def output_map(u: float, damper_full_at: float = 0.5) -> tuple[float, float]:
    """Damper-first mapping: flap opens over [0, damper_full_at], fan ramps over [damper_full_at, 1]."""
    damper = min(u / damper_full_at, 1.0)
    fan = max((u - damper_full_at) / (1.0 - damper_full_at), 0.0)
    return damper, fan


def _root_from_zero(g) -> float:
    if g(0.0) <= 0.0:
        return 0.0
    hi = 1e-4
    while g(hi) > 0.0:
        hi *= 2.0
    return brentq(g, 0.0, hi)


@dataclass
class Event:
    t_s: float
    setpoint_f: float | None = None
    lid_open_s: float = 0.0
    ambient: Ambient | None = None
    food_load_w: float | None = None
    top_area_m2: float | None = None


@dataclass(frozen=True)
class Obs:
    """What the firmware can see: noisy pit probe, setpoint, lid detector, on-board ambient sensor."""

    t_s: float
    pit_f: float
    setpoint_f: float
    lid: bool
    ambient: Ambient


class Controller(Protocol):
    def step(self, obs: Obs, dt: float) -> tuple[float, float]:
        """Return (damper open fraction, fan speed fraction)."""
        ...


@dataclass
class SimResult:
    t_s: np.ndarray
    pit_f: np.ndarray
    setpoint_f: np.ndarray
    output: np.ndarray  # damper-first equivalent: damper/2, or 0.5 + fan/2 once the fan runs
    damper: np.ndarray
    fan: np.ndarray
    air_cfm: np.ndarray
    fire_w: np.ndarray
    lid: np.ndarray
    meta: dict = field(default_factory=dict)


def simulate(
    egg: Egg,
    intake: Intake,
    fan: Fan,
    amb: Ambient,
    top_area_m2: float,
    events: list[Event],
    duration_s: float,
    controller: Controller,
    setpoint_f: float = 225.0,
    dt: float = 2.0,
    control_period_s: float = 10.0,
    noise_f: float = 0.0,
    seed: int = 0,
    init_f: float | None = None,
    food_load_w: float = 0.0,
) -> SimResult:
    """Run the plant against a controller. init_f=None is a cold light-off; otherwise start at steady state."""
    rng = np.random.default_rng(seed)
    n = int(duration_s / dt)
    ha = egg.h_in * egg.inner_area_m2
    g = thermal.g_wall(egg)
    c_wi = (egg.firebox_mass_kg + egg.shell_mass_kg / 2) * egg.ceramic_cp
    c_wo = (egg.shell_mass_kg / 2) * egg.ceramic_cp

    if init_f is None:
        t_g = t_wi = t_wo = amb.t_k
        fire, lit = 0.0, egg.lit_initial
    else:
        ss = thermal.steady_state(egg, amb, f_to_k(init_f), food_load_w)
        t_g, t_wi, t_wo = ss.t_gas_k, ss.t_wall_in_k, ss.t_wall_out_k
        fire, lit = ss.fire_w, 1.0
    probe = t_g
    sp_f = setpoint_f
    top_set = top_area_m2
    lid_until = -1.0
    damper, s = 0.0, 0.0
    next_control = 0.0
    ev = sorted(events, key=lambda e: e.t_s)
    keys = ("t", "pit", "sp", "u", "damper", "fan", "q", "fire", "lid")
    out = {k: np.zeros(n) for k in keys}

    for i in range(n):
        t = i * dt
        while ev and ev[0].t_s <= t:
            e = ev.pop(0)
            if e.setpoint_f is not None:
                sp_f = e.setpoint_f
            if e.lid_open_s > 0:
                lid_until = t + e.lid_open_s
            if e.ambient is not None:
                amb = e.ambient
            if e.food_load_w is not None:
                food_load_w = e.food_load_w
            if e.top_area_m2 is not None:
                top_set = e.top_area_m2
        lid = t < lid_until
        probe += (t_g - probe) * dt / egg.probe_tau_s
        pit_f = k_to_f(probe) + (rng.normal(0.0, noise_f) if noise_f > 0 else 0.0)
        if t >= next_control:
            damper, s = controller.step(Obs(t, pit_f, sp_f, lid, amb), control_period_s)
            next_control += control_period_s

        top = LID_OPEN_TOP_AREA_M2 if lid else top_set
        q = air_flow(egg, intake, fan, amb, s, damper, t_g, top)
        m_air = q * air_density(amb.t_k) + (LID_OPEN_EXTRA_AIR_KG_S if lid else 0.0)
        lit += dt * lit * (1.0 - lit) / egg.ignition_tau_s
        fire += (thermal.heat_release(egg, m_air, lit) - fire) * dt / egg.fire_tau_s

        flue = thermal.flue_mass(egg, m_air) * egg.cp_flue * (t_g - amb.t_k)
        lid_loss = LID_OPEN_G_W_K * (t_g - amb.t_k) if lid else 0.0
        q_gw = ha * (t_g - t_wi)
        q_wall = g * (t_wi - t_wo)
        t_g += dt * ((1 - egg.rad_fraction) * fire - q_gw - flue - lid_loss - food_load_w) / egg.gas_heat_capacity
        t_wi += dt * (egg.rad_fraction * fire + q_gw - q_wall) / c_wi
        t_wo += dt * (q_wall - thermal.q_out(egg, amb, t_wo)) / c_wo

        u = 0.5 + s / 2 if s > 0 else damper / 2
        row = (t, pit_f, sp_f, u, damper, s, q / CFM, fire, lid)
        for k, v in zip(keys, row):
            out[k][i] = v

    return SimResult(
        out["t"], out["pit"], out["sp"], out["u"], out["damper"], out["fan"], out["q"], out["fire"],
        out["lid"].astype(bool),
    )
