# Project Instructions for AI Agents

Instructions and context for AI coding agents working on this project. `CLAUDE.md` is the same file for Claude Code.

## Build & Test

Every routine command is a `just` recipe, nested by area: `just` lists them all (`just fw::twin::run`, `just mech::export`, ...).
Add a recipe to the module it belongs to (`just/*.just`, `fw/*.just`, `hw/mod.just`, `tools/stack.just`) rather than
documenting a raw `uv run` or `cargo` command.

```bash
just test                              # Python suite + firmware host tests
just ci                                # everything CI runs (py::ci, fw::ci, hw::check)
just model::all                        # regenerate docs/modeling and docs/sim (model::report, ::control, ::probes, ::sim)
just fw::flash                         # build + flash the ESP32-C6 over USB-C
just fw::twin::run --scenario kamado-brisket --speed 60x   # the app on the host twin (default: flat out)
just fw::twin::scenarios               # the scenario suite on the twin: bounds + baselines, PNG per run
just fw::interop::mqtt                 # the MQTT client against Mosquitto in a container (Apple `container` CLI) and rumqttc
just fw::interop::mosquitto-tls        # the same over TLS: certificate name as the user, an ACL by name (podman; MINIMEATER_CONTAINER)
just fw::interop::tls                  # the MQTT client over TLS 1.3 with a client certificate against a Python TLS peer
just fw::interop::ha-discovery         # Home Assistant's own code validates the device-discovery message
just fw::interop::otlp                 # the OTLP exporter against a real OpenTelemetry collector in a container
just fw::interop::ha-live              # real Home Assistant (container): the twin's entities appear and a setpoint round-trips
just stack::up / stack::down           # Mosquitto + OTel collector in containers; `just fw::twin::stack` cooks against them
just hw::fab                           # gerbers, BOM and CPL for JLCPCB (hw/fab)
just vendor::fetch                     # the vendor 3D models (not in the repository)
```

## Architecture Overview

- **Python:** `src/minimeater/model/` holds the plant, airflow and controller models. It is the reference implementation the firmware ports. `src/minimeater/mech/` holds the build123d enclosure (`docs/mech.md`; `just mech::export`, `just mech::render docs/mech`). There is one uv project.
- **Firmware:** the `fw/` cargo workspace.
  - `minimeater-core` is `no_std` and has a `std` feature for host tests. It holds control, the food estimator and probe conversion.
  - `minimeater-app` is the application, generic over the peripheral traits in `minimeater-app::hal`; it must not use esp-hal or target `cfg`s. It holds the tasks, the probe and actuator drivers, alarms and the MQTT payload codec (`proto`).
  - `minimeater-sim` is the plant model; `minimeater-sim::hal` implements the traits on it for host runs.
  - `minimeater-twin` runs `minimeater-app::tasks` on a virtual-time embassy executor against `minimeater-sim::hal` (`just fw::twin::run`).
  - `minimeater-fw` is the ESP32-C6 embassy binary; `board.rs` is the only place that knows pins and esp-hal peripherals.
- **Hardware:** `hw/` (KiCad).
- **Decisions and specs:** decisions are in `docs/adr/`. The MQTT schema in `docs/protocol.md` is normative.

## Conventions & Patterns

- Always use uv for Python (`uv run ...`), never bare `python3`.
- Temperatures are °F throughout: docs, models and the MQTT wire. Airflow is CFM.
- PCB parts must be in JLCPCB stock (5-board build). Prefer the higher-quality part unless the price gap is huge.
- Generated docs (`docs/modeling/*.md` except README, `docs/sim/*.md`) are rewritten by their `just model::*` recipe; don't edit them by hand.
