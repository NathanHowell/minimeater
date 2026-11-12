# minimeater's tasks, one module per area: `just` lists them all. Recipes are called by path, e.g.
# `just fw::twin::run --scenario kamado-brisket --speed 60x` or `just mech::export`.
# The firmware modules (fw/*.just) run in fw/; the others run at the repository root, so paths given to them are
# relative to it.

mod py "just/py.just"
mod model "just/model.just"
mod mech "just/mech.just"
mod hw "hw/mod.just"
mod fw "fw/mod.just"
mod stack "tools/stack.just"
mod vendor "just/vendor.just"

# List every recipe.
default:
    @just --list --list-submodules

# Every test that needs no container, KiCad or target: the Python suite and the firmware on the host.
test: py::test fw::test

# Everything CI runs: Python and the enclosure export, firmware (format, clippy, host tests, the ESP32-C6 build), KiCad ERC and DRC.
ci: py::ci fw::ci hw::check
