# Run a scenario on the host digital twin, e.g. `just twin --scenario kamado-brisket --speed 60x`.
twin *args:
    cd fw && cargo run --release -p minimeat-twin -- {{args}}
