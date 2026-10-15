"""Power tree dissipation (`.3.2`): what each part on the 5 V path burns at ADR 0004's operating points, the junction
temperature that gives at the board's 85 C ceiling, and the fault cases.

Closed form, no simulator. Board, as on the schematic: J1 -> VBUS_RAW (D1 SMF5V0A TVS) -> U12 TPS259531 eFuse -> VBUS_F;
VBUS_F feeds U4 TLV62569 (3V3 buck with L1) for the MCU, ADS122C04 DVDD, TMP117 and ATECC608B; FB1 -> U5 TLV75533P LDO
(3V3A) for the ADS122C04's AVDD and the probe bias; U6 and U7 TPS2553 switch the servo and fan. The first board's PTC
(F1) is gone (U12 replaced it, minimeat-8s2). Sources are tagged [spec] (datasheet), [fig] (read off a datasheet
graph, about +-1 point) or [est].
"""

from dataclasses import dataclass

import numpy as np

V_BUS = 5.0  # VBUS_F nominal; the source is 4.75-5.25 V
V_3V3 = 3.32  # R3 453k / R4 100k (docs/sim/power.md)
T_BOARD = 85.0  # C, the hottest the board may run (ICD)
TJ_MAX = 125.0  # C, operating limit of U4, U5, U6, U7 and U12 [spec]

# ADR 0004's loads
I_MCU_COOK = 0.080  # A at 3.3 V: ESP32-C6 with WiFi
I_MCU_TX = 0.350  # A at 3.3 V: TX burst
I_DIGITAL = 0.002  # A at 3.3 V: ADS122C04 DVDD (100 uA max), TMP117, ATECC608B asleep, pull-ups
I_FAN_COOK = 0.030  # A at 5 V
I_FAN_MAX = 0.190  # A at 5 V, full speed
I_SERVO_RUN = 0.220  # A at 5 V, moving (HS-5065MG: 180 mA running)
I_SERVO_LIMIT = 0.60  # A, U6 with R5 43.2k
I_FAN_LIMIT = 0.519  # A, U7 with R6 49.9k
I_STALL = (0.96, 1.2)  # A at 5 V, servo stall (ADR 0004)

# U5's load: ADS122C04 AVDD 510 uA max (normal mode, gain 1-16) + 60 uA external reference [spec, SBAS751B]; the four
# probe biases to VREF_PROBE (47k bias, 1k series, probe shorted: the most they can pull) [calc]
I_ANALOG = 0.51e-3 + 0.06e-3 + 4 * V_3V3 / (47e3 + 1e3)

# U4 TLV62569 (SLVSDG1C): efficiency at VIN 5 V, VOUT 3.3 V, read off Figure 9 [fig]
ETA_LOAD = np.array([0.001, 0.003, 0.01, 0.03, 0.1, 0.35, 1.0, 2.0])
ETA = np.array([0.90, 0.93, 0.94, 0.945, 0.95, 0.95, 0.955, 0.92])
ETA_BOARD_ALLOWANCE = 0.02  # [est] taken off the curve: its inductor, layout and 25 C are not ours (L1 is 78 mohm max, board at 85 C)
BUCK_RHS, BUCK_RLS, BUCK_IQ = 0.100, 0.060, 35e-6  # typ [spec]; no maximum is published
L1_DCR = 0.078  # LQH3NPN2R2MMEL, distributor listings (max) [est: Murata's sheet not read]
L1_ISAT = 1.8  # A, same source [est]
BUCK_ILIM = 3.0  # A [spec]
RDS_TEMPCO = 1.35  # MOSFET resistance at about 105 C over 25 C [est]

LDO_IGND = 33e-6  # A, TLV75533P at 85 C, no load [spec, 25 C typ 14 uA, 31 max; 33 uA max to 85 C]
SW_RON_MAX = 0.135  # ohm, TPS2553 DBV over temperature [spec]
SW_IIN_ON = 140e-6  # A, TPS2553 enabled, no load [spec]
EFUSE_RON_85 = 0.047  # ohm, TPS259531 at 85 C max (34 mohm typ at 25 C) [spec]
EFUSE_IQ = 250e-6  # A max [spec]
EFUSE_ILIM = 2.04  # A, R35 1k
TVS_LEAK = 0.4e-3  # A at 5 V, SMF5.0A maximum (one listing says 0.8 mA) [est: distributor listings]

# junction-to-ambient, C/W
THETA = {"D1": 200.0,  # [est] SOD-123FL
         "U12": 65.6,  # [spec]
         "U4": 188.2,  # [spec] JEDEC; the datasheet's EVM figure for the SOT-23 LDO is lower
         "U5": 231.1,  # [spec] JEDEC DBV; EVM 100.8
         "U6": 182.6, "U7": 182.6}  # [spec]


def buck_eta(i_out: float) -> float:
    """Datasheet curve less the board allowance."""
    return float(np.interp(np.log10(max(i_out, ETA_LOAD[0])), np.log10(ETA_LOAD), ETA)) - ETA_BOARD_ALLOWANCE


def buck_conduction(i_out: float) -> float:
    """The part of U4's loss the datasheet's numbers give directly: the FETs and L1 in series over a cycle, plus its
    quiescent current. The rest of the loss (switching, gate drive) is what the efficiency curve adds."""
    d = V_3V3 / V_BUS
    return i_out**2 * (RDS_TEMPCO * (d * BUCK_RHS + (1 - d) * BUCK_RLS) + L1_DCR) + BUCK_IQ * V_BUS


@dataclass(frozen=True)
class Scenario:
    name: str
    i_3v3: float  # A drawn from the 3V3 rail
    i_fan: float  # A at 5 V
    i_servo: float  # A at 5 V
    fan_on: bool = True
    servo_on: bool = True


SCENARIOS = (
    Scenario("cook (WiFi idle, fan trickle, servo gated)", I_MCU_COOK + I_DIGITAL, I_FAN_COOK, 0.0, servo_on=False),
    Scenario("worst normal (TX burst, fan full, servo moving)", I_MCU_TX + I_DIGITAL, I_FAN_MAX, I_SERVO_RUN),
    Scenario("1.5 A budget (TX burst, fan full, servo at its 0.6 A limit)", I_MCU_TX + I_DIGITAL, I_FAN_MAX, I_SERVO_LIMIT),
)


def solve(s: Scenario) -> dict[str, float]:
    """Power (W) in each part and the totals for one scenario."""
    p3 = V_3V3 * s.i_3v3
    eta = buck_eta(s.i_3v3)
    p_buck = p3 / eta - p3
    i_buck_in = p3 / eta / V_BUS
    p_ldo = (V_BUS - V_3V3) * I_ANALOG + V_BUS * LDO_IGND
    i_ldo_in = I_ANALOG + LDO_IGND
    p_fan_sw = (s.i_fan**2 * SW_RON_MAX if s.fan_on else 0.0) + (V_BUS * SW_IIN_ON if s.fan_on else 0.0)
    p_servo_sw = (s.i_servo**2 * SW_RON_MAX if s.servo_on else 0.0) + (V_BUS * SW_IIN_ON if s.servo_on else 0.0)
    i_f = i_buck_in + i_ldo_in + s.i_fan + s.i_servo + (SW_IIN_ON if s.fan_on else 0) + (SW_IIN_ON if s.servo_on else 0)
    p_efuse = i_f**2 * EFUSE_RON_85 + V_BUS * EFUSE_IQ
    p_tvs = TVS_LEAK * V_BUS
    i_in = i_f + EFUSE_IQ + TVS_LEAK
    p_board = p_tvs + p_efuse + p_buck + p_ldo + p_fan_sw + p_servo_sw
    return {"i_3v3": s.i_3v3, "i_buck_in": i_buck_in, "i_f": i_f, "i_in": i_in, "p_in": V_BUS * i_in, "eta": eta,
            "D1": p_tvs, "U12": p_efuse, "U4": p_buck, "U5": p_ldo, "U7": p_fan_sw, "U6": p_servo_sw,
            "p_board": p_board, "buck_conduction": buck_conduction(s.i_3v3)}


def tj(part: str, p: float, t_board: float = T_BOARD) -> float:
    return t_board + p * THETA[part]


def faults() -> dict[str, dict[str, float]]:
    """Fault cases: power in the part that takes the fault, and what ends it."""
    out = {}
    for tag, i_stall in (("hard", I_STALL[1]), ("light", I_STALL[0])):
        r = V_BUS / i_stall  # rotor resistance
        v_servo = I_SERVO_LIMIT * r
        out[f"U6, servo stalled at {i_stall:g} A ({tag})"] = {"i": I_SERVO_LIMIT, "drop": V_BUS - v_servo,
                                                              "p": (V_BUS - v_servo) * I_SERVO_LIMIT}
    out["U6, VSERVO shorted to ground"] = {"i": I_SERVO_LIMIT, "drop": V_BUS, "p": V_BUS * I_SERVO_LIMIT}
    out["U7, VFAN shorted to ground"] = {"i": I_FAN_LIMIT, "drop": V_BUS, "p": V_BUS * I_FAN_LIMIT}
    out["U12, VBUS_F shorted to ground"] = {"i": EFUSE_ILIM, "drop": V_BUS, "p": V_BUS * EFUSE_ILIM}
    return out


def efuse_start() -> dict[str, float]:
    """U12 charging VBUS_F at its soft-start rate: the FET carries C dV/dt while its drop falls from 5 V to nothing."""
    c = 100e-6 + 2 * 10e-6 + 3 * 100e-9  # nominal; derated is half that
    sr = 4200.0  # V/s, C42 10 nF
    t = V_BUS / sr
    return {"c": c, "energy": 0.5 * c * V_BUS**2, "t": t, "p_avg": 0.5 * c * V_BUS**2 / t, "i": c * sr}
