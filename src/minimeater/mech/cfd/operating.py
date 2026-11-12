"""Where the firmware runs the flap and the fan (`just mech::cfd::operating [dir]`): the actuation path the airflow map
(`model.control.AirflowMap`, ported to `minimeater-core::model::airflow`) takes as the commanded airflow rises, and
the steady hold point (`FeedForward`) for each pit temperature and ambient, with the flap's shedding estimate there
against the duct's modes (`acoustics.py`).

The map is damper first: the flap opens from closed with the fan stopped (natural draft only), then the fan ramps with
the flap fully open; below the fan's minimum speed it cycles between off and the minimum (`FanCycler`), so the flap
then sees two flows in turn, the draft alone and the fan at its minimum. The flap's angle is the firmware's projected-area
law (`actuator::flap_angle_deg`, degrees from closed; acoustics.py counts from open).
"""

import math
import sys
from dataclasses import dataclass
from pathlib import Path

import numpy as np

from ...model.control import AirflowMap, FeedForward
from ...model.control_report import FAN
from ...model.params import DEFAULT_EGG, DEFAULT_INTAKE, Ambient
from ...model.sim import air_flow
from ...model.units import CFM, f_to_k
from . import acoustics as A

PITS_F = (225.0, 275.0, 350.0, 450.0)
AMBIENTS_F = (40.0, 70.0, 95.0)


def angle_from_closed(open_frac: float) -> float:
    return math.degrees(math.acos(1.0 - min(max(open_frac, 0.0), 1.0)))


def ambient(t_f: float) -> Ambient:
    return Ambient(t_k=f_to_k(t_f))


@dataclass(frozen=True)
class Hold:
    pit_f: float
    amb_f: float
    q_cfm: float  # the time-average the controller holds
    flap_deg: float  # from closed
    fan_s: float  # time-average speed fraction
    states: tuple  # (flap angle from open, CFM) the flap actually sees: one, or two when the fan cycles


def hold(amap: AirflowMap, ff: FeedForward, pit_f: float, amb_f: float) -> Hold:
    amb = ambient(amb_f)
    q = ff.q_cfm(amb, pit_f)
    damper, s = amap.actuate(amb, pit_f, q)
    flap = angle_from_closed(damper)
    t = f_to_k(pit_f)
    top = amap.top_area_m2
    if s <= 0.0:
        states = ((90.0 - flap, q),)
    elif s < FAN.s_min:  # cycling: the draft alone, then the fan at its minimum
        off = air_flow(DEFAULT_EGG, DEFAULT_INTAKE, FAN, amb, 0.0, damper, t, top) / CFM
        on = air_flow(DEFAULT_EGG, DEFAULT_INTAKE, FAN, amb, FAN.s_min, damper, t, top) / CFM
        states = ((90.0 - flap, off), (90.0 - flap, on))
    else:
        states = ((90.0 - flap, q),)
    return Hold(pit_f, amb_f, q, flap, s, states)


def path(amap: AirflowMap, pit_f: float, amb_f: float, n: int = 200):
    """(commanded CFM, flap from closed, fan rpm time-average) along the map, from closed to flat out."""
    amb = ambient(amb_f)
    lo, hi = amap.limits(amb, pit_f)
    qs = np.linspace(lo, hi, n)
    out = []
    for q in qs:
        damper, s = amap.actuate(amb, pit_f, q)
        out.append((q, angle_from_closed(damper), s * FAN.rpm_max))
    return np.array(out)


def risk(states) -> list[str]:
    """The flap's shedding estimates within 15 % of a duct mode with Q >= 2, for each state it sees."""
    out = []
    for th, cfm in states:
        if cfm <= 0:
            continue
        q = cfm * A.CFM
        ms = [m for m in A.modes(th, q) if m.q >= 2.0]
        for src, fs in A.shedding(th, q).items():
            if src == "plate" and th < 10:
                continue
            for m in ms:
                if abs(fs - m.f) <= 0.15 * m.f:
                    out.append(f"{src} {fs:.0f} Hz ~ {m.f:.0f} Hz (Q {m.q:.0f}) at {cfm:.2f} CFM")
    return out


def plot(amap: AirflowMap, ff: FeedForward, out: Path):
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    fig, axes = plt.subplots(1, len(AMBIENTS_F), figsize=(5 * len(AMBIENTS_F), 4.5), sharey=True)
    colours = dict(zip(PITS_F, ("tab:blue", "tab:green", "tab:orange", "tab:red")))
    for ax, amb_f in zip(axes, AMBIENTS_F):
        ax2 = ax.twinx()
        for pit in PITS_F:
            p = path(amap, pit, amb_f)
            c = colours[pit]
            ax.plot(p[:, 0], p[:, 1], color=c, lw=1.5, label=f"{pit:.0f} F")
            ax2.plot(p[:, 0], p[:, 2], color=c, lw=1.5, ls="--")
            h = hold(amap, ff, pit, amb_f)
            ax.plot(h.q_cfm, h.flap_deg, "o", color=c, ms=7, mec="k")
            ax2.plot(h.q_cfm, h.fan_s * FAN.rpm_max, "s", color=c, ms=6, mec="k")
        p = path(amap, amb_f, amb_f)  # light-up: the pit at ambient, no draft; the map opens the flap and runs the fan
        ax.plot(p[:, 0], p[:, 1], color="k", lw=1.0, label="light-up")
        ax2.plot(p[:, 0], p[:, 2], color="k", lw=1.0, ls="--")
        ax.set_title(f"ambient {amb_f:.0f} F")
        ax.set_xlabel("commanded airflow, CFM")
        ax.set_ylim(0, 95)
        ax2.set_ylim(0, FAN.rpm_max * 1.05)
        ax.grid(True, alpha=0.3)
        if amb_f == AMBIENTS_F[-1]:
            ax2.set_ylabel("fan rpm, time average (dashed; squares: hold)")
    axes[0].set_ylabel("flap, deg from closed (solid; dots: hold)")
    axes[0].legend(title="pit", loc="center right")
    fig.suptitle(f"{FAN.name}: the airflow map's path (damper first), top vent 1/4")
    fig.tight_layout()
    fig.savefig(out, dpi=110)


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "docs/cfd")
    out.mkdir(parents=True, exist_ok=True)
    amap = AirflowMap(DEFAULT_EGG, DEFAULT_INTAKE, FAN)
    ff = FeedForward(DEFAULT_EGG)
    plot(amap, ff, out / "operating.png")
    print("| pit F | ambient F | hold CFM | flap from closed | fan | flap sees (from open, CFM) | shedding near a mode |")
    print("|---|---|---|---|---|---|---|")
    for amb_f in AMBIENTS_F:
        for pit in PITS_F:
            h = hold(amap, ff, pit, amb_f)
            if h.fan_s <= 0:
                fan = "off"
            elif h.fan_s < FAN.s_min:
                fan = f"cycling {h.fan_s / FAN.s_min:.0%} at {FAN.rpm_min} rpm"
            else:
                fan = f"{h.fan_s * FAN.rpm_max:.0f} rpm"
            sees = ", ".join(f"{th:.0f} deg {c:.2f}" for th, c in h.states)
            r = "; ".join(risk(h.states)) or "none"
            print(f"| {pit:.0f} | {amb_f:.0f} | {h.q_cfm:.2f} | {h.flap_deg:.0f} | {fan} | {sees} | {r} |")
