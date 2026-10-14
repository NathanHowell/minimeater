"""Plane-wave acoustics of the air path, and where the flap's shedding could meet a duct resonance (`.2.17`).

Below the first cross mode of the bore (1.84 c / (pi D), 6.3 kHz at 32 mm; 3.5 kHz in the fan's 58 mm frame) only plane
waves propagate, so the path is a chain of transfer matrices: the grille and the cavity behind the fan as an open end,
the fan's annulus, the cone (in 1 mm steps), the bore to the flap, the flap as a lumped element, the bore on, the tube,
and the tube's open end in the firebox. Viscothermal wall losses (Kirchhoff) in every segment; mean-flow convection is
left out (Mach 0.01).

The flap is a dipole source: a fluctuating force F on it is a pressure jump F / A_bore across it, which drives a volume
velocity through the whole loop, U = dp / Z_loop, with Z_loop the impedance upstream + the flap's own + downstream. The
peaks of 1 / |Z_loop| are the modes the flap can excite (it sits where they move air); their width is how much damping
a feedback loop would have to beat. The flap's resistance is the small-signal slope of its quadratic loss, rho K U / A,
so a partly closed flap damps the duct's modes itself.

Shedding: the gap jets past a partly closed flap shed at f = St U_gap / h, for two lengths: the disc's 2 mm edge (St
0.2, the round edge's own wake) and the disc's projected height D sin(theta) (St 0.15, the inclined plate's wake). Both are
estimates for the LES (`.2.18`) to replace.
"""

import math
from dataclasses import dataclass

import numpy as np

from ..geom import g
from ..params import P
from .geometry import stations

C = 343.0  # m/s, 20 C
RHO = 1.204
NU = 1.5e-5
GAMMA = 1.4
PR = 0.71
TUBE_LEN = 0.10  # m, the tube past the shoulder into the firebox (model.params.Intake.tube_len_m)
CC = 0.92  # the gap jets' contraction coefficient: the law's fit at 45 deg (flap_k corrects it at the other CFD angles)
OPEN_BLOCKAGE = 0.16  # the open flap's share of the bore (mech/analysis.flap_blockage_open)


def _k(w: np.ndarray, r: float) -> np.ndarray:
    """Wavenumber with Kirchhoff's wall losses, e^{j(wt - kx)}."""
    a = np.sqrt(w * NU / 2) * (1 + (GAMMA - 1) / math.sqrt(PR)) / (r * C)
    return w / C + a * (1 - 1j)


def _seg(w, r, length):
    """Transfer matrix of a uniform tube: [p1, U1] = T [p2, U2]."""
    k, zc = _k(w, r) * length, RHO * C / (math.pi * r * r)
    return np.array([[np.cos(k), 1j * zc * np.sin(k)], [1j * np.sin(k) / zc, np.cos(k)]])


def _chain(w, segs):
    t = np.broadcast_to(np.eye(2, dtype=complex)[:, :, None], (2, 2, len(w))).copy()
    for r, length in segs:
        m = _seg(w, r, length)
        t = np.einsum("ijn,jkn->ikn", t, m)
    return t


def _load(t, z):
    """Impedance at the input of chain t terminated in z."""
    return (t[0, 0] * z + t[0, 1]) / (t[1, 0] * z + t[1, 1])


def radiation(w, r, flanged):
    """Low-frequency radiation impedance of an open end (Levine-Schwinger; 0.61 r unflanged, 0.85 r flanged)."""
    ka = w / C * r
    zc = RHO * C / (math.pi * r * r)
    return zc * ((ka * ka / (2 if flanged else 4)) + 1j * ka * (0.85 if flanged else 0.61))


@dataclass(frozen=True)
class Path:
    up: list  # (r, length) from the flap back to the fan's inlet face
    down: list  # from the flap to the tube's end
    r_fan: float
    r_tube: float


def path() -> Path:
    s = stations(1.0)
    m = 1e-3
    a_fan = math.pi * (s.r_fan**2 - s.r_hub**2)
    up = [(s.r_bore * m, (g.flap_x - s.x_bore) * m)]
    n = round(s.x_bore - s.x_fan)
    for i in range(n):  # the cone, back toward the fan, in 1 mm steps
        x = s.x_bore - (i + 0.5) * (s.x_bore - s.x_fan) / n
        r = s.r_bore + (s.r_fan - s.r_bore) * (s.x_bore - x) / (s.x_bore - s.x_fan)
        up.append((r * m, (s.x_bore - s.x_fan) / n * m))
    up.append((math.sqrt(a_fan / math.pi) * m, P.fan.depth * m))  # the fan's annulus, as its equivalent round
    down = [(s.r_bore * m, (s.x_tube - g.flap_x) * m), (s.r_tube * m, TUBE_LEN)]
    return Path(up, down, s.r_fan * m, s.r_tube * m)


def open_area_fraction(theta_deg: float) -> float:
    """The bore's share left open at the flap. Open, the rib and the disc's edge block OPEN_BLOCKAGE; turned, the
    disc's projection (an ellipse, (r_disc / r_bore)^2 sin theta) plus its 2 mm edge's (cos theta), with the rib hidden
    behind the disc (it lies along the ellipse's long axis)."""
    t = math.radians(theta_deg)
    r_disc = g.bore_r - P.damper.flap_gap
    edge = P.damper.flap_t * 2 * r_disc / (math.pi * g.bore_r**2)
    blocked = max(OPEN_BLOCKAGE, (r_disc / g.bore_r) ** 2 * math.sin(t) + edge * math.cos(t))
    return max(1 - blocked, 0.002)


CFD_K = {30.0: 2.56, 45.0: 11.16, 60.0: 56.36}  # k_total at 6 CFM, build/cfd/flap-<deg> (docs/cfd.md)


def flap_k(theta_deg: float) -> float:
    """Loss coefficient on the bore's velocity: a sudden expansion from the gap jets' vena contracta (Borda-Carnot),
    times a correction interpolated (log-linear in angle, held flat outside) through the steady CFD's angles."""
    law = (1 / (CC * open_area_fraction(theta_deg)) - 1) ** 2
    angles = sorted(CFD_K)
    ratios = [math.log(CFD_K[a] / ((1 / (CC * open_area_fraction(a)) - 1) ** 2)) for a in angles]
    return law * math.exp(float(np.interp(theta_deg, angles, ratios)))


def flap_z(w, theta_deg: float, q_m3s: float):
    a = math.pi * g.bore_r**2 * 1e-6
    a_open = a * open_area_fraction(theta_deg)
    r_flow = RHO * flap_k(theta_deg) * (q_m3s / a) / a
    # the gap's inertance: the disc's thickness plus an end correction each side of the gaps
    l_eff = P.damper.flap_t * 1e-3 + 2 * 0.4 * math.sqrt(a_open / math.pi)
    return r_flow + 1j * w * RHO * l_eff / a_open


def loop(f: np.ndarray, theta_deg: float, q_m3s: float):
    """Z_loop seen by a pressure jump at the flap (Pa s / m^3), and the share of the driven flow's power radiated at
    each end."""
    w = 2 * math.pi * f
    p = path()
    z_up_end = radiation(w, p.r_fan, flanged=False)  # into the cavity behind the fan and out through the grille
    z_dn_end = radiation(w, p.r_tube, flanged=True)  # into the firebox
    z_up = _load(_chain(w, p.up), z_up_end)
    z_dn = _load(_chain(w, p.down), z_dn_end)
    return z_up + flap_z(w, theta_deg, q_m3s) + z_dn


@dataclass(frozen=True)
class Mode:
    f: float  # Hz
    q: float  # quality factor (f / -3 dB width)
    gain: float  # peak |1 / Z_loop|, m^3 / (s Pa)


def modes(theta_deg: float, q_m3s: float, f_max: float = 6000.0) -> list[Mode]:
    f = np.arange(20.0, f_max, 0.5)
    h = 1 / np.abs(loop(f, theta_deg, q_m3s))
    out = []
    for i in range(1, len(f) - 1):
        if h[i] > h[i - 1] and h[i] >= h[i + 1]:
            half = h[i] / math.sqrt(2)
            lo = i
            while lo > 0 and h[lo] > half:
                lo -= 1
            hi = i
            while hi < len(f) - 1 and h[hi] > half:
                hi += 1
            if lo == 0 or hi == len(f) - 1:
                continue
            out.append(Mode(float(f[i]), float(f[i] / (f[hi] - f[lo])), float(h[i])))
    return out


def gap_speed(theta_deg: float, q_m3s: float) -> float:
    a = math.pi * g.bore_r**2 * 1e-6
    return q_m3s / (CC * a * open_area_fraction(theta_deg))


def shedding(theta_deg: float, q_m3s: float) -> dict[str, float]:
    u = gap_speed(theta_deg, q_m3s)
    d = 2 * g.bore_r * 1e-3
    return {"edge": 0.2 * u / (P.damper.flap_t * 1e-3),
            "plate": 0.15 * u / max(d * math.sin(math.radians(theta_deg)), 1e-3)}


CFM = 0.0283168 / 60


def coincidences(angles=range(10, 85, 5), cfms=(1.0, 2.0, 4.0, 6.0, 8.0), tol=0.15, q_min=2.0):
    """(angle, cfm, source, f_shed, mode f, mode Q) wherever a shedding estimate is within `tol` of a mode with Q >= q_min."""
    hits = []
    for th in angles:
        for cfm in cfms:
            q = cfm * CFM
            ms = [m for m in modes(th, q) if m.q >= q_min]
            for src, fs in shedding(th, q).items():
                for m in ms:
                    if abs(fs - m.f) <= tol * m.f:
                        hits.append((th, cfm, src, fs, m.f, m.q))
    return hits


def plot(out):
    """Modes (dots sized by Q, at 4 CFM) and the two shedding estimates (bands over 1-8 CFM) against flap angle."""
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    angles = np.arange(0, 81, 2.5)
    fig, ax = plt.subplots(figsize=(8, 5))
    for th in angles:
        for m in modes(th, 4 * CFM):
            ax.scatter(th, m.f, s=4 * m.q, c="k", alpha=0.6, lw=0)
    for src, col, a0 in (("edge", "tab:red", 0.0), ("plate", "tab:blue", 10.0)):  # the plate's wake needs the disc turned
        th = angles[angles >= a0]
        lo = [shedding(t, 1 * CFM)[src] for t in th]
        hi = [shedding(t, 8 * CFM)[src] for t in th]
        ax.fill_between(th, lo, hi, color=col, alpha=0.25, label=f"shedding, {src} (1-8 CFM)")
    ax.set_yscale("log")
    ax.set_ylim(50, 6000)
    ax.set_xlabel("flap angle, deg (0 open)")
    ax.set_ylabel("Hz")
    ax.set_title("Duct modes (dot size: Q at 4 CFM) and shedding estimates")
    ax.legend(loc="upper left")
    ax.grid(True, which="both", alpha=0.3)
    fig.tight_layout()
    fig.savefig(out, dpi=110)


def main():
    import sys
    from pathlib import Path as P_

    out = P_(sys.argv[1] if len(sys.argv) > 1 else "docs/cfd")
    out.mkdir(parents=True, exist_ok=True)
    plot(out / "acoustics.png")
    print("modes, flap open, 4 CFM:", [(round(m.f), round(m.q, 1)) for m in modes(0, 4 * CFM)])
    for th, cfm, src, fs, fm, q in coincidences():
        print(f"{th:4.0f} deg {cfm:3.0f} CFM  {src:5s} {fs:6.0f} Hz ~ mode {fm:6.0f} Hz (Q {q:.1f})")
