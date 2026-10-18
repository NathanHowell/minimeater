"""Candidate fans, approximate P-Q curves and fan-law scaling.

Only free-air flow, shut-off pressure, max speed and max noise are published for
most candidates, so the curve between those two points is an assumption:
  P(Q) = Pmax * s^2 * (1 - (Q / (s*Qmax))^n)
with n = 1 (linear, conservative) for axial fans and n = 2 (flatter) for radial
blowers. Fan laws: Q ~ s, P ~ s^2, power ~ s^3, noise ~ +50*log10(s) dB.
Replace with measured curves from the bake-off rig.
"""

import math
from dataclasses import dataclass

from scipy.optimize import brentq

from .units import CFM, MMH2O


@dataclass(frozen=True)
class Fan:
    name: str
    kind: str  # "axial" | "radial"
    size: str
    qmax_cfm: float
    pmax_mmh2o: float
    rpm_max: float
    dba_max: float
    watts_max: float
    rpm_min: float  # lowest speed the fan will run continuously (20% PWM)
    # Stopped-fan obstruction to natural (convective) draft: loss coefficient at the
    # velocity through the fan's open flow area. Axial: open face between hub and frame;
    # radial: the outlet, and the air must also thread the stalled impeller. [est]
    open_area_m2: float = 20e-4
    stopped_k: float = 1.5
    volts: float = 5.0
    notes: str = ""

    @property
    def n(self) -> float:
        return 1.0 if self.kind == "axial" else 2.0

    @property
    def qmax_m3_s(self) -> float:
        return self.qmax_cfm * CFM

    @property
    def pmax_pa(self) -> float:
        return self.pmax_mmh2o * MMH2O

    @property
    def s_min(self) -> float:
        return self.rpm_min / self.rpm_max

    def pressure(self, q_m3_s: float, s: float) -> float:
        """Static pressure at flow q and speed fraction s (may go negative past free-air flow)."""
        if s <= 0.0:
            return -math.inf if q_m3_s > 0 else 0.0
        x = q_m3_s / (s * self.qmax_m3_s)
        return self.pmax_pa * s * s * (1.0 - x**self.n) if x >= 0 else self.pmax_pa * s * s

    def noise_dba(self, s: float) -> float:
        return self.dba_max + 50.0 * math.log10(max(s, 1e-3))

    def watts(self, s: float) -> float:
        return self.watts_max * s**3


FANS = [
    Fan(
        "Noctua NF-A4x20 5V PWM", "axial", "40x40x20",
        qmax_cfm=5.53, pmax_mmh2o=2.26, rpm_max=5000, dba_max=14.9, watts_max=0.5, rpm_min=1100, open_area_m2=8.8e-4,
        notes="specs: noctua.at; watts [est]",
    ),
    Fan(
        "Noctua NF-A6x25 5V PWM", "axial", "60x60x25",
        qmax_cfm=17.19, pmax_mmh2o=2.18, rpm_max=3000, dba_max=19.3, watts_max=0.935, rpm_min=600, open_area_m2=21.6e-4,
        stopped_k=3.2,
        notes="specs: noctua.at; rpm_min [est]; stopped_k from the stopped-rotor CFD at 1 CFM (4.2 at 0.5: docs/cfd.md)",
    ),
    Fan(
        "Noctua NF-A8 5V PWM", "axial", "80x80x25",
        qmax_cfm=32.67, pmax_mmh2o=2.37, rpm_max=2200, dba_max=17.7, watts_max=0.9, rpm_min=450, open_area_m2=41.6e-4,
        notes="specs: noctua.at / retailers; watts [est]",
    ),
    Fan(
        "Delta BFB0505HA-C (radial)", "radial", "50x50x15",
        qmax_cfm=3.5, pmax_mmh2o=14.3, rpm_max=4500, dba_max=32.5, watts_max=0.9, rpm_min=1500, open_area_m2=2.6e-4, stopped_k=3.0,
        notes="CFM/pressure/dBA: DigiKey listing; rpm, watts, rpm_min [est]",
    ),
    Fan(
        "Delta BFB0505HHA-C (radial)", "radial", "50x50x15",
        qmax_cfm=4.0, pmax_mmh2o=20.1, rpm_max=5900, dba_max=37.5, watts_max=1.3, rpm_min=2000, open_area_m2=2.6e-4, stopped_k=3.0,
        notes="pressure/rpm/dBA/W: distributor listing; CFM [est, unconfirmed]",
    ),
]


@dataclass(frozen=True)
class OperatingPoint:
    fan: Fan
    q_target_m3_s: float
    s: float  # speed fraction needed (nan if infeasible)
    feasible: bool
    below_min: bool  # needs on/off cycling or damper throttling

    @property
    def rpm(self) -> float:
        return self.s * self.fan.rpm_max

    @property
    def dba(self) -> float:
        return self.fan.noise_dba(self.s)

    @property
    def watts(self) -> float:
        return self.fan.watts(self.s)


def required_speed(fan: Fan, q_m3_s: float, p_required_pa: float) -> OperatingPoint:
    """Speed fraction at which the fan delivers q against p_required (stack already subtracted)."""
    if p_required_pa <= 0.0:
        # Natural draft alone pushes this much air: fan off, flow set by the damper.
        return OperatingPoint(fan, q_m3_s, 0.0, True, True)

    def f(s):
        return fan.pressure(q_m3_s, s) - p_required_pa

    if f(1.0) < 0.0:
        return OperatingPoint(fan, q_m3_s, math.nan, False, False)
    lo = q_m3_s / fan.qmax_m3_s + 1e-9  # below this the fan cannot even reach q in free air
    s = brentq(f, lo, 1.0)
    return OperatingPoint(fan, q_m3_s, s, True, s < fan.s_min)


def max_flow(fan: Fan, s: float, p_of_q) -> float:
    """Flow where the fan curve at speed s meets a system curve p_of_q(q) (stack already subtracted)."""
    hi = s * fan.qmax_m3_s
    g = lambda q: fan.pressure(q, s) - p_of_q(q)  # noqa: E731
    if g(0.0) <= 0.0:
        return 0.0
    while g(hi) >= 0.0:  # stack draft can push past the fan's free-air flow
        hi *= 2.0
    return brentq(g, 0.0, hi)
