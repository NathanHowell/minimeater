# Run a scenario on the host digital twin, e.g. `just twin --scenario kamado-brisket --speed 60x`.
twin *args:
    cd fw && cargo run --release -p minimeat-twin -- {{args}}

# The scenario suite on the twin (bounds + baselines), then a PNG of each run in fw/target/tmp/scenarios.
scenarios:
    cd fw && cargo test --release -p minimeat-twin --test suite -- --nocapture
    uv run python -I scripts/plot_scenarios.py

# Rewrite the suite's baselines from this run, after a change that is meant to move them.
scenarios-update:
    cd fw && UPDATE_BASELINES=1 cargo test --release -p minimeat-twin --test suite -- --nocapture
