"""`just mech::cfd::whistle <les case dir>...`: what the flap's unsteady force radiates (`.2.19`).

The LES (`les.py`, incompressible) gives the force on the flap and rod. Its axial part is a pressure jump F_x / A_bore across
the flap, the only part that drives the duct's plane waves (the cross forces couple to the bore's first cross mode, above 6
kHz). `acoustics.py`'s chain carries the jump to the two open ends: the volume velocity it drives round the loop, U = dp /
Z_loop, then the share that leaves each end through its radiation resistance. The grille end radiates into the room, as a
point source into the whole sphere at 1 m; the tube's end into the firebox (reported as its sound power, not a level at a
listener). A-weighted and summed over the band, the room level is set against the fan's 19.3 dB(A).

This is the broadband (non-feedback) level: what the shedding radiates when it does not lock onto a mode. Whether it can
(a whistle) comes from a forced run (`les.py --force-hz`): the inflow oscillates at a mode's frequency, which a flap this
small against the wavelength sees as the mode's acoustic velocity, and the part of the flap's axial force in phase with the
oscillating flow is its acoustic resistance (Testud and Auregan's whistling potentiality for orifices). Positive, the flap
absorbs the mode; negative, the flow feeds it, and it whistles if that beats the rest of the loop's resistance (the walls and
the two open ends, `acoustics.loop` less the flap's own).
"""

import argparse
import json
import math
from pathlib import Path

import numpy as np

from . import acoustics as ac

P_REF = 20e-6
W_REF = 1e-12
FAN_DBA = 19.3  # NF-A6x25 at full speed (Noctua's figure)
R_LISTEN = 1.0  # m


def a_weight(f: np.ndarray) -> np.ndarray:
    """IEC 61672 A-weighting, dB."""
    f2 = f * f
    ra = (12194.0**2 * f2 * f2) / ((f2 + 20.6**2) * np.sqrt((f2 + 107.7**2) * (f2 + 737.9**2)) * (f2 + 12194.0**2))
    return 20 * np.log10(np.maximum(ra, 1e-30)) + 2.0


def ends(f: np.ndarray, theta_deg: float, q_m3s: float):
    """Per unit pressure jump at the flap: the volume velocity leaving the grille end and the tube's end (m^3/s/Pa), and
    their radiation resistances (Pa s/m^3)."""
    w = 2 * math.pi * f
    p = ac.path()
    z_up_end = ac.radiation(w, p.r_fan, flanged=False)
    z_dn_end = ac.radiation(w, p.r_tube, flanged=True)
    t_up, t_dn = ac._chain(w, p.up), ac._chain(w, p.down)
    u_loop = 1 / ac.loop(f, theta_deg, q_m3s)
    # [p, U] at the flap = T [p_end, U_end] with p_end = Z_end U_end, so U_end = U / (T10 Z_end + T11)
    u_up = u_loop / (t_up[1, 0] * z_up_end + t_up[1, 1])
    u_dn = u_loop / (t_dn[1, 0] * z_dn_end + t_dn[1, 1])
    return u_up, u_dn, z_up_end.real, z_dn_end.real


def radiated(f: np.ndarray, s_fx: np.ndarray, theta_deg: float, q_m3s: float) -> dict:
    """The levels a one-sided axial-force PSD S_Fx (N^2/Hz) radiates. Returns the room's dB and dB(A) at R_LISTEN from the
    grille end, the firebox's sound power, and the A-weighted room spectrum's peak."""
    keep = (f > 20) & (f < 6000)
    f, s_fx = f[keep], s_fx[keep]
    a_bore = math.pi * (ac.g.bore_r * 1e-3) ** 2
    s_dp = s_fx / a_bore**2  # Pa^2/Hz
    u_up, u_dn, r_up, r_dn = ends(f, theta_deg, q_m3s)
    w_up = r_up * np.abs(u_up) ** 2 * s_dp  # W/Hz
    w_dn = r_dn * np.abs(u_dn) ** 2 * s_dp
    p2 = ac.RHO * ac.C * w_up / (4 * math.pi * R_LISTEN**2)  # Pa^2/Hz at the listener
    df = np.gradient(f)
    p2a = p2 * 10 ** (a_weight(f) / 10)
    db = lambda x: float(10 * math.log10(max(x, 1e-30)))  # noqa: E731
    i = int(np.argmax(p2a))
    return {
        "room_db": db(np.sum(p2 * df) / P_REF**2),
        "room_dba": db(np.sum(p2a * df) / P_REF**2),
        "room_peak_hz": float(f[i]),
        "firebox_swl_db": db(np.sum(w_dn * df) / W_REF),
        "fan_dba": FAN_DBA,
    }


def impedance(path: Path) -> dict:
    """The flap's acoustic impedance at a forced run's frequency (Pa s/m^3): its axial force's Fourier component over the
    pressure jump's area, against the oscillating flow's, over a whole number of periods after `t_stats`."""
    from .les_post import _dat, uniform

    meta = json.loads((path / "case.json").read_text())
    hz, amp, t_stats = meta["force_hz"], meta["force_amp"], meta["t_stats"]
    force = _dat(path, "forces", "force.dat")
    t_end = force[-1, 0]
    n = math.floor((t_end - t_stats) * hz)
    if n < 10:
        raise SystemExit(f"{path}: {n} forcing periods past t_stats: not enough")
    t, F = uniform(force, t_end - n / hz, t_end)
    fx = F[:, 0] * ac.RHO
    e = np.exp(-2j * math.pi * hz * t)
    f_hat = 2 * np.mean(fx * e)  # x(t) = Re{X e^{i w t}}
    q0 = meta["cfm"] * ac.CFM
    q_hat = -1j * q0 * amp  # the inlet's q0 a sin(w t)
    a_bore = math.pi * (ac.g.bore_r * 1e-3) ** 2
    z = (f_hat / a_bore) / q_hat
    z_model = complex(np.asarray(ac.flap_z(np.array([2 * math.pi * hz]), meta["flap_deg"], q0))[0])
    z_rest = complex(ac.loop(np.array([hz]), meta["flap_deg"], q0)[0]) - z_model
    return {"force_hz": hz, "force_amp": amp, "periods": n, "z_flap": [z.real, z.imag], "z_flap_model": [z_model.real, z_model.imag],
            "r_rest": z_rest.real, "margin": (z.real + z_rest.real) / z_rest.real,
            "whistles": bool(z.real + z_rest.real < 0)}


def case(path: Path) -> dict:
    meta = json.loads((path / "case.json").read_text())
    if meta.get("force_hz"):
        return {"case": path.name, "flap_deg": meta["flap_deg"], "cfm": meta["cfm"], **impedance(path)}
    sp = np.load(path / "spectra.npz")
    theta, q = meta["flap_deg"], meta["cfm"] * ac.CFM
    out = {"case": path.name, "flap_deg": theta, "cfm": meta["cfm"], **radiated(sp["f"], sp["S_Fx"], theta, q)}
    out["modes"] = [{"f": round(m.f), "q": round(m.q, 1)} for m in ac.modes(theta, q)[:3]]
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("cases", nargs="+", type=Path)
    a = ap.parse_args()
    for c in a.cases:
        print(json.dumps(case(c)))


if __name__ == "__main__":
    main()
