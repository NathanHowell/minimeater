"""5 V input in ngspice: USB hot-plug (cable L/R ringing into the board's input, its TVS and the U12 eFuse) and the
load-switch inrush of the servo and fan outputs.

Board, as on the schematic (sheet power): J1 -> VBUS_RAW (D1 SMF5V0A TVS, C38 and C41 100 nF) -> U12 TPS259531 eFuse
-> VBUS_F with C1 100 uF 1210, C2 and C4 10 uF 0805, three 100 nF; U4 (buck), U5 (LDO), U6/U7 (load switches) hang on
VBUS_F. Absolute maxima: 20 V at the eFuse's input; 6 V at VBUS_F on the TLV62569 and TLV75533P, 7 V on the TPS2553
(datasheets). The first board had a PTC (F1) where U12 is; `front_end="ptc"` keeps that for comparison.
"""

from dataclasses import dataclass

import numpy as np

from . import ngspice

V_SRC = 5.25  # USB source at its upper limit
R_PTC = 0.05  # F1 MF-NSMF200-2 (2 A hold): 0.02-0.085 ohm [datasheet]; rises when it trips
TVS = ".model DTVS D(IS=1e-12 N=1 BV=6.8 IBV=1m RS=0.11 CJO=80p)"  # SMF5V0A: ~6.8 V breakdown, ~9.2 V at 21 A [datasheet, approx]
# MLCC DC-bias derating at 5 V [est]: 100 uF 1210 X5R ~45%, 10 uF 0805 ~55%, 100 nF ~90%.
CAPS = {"nominal": (100e-6, 10e-6, 10e-6, 100e-9), "derated": (45e-6, 5.5e-6, 5.5e-6, 90e-9)}
ESR = 3e-3

# U12 TPS259531 (SLVSE57C) as a behavioural switch: off until the turn-on delay, then the output follows a ramp at the
# dVdt slew rate, never above the input or the overvoltage clamp, the current never above the limit. The internal
# FET's 34 mohm is the resistance in series.
EFUSE_RON = 0.034
EFUSE_ILIM = 2000 / 1000 + 0.04  # R35 R_ILM 1k: 2.04 A (eq. 4), the old PTC's 2 A
EFUSE_SR = 42000 / 10e3 * 1e3  # C42 C_dVdt 10 nF: 4.2 V/ms (eq. 3), in V/s
EFUSE_DELAY = 0.4e-3  # turn-on delay with C_dVdt, after EN crosses UVLO [est from the 149-289 us of Table 7.6]
EFUSE_CLAMP = 5.7  # output clamp, TPS25953x: 5.2-5.7 V while clamping (VCLAMP max)


@dataclass(frozen=True)
class HotPlug:
    l_cable: float
    r_cable: float
    caps: str = "derated"
    r_ptc: float = R_PTC
    front_end: str = "efuse"  # "efuse" (U12, as built) or "ptc" (F1, the first board)


def hotplug(h: HotPlug) -> dict[str, float]:
    c1, c2, c4, cs = CAPS[h.caps]
    if h.front_end == "ptc":
        fe = f"Rptc b c {h.r_ptc}\n"
    else:  # C38 + C41 at the input, then the switch
        fe = (f"Cin b nin {2 * cs}\nRin nin 0 {ESR}\n"
              f"Bfuse b c I = min(max((min(min(v(b), {EFUSE_CLAMP}), {EFUSE_SR} * max(time - {EFUSE_DELAY}, 0)) - v(c)) / {EFUSE_RON}, 0), {EFUSE_ILIM})\n"
              "Rbleed c 0 1meg\n")
    t_end = "400u" if h.front_end == "ptc" else "3m"
    net = f"""* hot plug
Vusb src 0 PWL(0 0 1u {V_SRC})
Lcab src a {h.l_cable}
Rcab a b {h.r_cable}
Vtv b tvk 0
D1 0 tvk DTVS
{TVS}
{fe}C1 c n1 {c1}
R1 n1 0 {ESR}
C2 c n2 {c2}
R2 n2 0 {ESR}
C4 c n4 {c4}
R4 n4 0 {ESR}
C3 c n3 {cs}
R3 n3 0 {ESR}
.control
tran 20n {t_end}
let ic = -i(vusb)
let itvs = i(vtv)
wrdata v.dat v(c) v(b)
wrdata i.dat ic itvs
.endc
.end
"""
    o = ngspice.run(net, ["v", "i"])
    v, i = o["v"], o["i"]
    t = i[:, 0]
    early = t <= 100e-6  # what the host sees at attach, before any soft start has begun
    return {"vbus_f_peak": float(v[:, 1].max()), "vbus_raw_peak": float(v[:, 3].max()), "i_peak": float(i[:, 1].max()),
            "tvs_peak": float(i[:, 3].max()), "charge": float(np.trapezoid(i[:, 1], t)),
            "charge_100us": float(np.trapezoid(i[early, 1], t[early])),
            "i_peak_after_100us": float(i[~early, 1].max()) if (~early).any() else 0.0}


def inrush(c_out: float, i_lim: float, r_src: float, i_other: float, c_in: float = 130e-6) -> dict[str, float]:
    """A load switch with its current limit turning on into `c_out` (worst case: no slew limiting). The 5 V rail
    is a source behind `r_src` with c_in on it and `i_other` of other load (buck + fan). Returns the rail minimum,
    the time the switch spends in current limit and the source current peak."""
    net = f"""* inrush
Vsrc src 0 DC 5.0
Rsrc src vin {r_src}
Cin vin 0 {c_in}
Iother vin 0 PWL(0 0 1u {i_other})
* limited switch: current = clamp((vin-vout)/0.1, 0, ilim), enabled at 1 ms
Bsw vin vout I = (time>1m) ? min(max((v(vin)-v(vout))/0.1,0),{i_lim}) : 0
Cout vout 0 {c_out}
Rleak vout 0 1meg
.control
tran 2u 12m
wrdata v.dat v(vin) v(vout)
let isrc = -i(Vsrc)
wrdata i.dat isrc
.endc
.end
"""
    o = ngspice.run(net, ["v", "i"])
    t, vin, vout = o["v"][:, 0], o["v"][:, 1], o["v"][:, 3]
    i = o["i"][:, 1]
    on = t > 1e-3
    lim = on & (np.abs(vout - vin) > 0.1 * i_lim * 0.98 + 1e-9) if False else on & (vout < vin - 0.1 * i_lim * 0.98)
    t_lim = float(t[lim][-1] - 1e-3) if lim.any() else 0.0
    return {"vin_min": float(vin[on].min()), "t_limit": t_lim, "i_src_peak": float(i[on].max())}
