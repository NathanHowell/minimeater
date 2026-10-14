"""Probe channel error budget (`.3.12`): every board-side error source at its datasheet limit, combined worst case
and by Monte Carlo, over 32-600 F for each probe family.

The channel as on the schematic (bias at the tip, ADR 0002's 2026-10-12 amendment): VREF_PROBE --- R23 47k --- tip
(TPD4E05U06 to GND) --- R19 1k --- AINx (C29 100 nF, ADS122C04 input). REFP is the same VREF_PROBE node, so the
rail's value and the reference input current cancel. The firmware reads x = code / full scale, subtracts the
shorted-input offset it measures every pass (`probes.rs`), takes R = 47k x / (1 - x) and Steinhart-Hart.

The DC solution is linear, so it is solved in closed form here and checked against ngspice (`dc_ratio_spice`); the
ADC's sampling kickback is a transient (`kickback`).
"""

import itertools
from dataclasses import dataclass, fields

import numpy as np

from ..model.probes import PROBES, Thermistor
from ..model.units import f_to_k, k_to_f
from . import ngspice

R_BIAS = 47e3
R_SERIES = 1e3
C_NODE = 100e-9
VCC = 3.3
F_MOD = 256e3  # ADS122C04 modulator, normal mode [spec]
T_BOARD = 85.0  # C, the hottest the board may run (ICD); drift terms are from 25 C
DT = T_BOARD - 25.0


@dataclass(frozen=True)
class Bounds:
    """Each source's limit (the half-width of its range unless noted), with where it comes from."""
    bias_tol: float = 1e-3  # TNPW0603 0.1 % [spec]
    bias_tc: float = 25e-6 * DT  # 25 ppm/K over 25-85 C [spec]
    esd_leak: float = 10e-9  # TPD4E05U06, max over temperature, I/O to GND only (0..10 nA) [spec; typ 10 pA]
    adc_in: float = 5e-9 + 10e-12 * DT  # ADS122C04 absolute input current, PGA bypassed, +-5 nA typ + 10 pA/C [spec typ; no max]
    series_tol: float = 1e-2  # R19 1 % (matters only with the input current)
    gain: float = 5e-4 + 2e-6 * DT  # gain error 0.05 % max (gain 1-32) + 2 ppm/C max [spec]
    offset: float = 10e-6  # after the firmware's shorted-input offset subtraction [est; uncalibrated max 150 uV]
    inl: float = 15e-6 * 2 * VCC  # 15 ppm of FSR (+-VREF) max [spec]
    noise: float = 3 * 3.9e-6 / 2  # 3 sigma of 3.9 uVrms per conversion, interquartile mean of 8 counted as 4 [est]


B = Bounds()
TERMS = [f.name for f in fields(Bounds)]


def reading_f(probe: Thermistor, t_f: float, d: dict[str, float]) -> float:
    """What the firmware reports (F) for a probe at t_f with the error sources at the values in `d` (signed;
    missing keys are 0). Self-heating is added separately (`self_heating_f`)."""
    r_t = probe.r(f_to_k(t_f))
    r_b = R_BIAS * (1 + d.get("bias_tol", 0.0) + d.get("bias_tc", 0.0))
    i_esd = d.get("esd_leak", 0.0)
    i_adc = d.get("adc_in", 0.0)
    v_tip = (VCC / r_b - i_esd - i_adc) / (1 / r_b + 1 / r_t)
    v_ain = v_tip - i_adc * R_SERIES * (1 + d.get("series_tol", 0.0))
    x = v_ain / VCC * (1 + d.get("gain", 0.0)) + (d.get("offset", 0.0) + d.get("inl", 0.0) + d.get("noise", 0.0)) / VCC
    return k_to_f(probe.t_k(R_BIAS * x / (1 - x)))


def self_heating_f(probe: Thermistor, t_f: float) -> float:
    """The probe's own dissipation over its still-air dissipation constant (reads high; much less in meat)."""
    r_t = probe.r(f_to_k(t_f))
    v = VCC * r_t / (r_t + R_BIAS)
    return v * v / r_t / probe.dissipation_w_k * 1.8


def _ranges(b: Bounds) -> dict[str, tuple[float, float]]:
    r = {n: (-getattr(b, n), getattr(b, n)) for n in TERMS}
    r["esd_leak"] = (0.0, b.esd_leak)  # it only sinks
    return r


def terms_f(probe: Thermistor, t_f: float, b: Bounds = B) -> dict[str, float]:
    """Each source alone at whichever end of its range is worse (F, magnitude), plus self-heating."""
    base = reading_f(probe, t_f, {})
    out = {}
    for n, (lo, hi) in _ranges(b).items():
        out[n] = max(abs(reading_f(probe, t_f, {n: v}) - base) for v in (lo, hi))
    out["self_heating"] = self_heating_f(probe, t_f)
    return out


def worst_f(probe: Thermistor, t_f: float, b: Bounds = B) -> float:
    """Worst error (F, magnitude) over every corner of the sources' ranges, self-heating included."""
    rng = _ranges(b)
    names = [n for n in TERMS if n not in ("series_tol",)]  # R19's tolerance is second order; held at its worse end below
    sh = self_heating_f(probe, t_f)
    worst = 0.0
    for signs in itertools.product((0, 1), repeat=len(names)):
        d = {n: rng[n][s] for n, s in zip(names, signs)}
        d["series_tol"] = b.series_tol if d["adc_in"] > 0 else -b.series_tol
        e = reading_f(probe, t_f, d) - t_f + sh
        worst = max(worst, abs(e))
    return worst


def monte_carlo_f(probe: Thermistor, t_f: float, n: int = 4000, b: Bounds = B, seed: int = 1) -> np.ndarray:
    """Errors (F) with each source drawn independently: the resistor tolerances and the ADC's gain, offset and input
    current normal with the limit at 3 sigma, the ESD leakage and the INL uniform over their ranges, the noise
    normal at its 1 sigma."""
    g = np.random.default_rng(seed)
    rng = _ranges(b)
    sh = self_heating_f(probe, t_f)
    out = np.empty(n)
    for k in range(n):
        d = {}
        for name, (lo, hi) in rng.items():
            if name in ("esd_leak", "inl"):
                d[name] = g.uniform(lo, hi)
            else:
                d[name] = float(np.clip(g.normal(0.0, hi / 3), lo, hi))
        out[k] = reading_f(probe, t_f, d) - t_f + sh
    return out


def dc_ratio_spice(probe: Thermistor, t_f: float, i_esd: float, i_adc: float) -> float:
    """The same channel in ngspice (with FB2/R18/C27/C28 from `frontend`): x = v(ain) / v(vref)."""
    from .frontend import RAIL

    r_t = probe.r(f_to_k(t_f))
    net = f"""* channel dc
Vrail rail 0 DC {VCC}
{RAIL}R23 vref tip {R_BIAS}
Rt tip 0 {r_t}
Iesd tip 0 DC {i_esd}
R19 tip ain {R_SERIES}
C29 ain 0 {C_NODE}
Iadc ain 0 DC {i_adc}
.control
op
let x = v(ain)/v(vref)
wrdata x.dat x
.endc
.end
"""
    return float(ngspice.run(net, ["x"])["x"].flat[-1])


def dc_ratio(probe: Thermistor, t_f: float, i_esd: float, i_adc: float) -> float:
    r_t = probe.r(f_to_k(t_f))
    v_tip = (VCC / R_BIAS - i_esd - i_adc) / (1 / R_BIAS + 1 / r_t)
    return (v_tip - i_adc * R_SERIES) / VCC


def kickback(probe: Thermistor, t_f: float, c_s: float, t_conv: float = 50e-3) -> dict[str, float]:
    """The ADC's input as a switched capacitor: c_s connects to AINx for half of each modulator cycle and is
    discharged to AVSS for the other half (the worst case: every cycle takes c_s * v(ain) from the node). The jack's
    node starts settled, as it is between conversions; the conversion is the mean of v(ain) over t_conv. Returns
    the reading's error (F) against the open circuit, the error a DC current of the same average would give, and
    the per-cycle dip (V)."""
    r_t = probe.r(f_to_k(t_f))
    x0 = dc_ratio(probe, t_f, 0.0, 0.0)
    per = 1 / F_MOD
    net = f"""* kickback
Vref vref 0 DC {VCC}
R23 vref tip {R_BIAS}
Rt tip 0 {r_t}
R19 tip ain {R_SERIES}
C29 ain 0 {C_NODE}
Vp1 p1 0 PULSE(0 1 0 5n 5n {per / 2 - 20e-9} {per})
Vp2 p2 0 PULSE(0 1 {per / 2} 5n 5n {per / 2 - 20e-9} {per})
S1 ain cs p1 0 SW
S2 cs 0 p2 0 SW
Cs cs 0 {c_s}
.model SW SW(VT=0.5 VH=0.1 RON=200 ROFF=1e13)
.ic v(ain)={x0 * VCC} v(tip)={x0 * VCC + 0.0}
.control
tran {per / 8} {t_conv} 0 {per / 8} uic
linearize v(ain)
let x = v(ain)/{VCC}
wrdata x.dat x
.endc
.end
"""
    d = ngspice.run(net, ["x"])["x"]
    t, x = d[:, 0], d[:, 1]
    x_mean = float(np.trapezoid(x, t) / (t[-1] - t[0]))
    i_avg = F_MOD * c_s * x_mean * VCC
    x_dc = dc_ratio(probe, t_f, 0.0, i_avg)

    def to_f(xx):
        return k_to_f(probe.t_k(R_BIAS * xx / (1 - xx)))

    return {"err_f": to_f(x_mean) - to_f(x0), "dc_equiv_f": to_f(x_dc) - to_f(x0), "i_avg": i_avg,
            "dip_v": c_s * x0 * VCC / C_NODE}


# Acceptance (bead .3.12): worst case under 1 F over a 150-300 F pit, under 0.5 F at food 130-205 F, every family.
PIT_F = (150.0, 300.0)
FOOD_F = (130.0, 205.0)
GRID_F = (32, 130, 150, 165, 205, 225, 250, 300, 400, 500, 600)


def accept() -> dict[str, float]:
    """The worst case inside each acceptance band, over the families, on a 5 F grid."""
    out = {}
    for name, (lo, hi) in (("pit", PIT_F), ("food", FOOD_F)):
        out[name] = max(worst_f(p, t) for p in PROBES for t in np.arange(lo, hi + 0.1, 5.0))
    return out

