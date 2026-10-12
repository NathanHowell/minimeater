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
| `docs/telemetry.md` | OpenTelemetry metrics the device exports | |
| `src/minimeat/model/` | Python plant, airflow and controller models; the reference for the firmware | uv |
| `src/minimeat/mech/` | build123d enclosure, damper and tube coupling (`.2.x`, not started) | uv |
| `hw/` | KiCad project, JLCPCB-assembled; every part must be in JLC stock | KiCad 9 |
| `fw/` | Rust cargo workspace: `minimeat-core` (`no_std` control, food estimator and probe conversion), `minimeat-app` (application over peripheral traits, with the MQTT payload codec), `minimeat-sim` (plant model and simulated board), `minimeat-twin` (the app's tasks on a virtual clock against the simulated board) and `minimeat-fw` (ESP32-C6 board bindings, embassy) | cargo, espflash |
| `tests/` | Python tests for the models and doc examples | `uv run pytest` |
| `scripts/` | helpers, e.g. `uv run scripts/jlc_stock.py <part>` for a JLC stock check | uv |

## Build and test

```bash
uv run pytest                    # Python models and doc checks
uv run minimeat-model            # regenerate docs/modeling/*.md (also -control, -probes)
cd fw && cargo test                    # firmware core, app and sim on the host
cd fw && cargo run -p minimeat-fw --release   # build and flash over USB-C (ESP32-C6)
just twin --scenario kamado-brisket --speed 60x   # the app on the host twin (default: flat out)
just scenarios                         # the 17-scenario suite on the twin: bounds + baselines, PNG per run
just mqtt-interop                     # the MQTT client against Mosquitto in a container (Apple `container` CLI) and rumqttc
just ha-discovery-check                # Home Assistant's own code validates the device-discovery message
just otlp-interop                     # the OTLP exporter against a real OpenTelemetry collector in a container (docs/telemetry.md)
just stack-up / twin-stack / stack-down  # Mosquitto + OTel collector in containers; the twin's cook against them (--speed 300)
just ha-live-check                     # real Home Assistant (container) onboarded, MQTT added: the twin's entities appear and a setpoint round-trips
```

Work is tracked with [beads](https://github.com/gastownhall/beads): run `bd ready`.
