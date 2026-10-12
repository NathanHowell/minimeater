# Project Instructions for AI Agents

This file provides instructions and context for AI coding agents working on this project.

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:1105d646 -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/core-concepts/sync-concepts.md for details and anti-patterns.

## Agent Context Profiles

The managed Beads block is task-tracking guidance, not permission to override repository, user, or orchestrator instructions.

- **Conservative (default)**: Use `bd` for task tracking. Do not run git commits, git pushes, or Dolt remote sync unless explicitly asked. At handoff, report changed files, validation, and suggested next commands.
- **Minimal**: Keep tool instruction files as pointers to `bd prime`; use the same conservative git policy unless active instructions say otherwise.
- **Team-maintainer**: Only when the repository explicitly opts in, agents may close beads, run quality gates, commit, and push as part of session close. A current "do not commit" or "do not push" instruction still wins.

## Session Completion

This protocol applies when ending a Beads implementation workflow. It is subordinate to explicit user, repository, and orchestrator instructions.

1. **File issues for remaining work** - Create beads for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **Handle git/sync by active profile**:
   ```bash
   # Conservative/minimal/default: report status and proposed commands; wait for approval.
   git status

   # Team-maintainer opt-in only, unless current instructions forbid it:
   git pull --rebase
   git push
   git status
   ```
5. **Hand off** - Summarize changes, validation, issue status, and any blocked sync/commit/push step

**Critical rules:**
- Explicit user or orchestrator instructions override this Beads block.
- Do not commit or push without clear authority from the active profile or the current user request.
- If a required sync or push is blocked, stop and report the exact command and error.
<!-- END BEADS INTEGRATION -->


## Build & Test

```bash
uv run pytest                          # Python models + doc example checks
uv run minimeat-model                  # regenerate docs/modeling (also minimeat-control, minimeat-probes)
cd fw && cargo test                    # firmware core, app and sim on host
cd fw && cargo run -p minimeat-fw --release   # build + flash ESP32-C6 over USB-C
just twin --scenario kamado-brisket --speed 60x   # the app on the host twin (default: flat out)
just scenarios                         # the 20-scenario suite on the twin: bounds + baselines, PNG per run
just mqtt-interop                     # the MQTT client against Mosquitto in a container (Apple `container` CLI) and rumqttc
just ha-discovery-check                # Home Assistant's own code validates the device-discovery message
just otlp-interop                     # the OTLP exporter against a real OpenTelemetry collector in a container (docs/telemetry.md)
just stack-up / twin-stack / stack-down  # Mosquitto + OTel collector in containers; the twin's cook against them (--speed 300)
just ha-live-check                     # real Home Assistant (container) onboarded, MQTT added: the twin's entities appear and a setpoint round-trips
```

## Architecture Overview

- **Python:** `src/minimeat/model/` holds the plant, airflow and controller models. It is the reference implementation the firmware ports. `src/minimeat/mech/` holds build123d parts. There is one uv project.
- **Firmware:** the `fw/` cargo workspace.
  - `minimeat-core` is `no_std` and has a `std` feature for host tests. It holds control, the food estimator and probe conversion.
  - `minimeat-app` is the application, generic over the peripheral traits in `minimeat-app::hal`; it must not use esp-hal or target `cfg`s. It holds the tasks, the probe and actuator drivers, alarms and the MQTT payload codec (`proto`).
  - `minimeat-sim` is the plant model; `minimeat-sim::hal` implements the traits on it for host runs.
  - `minimeat-twin` runs `minimeat-app::tasks` on a virtual-time embassy executor against `minimeat-sim::hal` (`just twin`).
  - `minimeat-fw` is the ESP32-C6 embassy binary; `board.rs` is the only place that knows pins and esp-hal peripherals.
- **Hardware:** `hw/` (KiCad).
- **Decisions and specs:** decisions are in `docs/adr/`. The MQTT schema in `docs/protocol.md` is normative.

## Conventions & Patterns

- Always use uv for Python (`uv run ...`), never bare `python3`.
- Temperatures are °F throughout: docs, models and the MQTT wire. Airflow is CFM.
- PCB parts must be in JLCPCB stock (5-board build). Prefer the higher-quality part unless the price gap is huge.
- Generated docs (`docs/modeling/*.md` except README) are rewritten by their `uv run minimeat-*` script; don't edit them by hand.
- Commit and push directly to master after each bead, and run `bd dolt push`. No co-authored-by lines.
