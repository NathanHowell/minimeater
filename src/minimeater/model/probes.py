"""Thermistor probe front-end: divider + ADC error budget.

Topology (one per jack): VREF --- R_bias ---+--- jack tip ~~~ probe NTC ~~~ jack sleeve --- AGND
                                            |
                                           AINx (single-ended vs AGND)
With a ratiometric ADC (ADS1x20: REFP0 tied to the same VREF that feeds the dividers) the reading is
the ratio x = R_t / (R_t + R_bias) and supply drift cancels. The ESP32 SAR ADC has an internal
reference, so the rail error appears directly unless VREF is measured on another channel.

Confidence tags as in params.py: [spec] datasheet, [forum] community data, [est] estimate.
"""

import math
from dataclasses import dataclass, field

from .units import c_to_k, f_to_k


@dataclass(frozen=True)
class Thermistor:
    name: str
    a: float
    b: float
    c: float
    source: str
    dissipation_w_k: float = 1.5e-3  # probe in still air [est]; much higher in meat

    def t_k(self, r: float) -> float:
        ln = math.log(r)
        return 1.0 / (self.a + self.b * ln + self.c * ln**3)

    def r(self, t_k: float) -> float:
        if self.c == 0.0:
            return math.exp((1.0 / t_k - self.a) / self.b)
        # invert 1/T = a + b*ln(R) + c*ln(R)^3 (depressed cubic in ln R, one real root)
        p = self.b / self.c
        q = (self.a - 1.0 / t_k) / self.c
        disc = math.sqrt((q / 2) ** 2 + (p / 3) ** 3)
        return math.exp(math.cbrt(-q / 2 + disc) + math.cbrt(-q / 2 - disc))

    def dlnr_dt(self, t_k: float) -> float:
        ln = math.log(self.r(t_k))
        return -1.0 / (t_k**2 * (self.b + 3 * self.c * ln**2))


def beta_thermistor(name: str, r25: float, beta: float, source: str) -> Thermistor:
    """Beta model expressed as Steinhart-Hart with c = 0."""
    b = 1.0 / beta
    return Thermistor(name, 1.0 / c_to_k(25.0) - b * math.log(r25), b, 0.0, source)


PROBES = [
    Thermistor(
        "ThermoWorks Pro-Series", 7.343140544e-4, 2.157437229e-4, 9.51568577e-8,
        "[forum] tvwbb 69233, fitted on a calibration block (~96k at 25C)",
    ),
    Thermistor("Maverick ET-732", 5.3612e-4, 1.91532e-4, 6.557e-8, "[forum] LinkMeter v2 thread (~1M at 25C)"),
    beta_thermistor("Generic 100k B3950", 100e3, 3950.0, "[est] typical import probe"),
]


@dataclass(frozen=True)
class Adc:
    name: str
    noise_vrms: float  # input-referred, per conversion
    lsb_v: float  # code width at VREF = 3.3 V
    offset_v: float  # after a one-time offset calibration where possible
    gain_err: float  # relative; the ratiometric reference does not cancel ADC-internal gain error
    rail_err: float  # relative error of VREF as seen by the ADC (0 when ratiometric)
    i_in_a: float  # input current incl. ESD protection leakage, at a hot board (~60C)
    notes: str = ""


ADCS = [
    Adc(
        "ADS1220 (24-bit, ratiometric)", 3.89e-6, 6.6 / 2**24, 30e-6, 1e-3, 0.0, 30e-9,
        "[spec] PGA bypassed, gain 1, 20 SPS: 3.89 uVrms; offset 30 uV max; gain err 0.1% max (PGA on, "
        "0.015% typ off); abs input current ~5 nA @25C, 20-50 nA @85C (Fig 6-22..6-24) + [est] ESD leakage",
    ),
    Adc(
        "ADS1120 (16-bit, ratiometric)", 0.0, 6.6 / 2**16, 30e-6, 1e-3, 0.0, 30e-9,
        "[est] same analog core as ADS1220, quantisation-limited at 20 SPS; offset/gain assumed equal",
    ),
    Adc(
        "ESP32-C6 SAR + VREF channel", 2e-3, 3.3 / 4096, 10e-3, 0.0, 2e-3, 100e-9,
        "[est] 12-bit, eFuse-calibrated, INL ~+/-10 LSB (Espressif blog 2025-08); rail measured on a 5th "
        "channel, residual 0.2%; sampling cap needs a low-impedance node or big C",
    ),
]


@dataclass(frozen=True)
class FrontEnd:
    r_bias: float = 47e3  # 0.1% 25ppm, Vishay TNPW060347K0BEEA (JLC C2089026)
    adc: Adc = field(default_factory=lambda: ADCS[0])
    v_ref: float = 3.3
    averaging: int = 8  # conversions averaged per reported sample
    r_tol: float = 1e-3  # 0.1% bias resistor, 25 ppm/K
    r_shunt: float = 20e6  # [est] damp probe / wet jack insulation leakage
    r_lead: float = 1.0  # [est] cable + jack round trip

    def ratio(self, r_t: float) -> float:
        return r_t / (r_t + self.r_bias)

    def errors_k(self, probe: Thermistor, t_k: float) -> dict[str, float]:
        """Temperature error magnitude (K) from each mechanism at true temperature t_k."""
        a = self.adc
        r_t = probe.r(t_k)
        s = abs(probe.dlnr_dt(t_k))  # fractional resistance change per K
        x = self.ratio(r_t)
        dx_dt = x * (1 - x) * s
        r_th = r_t * self.r_bias / (r_t + self.r_bias)
        noise_x = math.hypot(a.noise_vrms, a.lsb_v / math.sqrt(12)) / self.v_ref / math.sqrt(self.averaging)
        if a.noise_vrms < a.lsb_v / 2:  # no dither: averaging does not help, worst case is half an LSB
            noise_x = a.lsb_v / 2 / self.v_ref / 3
        return {
            "noise/quantisation (3 sigma)": 3 * noise_x / dx_dt,
            "ADC offset": a.offset_v / self.v_ref / dx_dt,
            "ADC gain + rail": (a.gain_err + a.rail_err) * x / dx_dt,
            "input current": a.i_in_a * r_th / self.v_ref / dx_dt,
            "bias tolerance": self.r_tol / s,
            "self-heating": (self.v_ref * x) ** 2 / r_t / probe.dissipation_w_k,
            "lead resistance": self.r_lead / r_t / s,
            "probe shunt leakage": -math.log(self.r_shunt / (r_t + self.r_shunt)) / s,
        }

    def total_k(self, probe: Thermistor, t_k: float, include_probe: bool = True) -> float:
        """Worst-case linear sum. include_probe=False drops the terms set by the probe, not our design."""
        e = self.errors_k(probe, t_k)
        if not include_probe:
            e.pop("lead resistance")
            e.pop("probe shunt leakage")
        return sum(e.values())


# Where each role actually operates. Food probes also see 32F (ice-bath check).
RANGES_F = {"food": (32.0, 212.0), "pit": (150.0, 500.0)}
GRID_F = (32.0, 100.0, 150.0, 212.0, 225.0, 325.0, 400.0, 500.0)


def worst_f(fe: FrontEnd, probes=PROBES, include_probe: bool = False) -> float:
    """Worst design-attributable error (F) over probe families, both roles, 32-500F."""
    return max(fe.total_k(p, f_to_k(t), include_probe) * 1.8 for p in probes for t in GRID_F)


E12 = [1.0, 1.2, 1.5, 1.8, 2.2, 2.7, 3.3, 3.9, 4.7, 5.6, 6.8, 8.2]


def e12_range(lo: float, hi: float) -> list[float]:
    out = []
    decade = 10 ** math.floor(math.log10(lo))
    while decade <= hi:
        out += [round(v * decade) for v in E12 if lo <= v * decade <= hi]
        decade *= 10
    return out


@dataclass(frozen=True)
class Detect:
    short_below: float  # ratio
    open_above: float
    hottest_valid: float  # lowest ratio a real probe produces (700F, lowest-R family)
    coldest_valid: float  # highest ratio a real probe produces (-20F, highest-R family)


def detect_thresholds(fe: FrontEnd, probes=PROBES) -> Detect:
    hottest = min(fe.ratio(p.r(f_to_k(700.0))) for p in probes)
    coldest = max(fe.ratio(p.r(f_to_k(-20.0))) for p in probes)
    # geometric midpoints between the valid band and the rails (in resistance terms)
    r_short = math.sqrt(1.0 * hottest / (1 - hottest) * fe.r_bias)  # between 1 ohm and the hottest probe
    r_open = math.sqrt(coldest / (1 - coldest) * fe.r_bias * 200e6)  # between the coldest probe and 200M
    return Detect(fe.ratio(r_short), fe.ratio(r_open), hottest, coldest)
