"""5 V and 3.3 V rail transients in ngspice (`.3.14`): plug-in and power-on, the radio's TX bursts, the servo's turn-on and
stall, and weak or sagging USB sources, through the whole path of the schematic's power sheet.

Path: USB source (V, cable R and L, or a current-limited source) -> D1 TVS, C38/C41 -> U12 TPS259531 eFuse (behavioural,
as `power.py`) -> VBUS_F (C1 100 uF, C2/C4 10 uF, 100 nF) -> U4 (3V3 buck), FB1 -> U5 (3V3A LDO), U6 (servo switch), U7 (fan
switch).

U4 is two behavioural halves (TI's transient model of the TLV62569 does run, `ti_models.py`, `buck_ti.py`, but it is optimistic
against the datasheet's own waveforms: a third of the PSM ripple, 0.6 of the load-step dip, 97-99 % efficiency where the part is
95 %. So the scenarios and the 3V3 floor use the capture-based half below, and `Rails(buck="ti")` runs the same netlist with TI's
model as U4, which `ti_crosscheck` does for six of them as a check on the 5 V side):
- its input is a constant-power load (the converter's efficiency over the 3V3 power), switched on at UVLO and soft-started over
  800 us, which is what the 5 V rail sees (and the only source of rail interaction: a regulator's input resistance is negative);
- its output is the regulated 3V3 less the load step's droop, taken from the datasheet's own transient capture (SLVSDG1C
  Figure 18: a 1.2 A step at 1 A/us dips 0.2 V and is back in about 4 us, so 0.167 V per amp and 4 us, applied as a high-pass
  on the load current), and less dropout when VBUS_F is too low to regulate. The regulation window (VFB 0.588-0.612 V, 1 %
  resistors) is a parameter. PSM ripple is not in it (about 15-30 mV at this output capacitance, `docs/sim/rails.md`).
The loop itself is not modelled: a PI loop that reproduces the capture's 0.2 V dip and 4 us recovery is not a fit of this
controller (DCS-Control has far more DC gain than the capture can set), so the capture is used as measured.

U5 is its dropout and regulation window on a 1 uF input behind FB1; the module and the ADC are current loads.
"""

from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field, replace

import numpy as np

from . import ngspice, ti_models
from .dissipation import L1_DCR
from .power import CAPS, EFUSE_CLAMP, EFUSE_DELAY, EFUSE_ILIM, EFUSE_RON, EFUSE_SR, ESR, TVS

V3_NOM = 0.6 * (1 + 453e3 / 100e3)  # R3 / R4: 3.318 V
V3_LOW = 0.588 * (1 + 453e3 * 0.99 / (100e3 * 1.01))  # VFB low, resistors 1 % the wrong way: 3.205 V
V3_HIGH = 0.612 * (1 + 453e3 * 1.01 / (100e3 * 0.99))
V3A = 3.3
ETA = 0.93  # U4 at 0.08-0.35 A, input 5 V, with the board's allowance (`dissipation.py`)
ETA_TI = 0.985  # TI's model at 0.1-0.35 A (it has no switching loss): what the model-in-the-loop runs add back to ETA
BUCK_SS = 0.8e-3  # s [spec]
BUCK_UVLO_ON, BUCK_UVLO_OFF = 2.45, 2.3  # V [spec]
DROPOUT_R = 0.1 * 0.664 + 0.06 * 0.336 + 0.078  # ohm: high side over a cycle at the duty, low side, L1 DCR
DIP_V_PER_A = 0.167  # Figure 18: 0.2 V for a 1.2 A step
DIP_TAU = 4e-6  # s
C_OUT_3V3 = 3 * 10e-6 * 0.55 + 5 * 100e-9 * 0.9  # derated at 3.3 V (`power.md`)
R_FB1 = 0.1  # ohm: 600R@100MHz 1.3 A ferrite, DC resistance [est]
I_ANALOG = 0.85e-3  # A on 3V3A (`dissipation.py`)
UVLO_EFUSE_OFF = 3.65  # V at the eFuse's input, falling (R33 / R34)
V_MCU_MIN = 3.0  # V, ESP32-C6-WROOM-1 VDD33 minimum [spec]


@dataclass(frozen=True)
class Rails:
    v_src: float = 5.0
    r_cable: float = 0.1  # cable, connector and the source's own output resistance
    l_cable: float = 1e-6
    i_limit: float | None = None  # a current-limited source (a Default or 1.5 A port): no cable inductance then
    caps: str = "derated"
    c1: float | None = None  # VBUS_F bulk override, F (derated value)
    v3: float = V3_NOM
    c_out: float = C_OUT_3V3
    i_mcu_idle: float = 0.080  # A at 3.3 V
    i_mcu_tx: float = 0.352
    tx: tuple[float, float, int] | None = None  # (start s, burst s, count): bursts every 2 x burst
    servo: str = "off"  # "off", "stall" (0.6 A limit, rotor locked) or "run" (0.22 A)
    t_servo: float = 6e-3
    fan_on: bool = False
    t_fan: float = 7e-3
    t_mcu: float = 3.5e-3  # the module starts drawing once 3V3 is up (EN delay)
    t_stop: float = 12e-3
    label: str = ""
    buck: str = "behavioural"  # "behavioural" (the capture-based U4 above) or "ti" (TI's transient model, `ti_models.py`)


TI_STEP = 50e-9  # s, the ceiling on the time step with TI's switching model (20 ns gives the same dip and ripple at twice the run time)


def _pwl(points: list[tuple[float, float]]) -> str:
    return "PWL(" + " ".join(f"{t:.9g} {v:.9g}" for t, v in points) + ")"


def _load_pwl(r: Rails) -> str:
    pts = [(0, 0), (r.t_mcu, 0), (r.t_mcu + 1e-6, r.i_mcu_idle)]
    if r.tx:
        t0, w, n = r.tx
        for k in range(n):
            a = t0 + 2 * w * k
            pts += [(a, r.i_mcu_idle), (a + 1e-6, r.i_mcu_tx), (a + w, r.i_mcu_tx), (a + w + 1e-6, r.i_mcu_idle)]
    pts.append((r.t_stop, r.i_mcu_idle))
    return _pwl(pts)


def netlist(r: Rails) -> str:
    c1, c2, c4, cs = CAPS[r.caps]
    if r.c1 is not None:
        c1 = r.c1
    if r.i_limit:  # Norton source with a hard limit, ramping up over 1 us like a plug-in
        src = (f"Vramp ramp 0 PWL(0 0 1u 1)\n"
               f"Bsrc 0 bs I = {r.i_limit} * tanh(4 * max(v(ramp) * ({r.v_src} - v(b)) / {r.r_cable}, 0) / {r.i_limit})\n"
               "Rbs bs 0 1meg\nVsens bs b 0\n")
        isrc = "let isrc = i(vsens)\n"
    else:
        src = f"Vusb src 0 PWL(0 0 1u {r.v_src})\nLcab src a {r.l_cable}\nRcab a b {r.r_cable}\n"
        isrc = "let isrc = -i(vusb)\n"
    if r.buck == "ti":
        lib = ti_models.tlv62569()
        if lib is None:
            raise RuntimeError("TI's TLV62569 model is not available (ti_models.tlv62569)")
        u4 = f"""* U4: TI's transient model (EN on VIN as on the board), L1, the output capacitors, R3/R4 and C7; the 3V3 load is a sink
.include {lib}
X4 c fb sw c 0 {ti_models.SUBCKT}
L1 sw lo 2.2u
Rl1 lo o {L1_DCR}
Co o co {r.c_out}
Rco co 0 {ESR}
Rt o fb {100e3 * (r.v3 / 0.6 - 1)}
Rb fb 0 100k
C7 o fb 6.8p
Vil il 0 {_load_pwl(r)}
Bld o 0 I = v(il) * min(max(v(o), 0) / 0.5, 1)
* what TI's model leaves out: its losses, as power taken at VBUS_F
Bxl c 0 I = (v(c) > {BUCK_UVLO_OFF}) * max(v(o), 0) * v(il) * {1 / ETA - 1 / ETA_TI} / max(v(c), 1)
"""
        opts = ".options reltol=1e-3 method=gear gmin=1e-9 abstol=1e-9 vntol=1e-5\n"
        tran = f"tran {TI_STEP} {r.t_stop} 0 {TI_STEP}"
    else:
        u4 = f"""* U4: soft start of the output set point (1 over {BUCK_SS} s while VBUS_F is above UVLO, back to 0 below it)
Bss 0 ss I = ((v(c) > {BUCK_UVLO_ON}) * (v(ss) < 1) / {BUCK_SS}) - ((v(c) < {BUCK_UVLO_OFF}) * (v(ss) > 0) * 1000)
Css ss 0 1
Rss ss 0 1e12
* 3V3 load (volts = amps) and its high-pass for the dip
Vil il 0 {_load_pwl(r)}
Rf il ilp 1k
Cf ilp 0 {DIP_TAU / 1e3}
Bo o 0 V = min({r.v3} * v(ss), v(c) - v(il) * {DROPOUT_R}) * (v(ss) > 0.001) - {DIP_V_PER_A} * (v(il) - v(ilp))
* U4's input: constant power through the converter's efficiency, plus the output capacitor's charging
Bq c 0 I = (v(c) > {BUCK_UVLO_OFF}) * (35u + (max(v(o), 0) * (v(il) + {r.c_out} * {V3_NOM} / {BUCK_SS} * (v(ss) < 1) * (v(ss) > 0)) / {ETA}) / max(v(c), 1))
"""
        opts = ""
        tran = f"tran 0.1u {r.t_stop} uic"
    servo_r = {"off": 1e9, "stall": 5.0 / 1.2, "run": 5.0 / 0.22}[r.servo]
    ts, tf = r.t_servo, r.t_fan
    return f"""* rails
{src}Vtv b tvk 0
D1 0 tvk DTVS
{TVS}
Cin b nin {2 * cs}
Rin nin 0 {ESR}
* U12 eFuse: soft start from {EFUSE_DELAY}, clamp, current limit; off below the falling UVLO
Bfuse b c I = (v(b) > {UVLO_EFUSE_OFF}) * min(max((min(min(v(b), {EFUSE_CLAMP}), {EFUSE_SR} * max(time - {EFUSE_DELAY}, 0)) - v(c)) / {EFUSE_RON}, 0), {EFUSE_ILIM})
Rbleed c 0 1meg
C1 c n1 {c1}
R1 n1 0 {ESR}
C2 c n2 {c2}
R2 n2 0 {ESR}
C4 c n4 {c4}
R4 n4 0 {ESR}
C3 c n3 {cs * 3}
R3 n3 0 {ESR}
{u4}* U5 behind FB1
Rfb c ldoin {R_FB1}
Cld ldoin 0 0.55u
Bl a3 0 V = min({V3A}, v(ldoin) - 0.005) * (v(ldoin) > 2)
Cl3 a3 0 1.3u
Bla ldoin 0 I = {I_ANALOG} * (v(ldoin) > 2)
* U7 fan and U6 servo load switches (TPS2553: limit 0.519 and 0.6 A), enabled by the MCU
Ven_f ef 0 PWL(0 0 {tf} 0 {tf + 1e-6} {3.3 if r.fan_on else 0})
Ven_s es 0 PWL(0 0 {ts} 0 {ts + 1e-6} {3.3 if r.servo != 'off' else 0})
Bsw_f c vf I = (v(ef) > 1.1) * min(max((v(c) - v(vf)) / 0.1, 0), 0.519)
Cf1 vf 0 10.2u
Rfan vf 0 {5.0 / 0.19 if r.fan_on else 1e9}
Bsw_s c vs I = (v(es) > 1.1) * min(max((v(c) - v(vs)) / 0.1, 0), 0.6)
Cs1 vs 0 {110e-6 if r.caps == 'nominal' else 52e-6}
Rservo vs 0 {servo_r}
Bchk chk 0 V = (time > 0.5m) * (v(b) < {UVLO_EFUSE_OFF + 0.02})
Rchk chk 0 1meg
{opts}.control
stop when v(chk) > 0.5
{tran}
{isrc}wrdata v.dat v(b) v(c) v(o) v(a3) v(vs) v(vf)
wrdata i.dat isrc
.endc
.end
"""


def run(r: Rails) -> dict[str, np.ndarray]:
    o = ngspice.run(netlist(r), ["v", "i"], ps_compat=r.buck == "ti", timeout=1800)
    v = o["v"]
    t = v[:, 0]
    i = o["i"]
    return {"t": t, "vbus_raw": v[:, 1], "vbus_f": v[:, 3], "v3": v[:, 5], "v3a": v[:, 7], "vservo": v[:, 9], "vfan": v[:, 11],
            "i_src": i[:, 1]}


def metrics(r: Rails, w: dict[str, np.ndarray]) -> dict[str, float]:
    """Minima after the rails are up, the power-on times, and the source's draw."""
    t = w["t"]
    up = np.where(w["v3"] >= V_MCU_MIN)[0]
    t_up = float(t[up[0]]) if len(up) else float("nan")
    after = t > t_up + 20e-6 if len(up) else t > r.t_stop
    ev = t > r.t_mcu  # the events: from the module's first draw on
    return {"t_3v3": t_up,
            "v3_max": float(w["v3"].max()),
            "v3_min": float(w["v3"][ev].min()) if len(up) else float("nan"),
            "v3a_min": float(w["v3a"][t > t_up + 0.5e-3].min()) if len(up) else float("nan"),
            "vbus_f_min": float(w["vbus_f"][t > 2.5e-3].min()),
            "vbus_f_end": float(w["vbus_f"][-1]),
            "vbus_f_max": float(w["vbus_f"].max()),
            "vbus_raw_min": float(w["vbus_raw"][t > 2.5e-3].min()),
            "vbus_raw_max": float(w["vbus_raw"].max()),
            "brownout": bool(t[-1] < r.t_stop * 0.999),
            "t_end": float(t[-1]),
            "i_src_max": float(w["i_src"].max()),
            "i_src_end": float(w["i_src"][-50:].mean())}


TX = (9e-3, 0.5e-3, 3)  # three 0.5 ms bursts, 1 ms apart, after the servo and fan are on
WORST = dict(tx=TX, servo="stall", fan_on=True, t_servo=8e-3, t_fan=7e-3, t_stop=14e-3)  # fan, then servo stalled, then TX


def scenarios() -> dict[str, list[tuple[str, Rails, dict[str, float]]]]:
    """Every scenario of the bead, each solved: {group: [(name, setup, metrics)]}. The 3V3 set point is the low end of its
    window for the minima and the high end for the overshoots."""
    def go(name, r):
        return (name, r, metrics(r, run(r)))

    g: dict[str, list] = {}
    g["plug"] = [go(f"{v:g} V, {rc:g} ohm, {lc * 1e6:g} uH cable", Rails(v_src=v, r_cable=rc, l_cable=lc, v3=R_HI)) for v, rc, lc in
                 ((5.25, 0.05, 2e-6), (5.0, 0.1, 1e-6), (4.75, 0.3, 0.25e-6))]
    g["plug"].append(go("0.5 A source (4.75 V, 0.3 ohm), loads gated", Rails(i_limit=0.5, v_src=4.75, r_cable=0.3, l_cable=0, v3=R_HI)))
    g["tx"] = [go("5.0 V, 0.1 ohm", Rails(tx=(5e-3, 0.5e-3, 3), v3=V3_LOW)),
               go("4.75 V, 0.3 ohm, 1.5 A source", Rails(tx=(5e-3, 0.5e-3, 3), v_src=4.75, r_cable=0.3, i_limit=1.5, l_cable=0, v3=V3_LOW)),
               go("4.75 V, 0.3 ohm, 0.5 A source (loads gated)", Rails(tx=(5e-3, 0.5e-3, 3), v_src=4.75, r_cable=0.3, i_limit=0.5, l_cable=0, v3=V3_LOW))]
    g["chain"] = [go("5.0 V, 0.1 ohm, 1 uH", Rails(v3=V3_LOW, **WORST)),
                  go("4.75 V, 0.3 ohm, 2 uH", Rails(v_src=4.75, r_cable=0.3, l_cable=2e-6, v3=V3_LOW, **WORST)),
                  go("4.75 V, 0.3 ohm, 1.5 A source", Rails(v_src=4.75, r_cable=0.3, i_limit=1.5, l_cable=0, v3=V3_LOW, **WORST)),
                  go("4.75 V, 0.3 ohm, 1.0 A source", Rails(v_src=4.75, r_cable=0.3, i_limit=1.0, l_cable=0, v3=V3_LOW, **WORST)),
                  go("4.75 V, 0.3 ohm, 0.5 A source", Rails(v_src=4.75, r_cable=0.3, i_limit=0.5, l_cable=0, v3=V3_LOW, **WORST))]
    g["bulk"] = [go(f"C1 = {c1 * 1e6:g} uF (derated)", Rails(c1=c1, v_src=4.75, r_cable=0.3, l_cable=2e-6, v3=V3_LOW, **WORST))
                 for c1 in (1e-9, 10e-6, 22e-6, 45e-6, 90e-6)]
    return g


R_HI = V3_HIGH


def ti_crosscheck(g: dict[str, list]) -> list[tuple[str, Rails, dict[str, float], dict[str, float]]] | None:
    """Six of `scenarios()`'s runs again with TI's transient model as U4 (3-6 minutes each, in parallel): [(name, setup,
    behavioural metrics, TI metrics)]. None when the model is not available."""
    if ti_models.tlv62569() is None:
        return None
    picks = [("plug-in", g["plug"][1]), ("plug-in, 0.5 A port, loads gated", g["plug"][3]), ("TX bursts", g["tx"][0]),
             ("fan, servo stall and TX, 5.0 V", g["chain"][0]), ("fan, servo stall and TX, 1.5 A port", g["chain"][2]),
             ("fan, servo stall and TX, 0.5 A port", g["chain"][4])]

    def go(pick):
        what, (name, r, m) = pick
        rt = replace(r, buck="ti")
        return f"{what}: {name}", r, m, metrics(rt, run(rt))

    with ThreadPoolExecutor(max_workers=len(picks)) as ex:
        return list(ex.map(go, picks))
