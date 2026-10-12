"""Probe front-end in ngspice: the schematic's per-jack network, one channel, against the Python error model.

Per channel (schematic, sheet probe): J tip --- R19 1k --- AINx node; the node has R23 47k up to VREF_PROBE and
C29 100 nF to AGND, and goes to the ADS122C04 pin. VREF_PROBE is 3V3A through FB2, R18 10 ohm, C27 10 uF, C28 100 nF.
The topology argument moves the bias resistor to the tip side of R19 ("bias_at_tip"), which is what ADR 0002
describes.
"""

import math
from dataclasses import dataclass

import numpy as np

from ..model.probes import PROBES, Thermistor
from ..model.units import f_to_k, k_to_f
from . import ngspice

R_BIAS = 47e3
R_SERIES = 1e3
C_NODE = 100e-9
I_LEAK = 30e-9  # ADR 0002 input-current budget at a hot board, ESD array included
VCC = 3.3

# VREF_PROBE filter as drawn: ferrite bead (600 ohm @ 100 MHz, modelled as dc resistance + L || R), 10 ohm, 10 uF, 100 nF.
RAIL = """\
Rdc rail f1 0.2
Lfb f1 f2 0.8u
Rfb f1 f2 700
R18 f2 vref 10
C27 vref 0 10u
C28 vref 0 100n
"""


def channel(topology: str, r_probe: float) -> str:
    """Netlist body for one channel. `rail` is driven by the caller; `tip` is the jack tip; `ain` the ADC pin."""
    if topology == "as_drawn":  # tip - 1k - node(47k up, 100n, ADC)
        net = f"R19 tip ain {R_SERIES}\nR23 vref ain {R_BIAS}\n"
    elif topology == "bias_at_tip":  # tip(47k up) - 1k - node(100n, ADC)
        net = f"R23 vref tip {R_BIAS}\nR19 tip ain {R_SERIES}\n"
    else:
        raise ValueError(topology)
    return RAIL + net + f"C29 ain 0 {C_NODE}\nRt tip 0 {r_probe}\nIleak ain 0 DC {I_LEAK}\n"


def ratio_to_f(probe: Thermistor, x: float) -> float:
    """What the firmware does: ratio -> resistance with the nominal 47k -> Steinhart-Hart -> degF."""
    r = R_BIAS * x / (1.0 - x)
    return k_to_f(probe.t_k(r))


def dc_error_f(topology: str, probe: Thermistor, t_f: float) -> float:
    """Reading minus truth (degF) at the settled operating point, with the leakage current."""
    r_t = probe.r(f_to_k(t_f))
    net = f"* dc\nVrail rail 0 DC {VCC}\n{channel(topology, r_t)}"
    net += ".control\nop\nlet x = v(ain)/v(vref)\nwrdata x.dat x\n.endc\n.end\n"
    x = float(ngspice.run(net, ["x"])["x"].flat[-1])
    return ratio_to_f(probe, x) - t_f


def settle_f(topology: str, probe: Thermistor, t_f: float, tol_f: float = 0.01) -> tuple[float, float]:
    """Plugging a probe into a jack: the node starts at VREF (open input), then relaxes. Returns (time to stay within
    tol_f of the final reading in s, time constant estimate in s)."""
    r_t = probe.r(f_to_k(t_f))
    net = f"* settle\nVrail rail 0 DC {VCC}\n{channel(topology, r_t)}.ic v(ain)={VCC}\n"
    net += ".control\ntran 20u 400m uic\nlet x = v(ain)/v(vref)\nwrdata x.dat x\n.endc\n.end\n"
    d = ngspice.run(net, ["x"])["x"]
    t, x = d[:, 0], d[:, 1]
    # error against the settled value, in degF
    final = ratio_to_f(probe, x[-1])
    err = np.array([abs(ratio_to_f(probe, xi) - final) if 0 < xi < 1 else 1e9 for xi in x])
    bad = np.nonzero(err > tol_f)[0]
    t_settle = float(t[bad[-1] + 1]) if len(bad) and bad[-1] + 1 < len(t) else (0.0 if not len(bad) else float("inf"))
    r_th = 1.0 / (1.0 / R_BIAS + 1.0 / (r_t + (R_SERIES if topology == "as_drawn" else 0)))
    return t_settle, r_th * C_NODE


def rail_ripple_db(topology: str, freqs: np.ndarray, r_probe: float) -> tuple[np.ndarray, np.ndarray]:
    """AC gain (dB) from a ripple on 3V3A to the reference (vref) and to the ADC pin (ain)."""
    net = f"* ac\nVrail rail 0 DC {VCC} AC 1\n{channel(topology, r_probe)}"
    net += ".control\nac dec 20 100 100meg\nlet a = db(v(vref))\nlet b = db(v(ain))\nwrdata a.dat a\nwrdata b.dat b\n.endc\n.end\n"
    o = ngspice.run(net, ["a", "b"])
    f = o["a"][:, 0]
    return f, np.column_stack([np.interp(freqs, f, o["a"][:, 1]), np.interp(freqs, f, o["b"][:, 1])])
