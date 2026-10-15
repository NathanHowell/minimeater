"""TI's TLV62569 transient model (`ti_models.py`) on its own, against the datasheet and at the board's operating points.

Two kinds of run, all 5 V in, L1 2.2 uH, output capacitor and FB divider as listed:
- the datasheet's own conditions (SLVSDG1C Table 3: 1.8 V, C2 10 uF, no C3): Figure 15 (PSM at 0.1 A) and Figure 18 (a 0.8 A
  to 2 A step at 1 A/us), to see how far the model is from the part's measured waveforms;
- the board (3V3 from R3 453k / R4 100k, C7 6.8 pF, the derated output capacitors): PSM ripple against load, the TX burst's
  drop below the idle valley, start-up and efficiency.
The model's losses are far too small, so its efficiency is reported against Figure 9 only to show that (`dissipation.py` keeps
the datasheet's curve); what it is good for is the ripple, the dips and the start-up.
"""

from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import numpy as np

from . import ngspice, ti_models
from .dissipation import ETA, ETA_LOAD, L1_DCR
from .power import ESR
from .rails import C_OUT_3V3, TI_STEP, V3_NOM

R_B = 100e3  # R4 / the datasheet's R2
LOADS = (0.03, 0.1, 0.35, 1.0)  # A: ripple and efficiency points


def _deck(lib: Path, vout: float, cout: float, load: str, t_end: float, c3: float | None, name: str) -> str:
    ff = f"C3 out fb {c3}\n" if c3 else ""
    return f"""* {name}
.include {lib}
Vin vin 0 PWL(0 0 50u 5)
Ven en 0 PWL(0 0 100u 0 101u 5)
X1 en fb sw vin 0 {ti_models.SUBCKT}
L1 sw lo 2.2u
Rd lo out {L1_DCR}
Co out co {cout}
Rc co 0 {ESR}
Rt out fb {R_B * (vout / 0.6 - 1)}
Rb fb 0 {R_B}
{ff}Cin vin 0 10u
{load}
.options reltol=1e-3 method=gear
.control
tran {TI_STEP} {t_end} 0 {TI_STEP}
let iin = -i(vin)
wrdata v.dat v(out) iin
.endc
.end
"""


def _run(vout, cout, load, t_end, c3=None):
    lib = ti_models.tlv62569()
    o = ngspice.run(_deck(lib, vout, cout, load, t_end, c3, "buck"), ["v"], ps_compat=True, timeout=1800)["v"]
    return o[:, 0], o[:, 1], o[:, 3]


def _span(t, v, a, b):
    return v[(t > a) & (t < b)]


def _step_load(i0: float, i1: float, t0: float, width: float, slew: float = 1e6) -> str:
    """A sink that ramps up to i0 after start-up, steps to i1 at t0 (A/s slew) and back after `width`, with a light load on
    the output so that it starts."""
    dt = (i1 - i0) / slew
    return (f"Il out 0 PWL(0 0 {0.4e-3} 0 {0.5e-3} {i0} {t0} {i0} {t0 + dt} {i1} {t0 + width} {i1} {t0 + width + dt} {i0})\n"
            "Rl0 out 0 20k")


def fig15() -> dict[str, float]:
    """Datasheet Figure 15 conditions: 0.1 A, 1.8 V, 10 uF. The figure's output ripple is about 0.1 V pk-pk."""
    t, v, _ = _run(1.8, 10e-6, "Rl out 0 18", 1.5e-3)
    w = _span(t, v, 1.2e-3, 1.5e-3)
    return {"pp": float(np.ptp(w)), "mean": float(w.mean())}


def fig18() -> dict[str, float]:
    """Datasheet Figure 18 conditions: 0.8 A to 2 A at 1 A/us, 1.8 V, 10 uF, no C3. The figure shows about 0.2 V of dip and
    0.2 V of overshoot on release, back within a few microseconds."""
    ts, width = 1.1e-3, 50e-6
    t, v, _ = _run(1.8, 10e-6, _step_load(0.8, 2.0, ts, width), 1.25e-3)
    pre = _span(t, v, ts - 40e-6, ts).mean()
    seg = (t > ts) & (t < ts + 30e-6)
    k = int(np.argmin(np.where(seg, v, 9)))
    back = t[(t > t[k]) & (np.abs(v - pre) < 0.02)]
    rel = (t > ts + width) & (t < ts + width + 30e-6)
    return {"pre": float(pre), "dip": float(pre - v[seg].min()), "t_dip": float(t[k] - ts),
            "t_back": float(back[0] - ts) if len(back) else float("nan"), "overshoot": float(v[rel].max() - pre)}


def board_point(i: float) -> dict[str, float]:
    """PSM or PWM at the board's 3V3 with a steady load of `i` A: ripple, level, input power over output power."""
    r = V3_NOM / i
    t, v, iin = _run(V3_NOM, C_OUT_3V3, f"Rl out 0 {r}", 2.4e-3, c3=6.8e-12)
    m = (t > t[-1] - 300e-6)
    tt, vv, ii = t[m], v[m], iin[m]
    dur = tt[-1] - tt[0]
    p_in = 5.0 * float(np.trapezoid(ii, tt)) / dur
    p_out = float(np.trapezoid(vv**2 / r, tt)) / dur
    return {"i": i, "pp": float(np.ptp(vv)), "mean": float(vv.mean()), "valley": float(vv.min()), "eta": p_out / p_in,
            "eta_fig9": float(np.interp(np.log10(i), np.log10(ETA_LOAD), ETA))}


def board_tx(i0: float = 0.080, i1: float = 0.352) -> dict[str, float]:
    """The TX burst (80 to 352 mA, 1 us edges) and its release at the board's 3V3: the lowest the output goes against the idle
    valley before the step (PSM lets the output ride above its set point, so the valley is what the window's low end
    describes), the same on the release, and the start-up time and overshoot with 80 mA on it."""
    ts, width = 1.1e-3, 200e-6
    t, v, _ = _run(V3_NOM, C_OUT_3V3, _step_load(i0, i1, ts, width, slew=(i1 - i0) / 1e-6), 1.5e-3, c3=6.8e-12)
    pre = _span(t, v, ts - 100e-6, ts)
    burst = (t > ts) & (t < ts + width)
    rel = (t > ts + width) & (t < ts + width + 100e-6)
    up = np.where(v > 0.9 * V3_NOM)[0]
    return {"valley_pre": float(pre.min()), "mean_pre": float(pre.mean()), "pp_pre": float(np.ptp(pre)),
            "min_burst": float(v[burst].min()), "drop": float(pre.min() - v[burst].min()),
            "max_release": float(v[rel].max()), "overshoot": float(v[rel].max() - pre.mean()),
            "t_up": float(t[up[0]] - 101e-6) if len(up) else float("nan"), "start_max": float(v[t < ts - 150e-6].max())}


def checks() -> dict | None:
    """Every run, in parallel (each takes 20-60 s); None when the model is not available."""
    if ti_models.tlv62569() is None:
        return None
    with ThreadPoolExecutor(max_workers=10) as ex:
        f15, f18, tx = ex.submit(fig15), ex.submit(fig18), ex.submit(board_tx)
        pts = [ex.submit(board_point, i) for i in LOADS]
        return {"fig15": f15.result(), "fig18": f18.result(), "tx": tx.result(), "points": [p.result() for p in pts]}
