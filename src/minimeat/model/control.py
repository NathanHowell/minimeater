"""Candidate pit controllers, evaluated in the Python plant model before porting to Rust.

Everything a controller knows about the egg comes from a *firmware-side* model
(nominal parameters, assumed top-vent setting); the plant it runs against may
differ (parameter band, real top vent), so model mismatch is part of the test.

Controllers:
  PercentPid  - baseline: PI on a 0-100% output, damper-first mapping (HeaterMeter-like).
  FfPi        - airflow-commanded PI: feedforward from the steady-state model, actuator
                inversion (airflow -> damper/fan), reference shaping, SIMC gains from
                an identified FOPDT model, gain-scheduled by setpoint, anti-windup.
  Mpc         - offset-free linear MPC on the same identified model with a Kalman
                disturbance estimate and an asymmetric cost (overshoot costs more than
                undershoot because a kamado cannot be actively cooled).
"""

import math
from dataclasses import dataclass, field

import numpy as np
from scipy.optimize import least_squares, minimize

from . import thermal
from .fans import Fan
from .params import TOP_VENT, Ambient, Egg, Intake
from .sim import Obs, air_flow, output_map, simulate
from .units import CFM, air_density, f_to_k, k_to_f

# ---------------------------------------------------------------- firmware-side model


@dataclass
class AirflowMap:
    """Actuator inversion: airflow (CFM) <-> (damper, fan) at a given pit temp.

    Actuator coordinate u in [0, 2]: u in [0,1] = damper fraction with fan stopped,
    u in [1,2] = damper open, fan speed u-1. Built from the firmware-side model with an
    assumed top-vent area; rebuilt when the ambient reading changes appreciably.
    """

    egg: Egg
    intake: Intake
    fan: Fan
    top_area_m2: float = TOP_VENT["slider 1/4"]
    temps_f: np.ndarray = field(default_factory=lambda: np.arange(60.0, 461.0, 10.0))
    us: np.ndarray = field(default_factory=lambda: np.concatenate([np.linspace(0, 1, 21), np.linspace(1.05, 2, 20)]))
    _amb: Ambient | None = None
    _table: np.ndarray | None = None

    def _build(self, amb: Ambient) -> None:
        tab = np.zeros((len(self.temps_f), len(self.us)))
        for i, t in enumerate(self.temps_f):
            for j, u in enumerate(self.us):
                damper, s = (u, 0.0) if u <= 1.0 else (1.0, u - 1.0)
                tab[i, j] = air_flow(self.egg, self.intake, self.fan, amb, s, damper, f_to_k(t), self.top_area_m2) / CFM
        self._table = np.maximum.accumulate(tab, axis=1)  # enforce monotonic for inversion
        self._amb = amb

    def _row(self, amb: Ambient, t_f: float) -> np.ndarray:
        if self._amb is None or abs(self._amb.t_k - amb.t_k) > 2.0 or abs(self._amb.wind_m_s - amb.wind_m_s) > 1.0:
            self._build(amb)
        t = float(np.clip(t_f, self.temps_f[0], self.temps_f[-1]))
        i = int(np.clip(np.searchsorted(self.temps_f, t) - 1, 0, len(self.temps_f) - 2))
        w = (t - self.temps_f[i]) / (self.temps_f[i + 1] - self.temps_f[i])
        return (1 - w) * self._table[i] + w * self._table[i + 1]

    def limits(self, amb: Ambient, t_f: float) -> tuple[float, float]:
        row = self._row(amb, t_f)
        return float(row[0]), float(row[-1])

    def actuate(self, amb: Ambient, t_f: float, q_cfm: float) -> tuple[float, float]:
        row = self._row(amb, t_f)
        u = float(np.interp(q_cfm, row, self.us))
        return (u, 0.0) if u <= 1.0 else (1.0, u - 1.0)


@dataclass
class FeedForward:
    """Steady-state airflow to hold a temperature, from the firmware-side model + ambient sensor."""

    egg: Egg
    _cache: dict = field(default_factory=dict)

    def q_cfm(self, amb: Ambient, t_f: float) -> float:
        key = (round(amb.t_k, 0), round(amb.wind_m_s, 0), round(t_f, 0))
        if key not in self._cache:
            if t_f <= k_to_f(amb.t_k) + 5.0:
                self._cache[key] = 0.0
            else:
                ss = thermal.steady_state(self.egg, amb, f_to_k(t_f))
                self._cache[key] = ss.m_air_kg_s / air_density(amb.t_k) / CFM
        return self._cache[key]


@dataclass
class RefShaper:
    """Shape the reference only while *approaching* a setpoint (light-off or a setpoint increase).

    While armed: approach along a first-order trajectory (rate-capped), never more than
    `tether` above the pit temp, so a slow-to-spread fire cannot wind the controller up.
    Disarms once the pit is within `band` of the setpoint; after that disturbances (lid,
    weather) see the true setpoint so recovery is not slowed down. Falling setpoints apply
    immediately - the only way down is less air.
    """

    tau_s: float = 360.0
    rate_max_f_s: float = 12.0 / 60.0
    tether_f: float = 40.0
    band_f: float = 5.0
    r: float | None = None
    armed: bool = True
    _sp: float | None = None

    def step(self, sp: float, y: float, dt: float) -> float:
        if self._sp is not None and sp > self._sp + self.band_f:
            self.armed, self.r = True, min(y, sp)
        self._sp = sp
        if self.r is None:
            self.r = min(y, sp)
        if not self.armed or sp <= self.r:
            self.r = sp
        else:
            self.r += dt * min((sp - self.r) / self.tau_s, self.rate_max_f_s)
            self.r = min(self.r, y + self.tether_f, sp)
        if self.armed and abs(y - sp) < self.band_f:
            self.armed, self.r = False, sp
        return self.r

    def preview(self, sp: float, n: int, dt: float) -> np.ndarray:
        r, out = self.r if self.r is not None else sp, np.empty(n)
        for k in range(n):
            if not self.armed or sp <= r:
                r = sp
            else:
                r = min(r + dt * min((sp - r) / self.tau_s, self.rate_max_f_s), sp)
            out[k] = r
        return out


# ---------------------------------------------------------------- identification ("autotune")


@dataclass(frozen=True)
class Fopdt:
    k_f_per_cfm: float
    tau_s: float
    theta_s: float
    setpoint_f: float


class _OpenLoop:
    def __init__(self, amap: AirflowMap, q_cfm: float, step_at_s: float, step_cfm: float):
        self.amap, self.q, self.t_step, self.dq = amap, q_cfm, step_at_s, step_cfm

    def step(self, obs: Obs, dt: float):
        q = self.q + (self.dq if obs.t_s >= self.t_step else 0.0)
        return self.amap.actuate(obs.ambient, obs.pit_f, q)


def identify(
    plant: Egg,
    intake: Intake,
    fan: Fan,
    amb: Ambient,
    top_area_m2: float,
    amap: AirflowMap,
    setpoint_f: float,
    step_frac: float = 0.3,
    duration_s: float = 2400.0,
    noise_f: float = 0.3,
) -> Fopdt:
    """Simulated autotune: from steady state at the setpoint, step the airflow and fit an FOPDT."""
    ss = thermal.steady_state(plant, amb, f_to_k(setpoint_f))
    q0 = ss.m_air_kg_s / air_density(amb.t_k) / CFM
    dq = step_frac * q0
    t0 = 1200.0  # let the open-loop actuation settle; residual drift is fitted below
    r = simulate(
        plant, intake, fan, amb, top_area_m2, [], t0 + duration_s, _OpenLoop(amap, q0, t0, dq),
        setpoint_f=setpoint_f, init_f=setpoint_f, noise_f=noise_f,
    )
    win = r.t_s >= t0 - 600.0
    t, y = r.t_s[win] - t0, r.pit_f[win]

    def model(p):
        k, tau, theta, y0, drift = p
        step = np.where(t > theta, k * dq * (1 - np.exp(-(t - theta) / tau)), 0.0)
        return y0 + drift * t + step

    fit = least_squares(
        lambda p: model(p) - y,
        x0=[100.0, 600.0, 60.0, y[0], 0.0],
        bounds=([1.0, 30.0, 0.0, -1e3, -1.0], [1e4, 2e4, 900.0, 1e3, 1.0]),
    )
    k, tau, theta = fit.x[:3]
    return Fopdt(float(k), float(tau), float(theta), setpoint_f)


@dataclass
class Schedule:
    models: list[Fopdt]

    def at(self, sp_f: float) -> Fopdt:
        ms = sorted(self.models, key=lambda m: m.setpoint_f)
        sps = [m.setpoint_f for m in ms]

        def interp(attr):
            return float(np.interp(sp_f, sps, [getattr(m, attr) for m in ms]))

        return Fopdt(interp("k_f_per_cfm"), interp("tau_s"), interp("theta_s"), sp_f)


# ---------------------------------------------------------------- controllers


@dataclass
class LidRecovery:
    """After a lid event, replay the pre-lid airflow until the pit is back near setpoint.

    The ceramic is still hot, so the pre-lid airflow is the right steady command; a
    feedback controller seeing a 100F+ error would otherwise either flare the fire
    (overshoot) or, with a model that ignores the hot walls, recover far too slowly.
    """

    band_f: float = 20.0
    max_s: float = 900.0
    # Multiple of the pre-lid airflow to replay: recovery speed vs post-lid overshoot.
    # Sweep (FfPi/Mpc, 3 scenarios x 3 plants): 1.0 -> ~12 min / ~1-5F, 1.5 -> ~6 min / ~5-9F,
    # 2.0 -> ~5 min / ~12-16F, 3.0 -> ~4 min / ~25-30F. A candidate user setting.
    boost: float = 1.5
    q_hold: float = 0.0
    active_s: float = -1.0
    _was_lid: bool = False

    def update(self, obs: Obs, dt: float, q_last: float) -> float | None:
        """Returns the airflow to command while recovering, else None (normal control)."""
        if obs.lid:
            if not self._was_lid:
                self.q_hold = q_last
            self._was_lid = True
            return 0.0
        if self._was_lid:
            self._was_lid, self.active_s = False, 0.0
        if self.active_s >= 0.0:
            self.active_s += dt
            if abs(obs.setpoint_f - obs.pit_f) < self.band_f or self.active_s > self.max_s:
                self.active_s = -1.0
                return None
            return self.q_hold * self.boost
        return None


@dataclass
class PercentPid:
    """Baseline: PI on 0..1 output in %/F units, damper-first mapping, output 0 while the lid is open."""

    kp_per_f: float = 0.02
    ki_per_f_s: float = 0.00005
    integ: float = 0.0

    def step(self, obs: Obs, dt: float) -> tuple[float, float]:
        if obs.lid:
            return 0.0, 0.0
        err = obs.setpoint_f - obs.pit_f
        self.integ = min(max(self.integ + self.ki_per_f_s * err * dt, 0.0), 1.0)
        return output_map(min(max(self.kp_per_f * err + self.integ, 0.0), 1.0))


@dataclass
class FfPi:
    amap: AirflowMap
    ff: FeedForward
    schedule: Schedule
    tc_over_theta: float = 2.0  # SIMC closed-loop time constant as a multiple of the dead time
    shaper: RefShaper = field(default_factory=RefShaper)
    lid: LidRecovery = field(default_factory=LidRecovery)
    integ: float = 0.0  # in CFM
    q_last: float = 0.0

    def step(self, obs: Obs, dt: float) -> tuple[float, float]:
        q_rec = self.lid.update(obs, dt, self.q_last)
        if q_rec is not None:
            return self.amap.actuate(obs.ambient, obs.pit_f, q_rec) if q_rec > 0 else (0.0, 0.0)
        r = self.shaper.step(obs.setpoint_f, obs.pit_f, dt)
        m = self.schedule.at(obs.setpoint_f)
        tc = max(self.tc_over_theta * m.theta_s, 60.0)
        kc = m.tau_s / (m.k_f_per_cfm * (tc + m.theta_s))  # CFM per F
        ti = min(m.tau_s, 4.0 * (tc + m.theta_s))
        e = r - obs.pit_f
        q_ff = self.ff.q_cfm(obs.ambient, r)
        q_min, q_max = self.amap.limits(obs.ambient, obs.pit_f)
        q_unsat = q_ff + kc * e + self.integ
        q = min(max(q_unsat, q_min), q_max)
        # conditional integration: stop integrating further into saturation
        if q == q_unsat or (q_unsat > q_max and e < 0) or (q_unsat < q_min and e > 0):
            self.integ += kc * e * dt / ti
        self.q_last = q
        return self.amap.actuate(obs.ambient, obs.pit_f, q)


@dataclass
class Mpc:
    amap: AirflowMap
    ff: FeedForward
    schedule: Schedule
    horizon_s: float = 1800.0
    blocks: int = 6
    overshoot_weight: float = 8.0
    move_weight: float = 30.0  # F^2 per CFM^2 of move
    q_proc_y: float = 0.5
    q_proc_d: float = 0.3
    r_meas: float = 0.3**2
    shaper: RefShaper = field(default_factory=RefShaper)
    lid: LidRecovery = field(default_factory=LidRecovery)
    _x: np.ndarray | None = None  # [y, d]
    _p: np.ndarray | None = None
    _past: list = field(default_factory=list)
    _q_prev: float = 0.0

    def step(self, obs: Obs, dt: float) -> tuple[float, float]:
        m = self.schedule.at(obs.setpoint_f)
        a = math.exp(-dt / m.tau_s)
        kk = m.k_f_per_cfm
        delay = int(round(m.theta_s / dt))
        y_op, q_op = obs.setpoint_f, self.ff.q_cfm(obs.ambient, obs.setpoint_f)

        while len(self._past) < delay + 1:
            self._past.insert(0, self._q_prev)
        if self._x is None:
            self._x = np.array([obs.pit_f, 0.0])
            self._p = np.diag([1.0, 25.0])
        q_rec = self.lid.update(obs, dt, self._q_prev)
        if q_rec is not None:
            # The lid transient is not in the model: track the measurement, keep the learned
            # disturbance, and replay the pre-lid airflow until back near setpoint.
            self._x = np.array([obs.pit_f, self._x[1]])
            self._past.append(q_rec)
            self._past = self._past[-(delay + 1):]
            self._q_prev = q_rec
            return self.amap.actuate(obs.ambient, obs.pit_f, q_rec) if q_rec > 0 else (0.0, 0.0)

        # Kalman: y+ = a y + (1-a)(y_op + K(q_d - q_op) + d), d random walk
        f = np.array([[a, 1 - a], [0.0, 1.0]])
        q_delayed = self._past[-(delay + 1)]
        bu = np.array([(1 - a) * (y_op + kk * (q_delayed - q_op)), 0.0])
        self._x = f @ self._x + bu
        self._p = f @ self._p @ f.T + np.diag([self.q_proc_y, self.q_proc_d])
        h = np.array([1.0, 0.0])
        s = h @ self._p @ h + self.r_meas
        kgain = self._p @ h / s
        self._x = self._x + kgain * (obs.pit_f - h @ self._x)
        self._p = (np.eye(2) - np.outer(kgain, h)) @ self._p

        # roll the model through the dead time with already-committed inputs
        y, d = self._x
        committed = self._past[-delay:] if delay > 0 else []
        for qd in committed:
            y = a * y + (1 - a) * (y_op + kk * (qd - q_op) + d)

        n = max(int(self.horizon_s / dt), self.blocks)
        blk = np.minimum(np.arange(n) * self.blocks // n, self.blocks - 1)
        bmat = np.zeros((n, self.blocks))
        bmat[np.arange(n), blk] = 1.0
        # y_j = a^j y + sum_{l<j} a^(j-1-l) (1-a)(y_op - K q_op + d + K q_l)
        powers = a ** np.arange(1, n + 1)
        free = powers * y + (1 - powers) * (y_op - kk * q_op + d)
        lower = np.tril(a ** np.subtract.outer(np.arange(n), np.arange(n)).clip(min=0))
        gmat = (1 - a) * kk * lower @ bmat
        self.shaper.step(obs.setpoint_f, obs.pit_f, dt)
        ref = self.shaper.preview(obs.setpoint_f, n + delay, dt)[delay:]

        q_min, q_max = self.amap.limits(obs.ambient, obs.pit_f)
        dmat = np.eye(self.blocks) - np.eye(self.blocks, k=-1)

        def cost(qv):
            e = free + gmat @ qv - ref
            w = np.where(e > 0, self.overshoot_weight, 1.0)
            dq = dmat @ qv
            dq[0] -= self._q_prev
            j = np.sum(w * e * e) / n + self.move_weight * np.sum(dq * dq)
            grad = 2 * gmat.T @ (w * e) / n + 2 * self.move_weight * dmat.T @ dq
            return j, grad

        x0 = np.full(self.blocks, min(max(self._q_prev, q_min), q_max))
        res = minimize(cost, x0, jac=True, method="L-BFGS-B", bounds=[(q_min, q_max)] * self.blocks)
        q = float(res.x[0])
        self._q_prev = q
        self._past.append(q)
        self._past = self._past[-(delay + 1):]
        return self.amap.actuate(obs.ambient, obs.pit_f, q)
