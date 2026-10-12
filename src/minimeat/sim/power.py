"""5 V input in ngspice: USB hot-plug (cable L/R ringing into the board's capacitors, TVS and PTC) and the load-switch
inrush of the servo and fan outputs.

Board, as on the schematic (sheet power): J1 -> VBUS_RAW (D1 SMF5V0A TVS) -> F1 PTC -> VBUS_F with C1 100 uF 1210,
C2 and C4 10 uF 0805, three 100 nF; U4 (buck), U5 (LDO), U6/U7 (load switches) hang on VBUS_F. Absolute maxima at
VBUS_F: 6 V on TLV62569, TLV75533 and TPS2553 [from the datasheets, to confirm in ICD].
"""

from dataclasses import dataclass

import numpy as np

from . import ngspice

V_SRC = 5.25  # USB source at its upper limit
R_PTC = 0.15  # F1 1.1 A hold, cold [est]; rises when it trips
TVS = ".model DTVS D(IS=1e-12 N=1 BV=6.8 IBV=1m RS=0.11 CJO=80p)"  # SMF5V0A: ~6.8 V breakdown, ~9.2 V at 21 A [datasheet, approx]
# MLCC DC-bias derating at 5 V [est]: 100 uF 1210 X5R ~45%, 10 uF 0805 ~55%, 100 nF ~90%.
CAPS = {"nominal": (100e-6, 10e-6, 10e-6, 100e-9), "derated": (45e-6, 5.5e-6, 5.5e-6, 90e-9)}
ESR = 3e-3


@dataclass(frozen=True)
class HotPlug:
    l_cable: float
    r_cable: float
    caps: str = "derated"
    r_ptc: float = R_PTC


def hotplug(h: HotPlug) -> dict[str, float]:
    c1, c2, c4, cs = CAPS[h.caps]
    net = f"""* hot plug
Vusb src 0 PWL(0 0 1u {V_SRC})
Lcab src a {h.l_cable}
Rcab a b {h.r_cable}
Vtv b tvk 0
D1 0 tvk DTVS
{TVS}
Rptc b c {h.r_ptc}
C1 c n1 {c1}
R1 n1 0 {ESR}
C2 c n2 {c2}
R2 n2 0 {ESR}
C4 c n4 {c4}
R4 n4 0 {ESR}
C3 c n3 {cs}
R3 n3 0 {ESR}
.control
tran 20n 400u
let ic = -i(vusb)
let itvs = i(vtv)
wrdata v.dat v(c) v(b)
wrdata i.dat ic itvs
.endc
.end
"""
    o = ngspice.run(net, ["v", "i"])
    v, i = o["v"], o["i"]
    return {"vbus_f_peak": float(v[:, 1].max()), "vbus_raw_peak": float(v[:, 3].max()), "i_peak": float(i[:, 1].max()),
            "tvs_peak": float(i[:, 3].max())}


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
