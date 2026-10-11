# minimeat

A HeaterMeter-inspired temperature controller for a Big Green Egg Large, run at a 150-325F pit. It is one 3D-printed box that clips onto the egg's stainless intake tube. Inside are:
- an axial fan;
- a servo-driven flap damper;
- an ESP32-C6 board with four thermistor probe jacks;
- a single USB-C port for power, flashing and the serial console.

The controller commands airflow with an offset-free MPC. It reports to Home Assistant over MQTT and exports metrics over OTLP.

## Layout

| Path | What | Tooling |
|---|---|---|
| `docs/adr/` | architecture decisions: MCU, probe front-end, control protocol, ... | |
| `docs/modeling/` | generated airflow, thermal, controller and probe-budget reports, plus hand-written conclusions | `uv run minimeat-model`, `minimeat-control`, `minimeat-probes` |
| `docs/protocol.md` | MQTT topic and payload schema (normative) | |
| `src/minimeat/model/` | Python plant, airflow and controller models; the reference for the firmware | uv |
| `src/minimeat/mech/` | build123d enclosure, damper and tube coupling (`.2.x`, not started) | uv |
| `hw/` | KiCad project, JLCPCB-assembled; every part must be in JLC stock | KiCad 9 |
| `fw/` | Rust cargo workspace: `minimeat-core` (`no_std` control, probes and protocol, host-tested) and `minimeat-fw` (ESP32-C6, embassy) | cargo, espflash |
| `tests/` | Python tests for the models and doc examples | `uv run pytest` |
| `scripts/` | helpers, e.g. `uv run scripts/jlc_stock.py <part>` for a JLC stock check | uv |

## Build and test

```bash
uv run pytest                    # Python models and doc checks
uv run minimeat-model            # regenerate docs/modeling/*.md (also -control, -probes)
cd fw && cargo test -p minimeat-core   # firmware core on the host
cd fw && cargo run -p minimeat-fw --release   # build and flash over USB-C (ESP32-C6)
```

Work is tracked with [beads](https://github.com/gastownhall/beads): run `bd ready`.
