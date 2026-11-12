# The application's tasks on a virtual clock against the simulated board and kamado (fw/minimeater-twin; runs in fw/).

# One scenario, e.g. `just fw::twin::run --scenario kamado-brisket --speed 60x` (default: flat out).
run *args:
    cargo run --release -p minimeater-twin -- {{ args }}

# The scenario suite (bounds and baselines), then a plot of each run (fw/target/tmp/scenarios).
scenarios:
    cargo test --release -p minimeater-twin --test suite -- --nocapture
    just -f ../justfile fw::twin::plot

# Rewrite the suite's baselines from this run, after a change that is meant to move them.
scenarios-update:
    UPDATE_BASELINES=1 cargo test --release -p minimeater-twin --test suite -- --nocapture

# Plot the suite's runs (PNG per run).
plot dir="fw/target/tmp/scenarios":
    cd .. && uv run python -I scripts/plot_scenarios.py {{ dir }}

# The twin against `just stack::up`'s broker and collector, e.g. `just fw::twin::stack --scenario kamado-brisket --speed 600`.
stack *args:
    #!/usr/bin/env bash
    set -euo pipefail
    mq=$(container ls | awk '/mm-mosq/ {split($6, a, "/"); print a[1]}')
    ot=$(container ls | awk '/mm-otel/ {split($6, a, "/"); print a[1]}')
    echo "broker $mq:1883, collector $ot:4318 (Prometheus text on $ot:8889/metrics)"
    cargo run --release -p minimeater-twin -- --mqtt $mq:1883 --otlp $ot:4318 {{ args }}
