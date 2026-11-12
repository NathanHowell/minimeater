"""`just model::thermistor-table`: the Rust thermistor test's `REFERENCE` table (fw/minimeater-core/src/thermistor.rs),
printed from the Python probes and front end so the port is checked against them: (preset, F, R ohm, ratio at the bias)."""

from .probes import PROBES, FrontEnd
from .units import f_to_k

PRESETS = {"ThermoWorks Pro-Series": "ThermoworksPro", "Maverick ET-732": "MaverickEt732", "Generic 100k B3950": "GenericB3950"}
POINTS_F = (32.0, 225.0, 500.0)


def rows(fe=FrontEnd()):
    for probe in PROBES:
        for t_f in POINTS_F:
            r = probe.r(f_to_k(t_f))
            yield PRESETS[probe.name], t_f, r, fe.ratio(r)


def main():
    out = list(rows())
    print(f"    const REFERENCE: [(Preset, f64, f64, f64); {len(out)}] = [")
    for preset, t_f, r, x in out:
        print(f"        (Preset::{preset}, {t_f!r}, {r!r}, {x!r}),")
    print("    ];")


if __name__ == "__main__":
    main()
