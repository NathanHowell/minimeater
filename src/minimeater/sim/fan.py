"""Fan connector in ngspice (`.3.13`): the 25 kHz PWM line into the fan and the fan's open-collector tach back to the
ESP32-C6, through the fan's cable, and the fan load switch's dissipation.

Board, as on the schematic: GPIO (FAN_PWM) --- R28 100R --- J6.4 and J6.3 --- R27 1k --- GPIO (FAN_TACH) with R16 10k
to 3V3 and C39 1 nF to GND (R31, a divider leg to GND, is not fitted); the firmware also enables the pin's own pull-up (45k, `board.rs`) and a 1023-cycle PCNT glitch filter
(12.8 us at 80 MHz). VFAN (J6.2) is 5 V through U7 TPS2553 (85 mohm typ, 135 max over temperature, 182.6 C/W in
SOT-23). The fan is a 5 V 4-wire PWM fan (Noctua NF-A6x25 5V PWM, placeholder): its PWM input is pulled up inside
the fan (to 5 V at most, Intel 4-wire spec; the value is not published, so it is swept here) and its tach is an
open collector, two pulses per revolution.

The bead was written for a 12 V low-side MOSFET blower; ADR 0004 replaced that with a 5 V PWM fan behind a load
switch, so there is no PWM MOSFET on the board and the tach needs no 12 V divider.
"""

from dataclasses import dataclass

import numpy as np

from . import ngspice

VDD = 3.3
VFAN = 5.0
VIH = 0.75 * VDD  # ESP32-C6 [spec]
VIL = 0.25 * VDD
PCNT_FILTER_S = 1023 / 80e6
FAN_VIL = 0.8  # Intel 4-wire fan spec: the fan reads below 0.8 V as low
R_GPIO = 17.0  # from IOH 40 mA at VOH 2.64 V and IOL 28 mA at 0.495 V, drive strength 3 [spec]
PWM_HZ = 25e3
TACH_PERIOD = 200e-6  # the real tach is 100 Hz at 3000 rpm; a faster one shows the same edges in a shorter run


@dataclass(frozen=True)
class Fan:
    pwm_pullup: float = 10e3  # inside the fan, to VFAN [est]
    tach_pullup: float | None = None  # inside the fan, to VFAN, if it has one [est: none]
    tach_ron: float = 50.0  # the fan's open-collector transistor [est]
    c_cable: float = 15e-12  # each conductor to the others and ground, 20 cm lead [est]
    c_mutual: float = 8e-12  # PWM to tach conductor, adjacent in the lead [est]
    l_cable: float = 0.15e-6  # each conductor [est]
    c_pin: float = 1e-9  # C39 at the pin, fitted


DIODES = ".model DCLAMP D(IS=1e-14 N=1.05 RS=10)"  # the GPIO pad's ESD diodes [est]


def run(f: Fan = Fan(), t_end: float = 2 * 200e-6) -> dict[str, np.ndarray]:
    per = 1 / PWM_HZ
    tpu = f"Rtpu ftach vfan {f.tach_pullup}\n" if f.tach_pullup else ""
    net = f"""* fan lines
Vdd vdd 0 DC {VDD}
Vfan vfan 0 DC {VFAN}
* PWM: GPIO, R28, the lead, the fan's input
Vg g 0 PULSE(0 {VDD} 3u 2n 2n {per / 2} {per})
Rg g gp {R_GPIO}
R28 gp jp 100
Lp jp fp {f.l_cable}
Cpj jp 0 {f.c_cable / 2}
Cpf fp 0 {f.c_cable / 2 + 10e-12}
Rppu fp vfan {f.pwm_pullup}
* tach: the fan's open collector, the lead, R27, R16, the pin's pull-up, clamps and capacitance
Vt tg 0 PULSE(0 1 {TACH_PERIOD / 4} 1u 1u {TACH_PERIOD / 2} {TACH_PERIOD})
Sot ftach 0 tg 0 SWOC
.model SWOC SW(VT=0.5 VH=0.05 RON={f.tach_ron} ROFF=1e9)
{tpu}Lt ftach jt {f.l_cable}
Ctf ftach 0 {f.c_cable / 2}
Ctj jt 0 {f.c_cable / 2}
R27 jt tach 1k
R16 tach vdd 10k
Rint tach vdd 45k
Cin tach 0 {5e-12 + f.c_pin}
Dhi tach vddc DCLAMP
Vclamp vddc vdd 0
Dlo 0 tach DCLAMP
{DIODES}
* the two conductors side by side
Cm1 fp ftach {f.c_mutual / 2}
Cm2 jp jt {f.c_mutual / 2}
.control
tran 1n {t_end} 0 2n
wrdata w.dat v(tach) v(fp) i(vclamp) v(tg) v(g)
.endc
.end
"""
    d = ngspice.run(net, ["w"])["w"]
    # wrdata repeats the time column before every vector
    return {"t": d[:, 0], "tach": d[:, 1], "fp": d[:, 3], "i_clamp": d[:, 5], "tach_on": d[:, 7] > 0.5, "gpio": d[:, 9]}


def _settled(t: np.ndarray, edges: np.ndarray, hold: float) -> np.ndarray:
    """Samples at least `hold` after the last edge of the given 0/1 signal."""
    last = np.maximum.accumulate(np.where(np.r_[False, edges[1:] != edges[:-1]], t, -np.inf))
    return t - last > hold


def tach_report(f: Fan = Fan()) -> dict[str, float]:
    w = run(f)
    t, v, on = w["t"], w["tach"], w["tach_on"]
    steady = _settled(t, on, 20e-6) & (t > 10e-6)
    lo, hi = v[steady & on], v[steady & ~on]
    # glitches: excursions past the thresholds away from the settled level, from the PWM's edges
    glitch_hi = float(lo.max())  # while the fan pulls low
    glitch_lo = float(hi.min())  # while the pull-ups hold it high
    # rise time 10-90 % on the first release
    rel = np.nonzero((t > TACH_PERIOD / 4 + TACH_PERIOD / 2) & ~on)[0]
    tr, vr = t[rel], v[rel]
    v0, v1 = float(lo.mean()), float(hi.mean())
    t10 = tr[np.argmax(vr > v0 + 0.1 * (v1 - v0))]
    t90 = tr[np.argmax(vr > v0 + 0.9 * (v1 - v0))]
    # how long any glitch stays past a threshold (the PCNT filter drops pulses shorter than 12.8 us)
    bad = (steady & on & (v > VIL)) | (steady & ~on & (v < VIH))
    width = 0.0
    if bad.any():
        idx = np.nonzero(bad)[0]
        runs = np.split(idx, np.nonzero(np.diff(idx) > 1)[0] + 1)
        width = max(float(t[r[-1]] - t[r[0]]) for r in runs)
    return {"v_low": float(lo.max()), "v_high": float(hi.min()), "v_high_max": float(v.max()), "rise": float(t90 - t10),
            "glitch_in_low": glitch_hi, "glitch_in_high": glitch_lo, "glitch_width": width,
            "i_clamp_max": float(w["i_clamp"].max())}


def pwm_report(f: Fan = Fan()) -> dict[str, float]:
    """At the fan's input: the low level (against the fan's 0.8 V), the high level, the overshoot past the rails,
    and the current the fan's pull-up pushes back into the GPIO when it drives high."""
    w = run(f, t_end=3 * 40e-6)
    t, v, g = w["t"], w["fp"], w["gpio"]
    steady_hi = (g > VDD - 0.01) & _settled(t, g > VDD / 2, 2e-6)
    steady_lo = (g < 0.01) & _settled(t, g > VDD / 2, 2e-6) & (t > 5e-6)
    v_hi = float(v[steady_hi].mean())
    return {"v_low": float(v[steady_lo].max()), "v_high": v_hi, "overshoot": float(v.max() - v_hi), "undershoot": float(-v.min()),
            "i_into_gpio": (VFAN - v_hi) / f.pwm_pullup}


R_ON_MAX = 0.135  # TPS2553 DBV, -40..125 C [spec]
THETA_JA = 182.6  # C/W [spec]
I_FAN = 0.19  # NF-A6x25 5V PWM at full speed (ICD)
I_LIMIT = 0.519  # R6 49.9k


def load_switch() -> dict[str, float]:
    """U7's dissipation and temperature rise: the fan's current is DC whatever the duty (the fan switches its own
    motor), so 100 % duty is the worst running case; a shorted or jammed fan holds the switch in its current limit."""
    p_run = I_FAN**2 * R_ON_MAX
    p_short = VFAN * I_LIMIT  # VFAN pulled to ground: all of it in the switch, until thermal shutdown
    return {"p_run": p_run, "rise_run": p_run * THETA_JA, "p_short": p_short, "rise_short": p_short * THETA_JA,
            "drop_run": I_FAN * R_ON_MAX}
