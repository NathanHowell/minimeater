# The firmware workspace (runs in fw/). `just fw::twin::...` runs the application on the host twin; `just fw::interop::...`
# checks it against real brokers, collectors and Home Assistant.

mod twin "twin.just"
mod interop "interop.just"

# Build the ESP32-C6 firmware (release, riscv32imac).
build:
    cargo fw-build

# Build, flash over the box's USB-C and open the serial monitor (`cargo install espflash --locked`).
flash:
    cargo fw

# Host tests (release): the whole workspace but the board crate, which only builds for the target; or what the args
# pick, e.g. `just fw::test -p minimeater-sim --test autotune`.
# Host tests: every crate but the board one, or the args' selection (`-p CRATE --test NAME`).
test *args:
    cargo test --release {{ if args == "" { "--workspace --exclude minimeater-fw" } else { args } }}

# Format the workspace.
fmt:
    cargo fmt --all

# Format check and clippy, warnings as errors.
lint:
    cargo fmt --all -- --check
    cargo clippy --workspace --exclude minimeater-fw --all-targets -- -D warnings

# What CI runs: lint, host tests, the target build.
ci: lint test build

# The sim's brisket curve at a pit temperature (°F).
meat-curve pit="250":
    cargo run -p minimeater-sim --example meat_curve -- {{ pit }}
