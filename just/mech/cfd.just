# Airflow CFD of the enclosure (docs/cfd.md): OpenFOAM in a container (`container` CLI on a Mac, podman elsewhere:
# MINIMEATER_CONTAINER). Cases and results go to build/.
set working-directory := "../.."

# One duct case, e.g. `just mech::cfd::run open` or `just mech::cfd::run flap-45 --cfm 3`: build/cfd/<case>.
run *args:
    uv run minimeater-cfd {{ args }}

# `run --flap 0 30` runs the grille case and a duct case per flap angle and traces through them (build/pathlines,
# hours); `trace --grille D --duct D` re-traces finished cases.
# The air's paths through the whole airway, for the film.
pathlines *args:
    uv run minimeater-pathlines {{ args }}

# `--flap 33 --cfm 0.7 --res coarse`, `--sweep` for the operating points, `--dry` to write the case and print its time
# step, `--mesh-only`, `--resume` after a stop.
# One flap LES case (pimpleFoam + WALE; hours, best on a Linux box): build/les/<case>.
les *args:
    uv run minimeater-les {{ args }}

# Spectra, peaks and plots of finished LES cases (build/les/*); `--compare A B` checks that two resolutions agree.
les-post *args:
    uv run minimeater-les-post {{ args }}

# Plane-wave acoustics of the air path: where the flap's shedding could meet a duct resonance.
acoustics *args:
    uv run minimeater-acoustics {{ args }}

# Where the firmware runs the flap and the fan: the actuation path and the hold point per pit temperature.
operating *args:
    uv run minimeater-operating {{ args }}

# Slices through the flow field of finished grille cases: `just mech::cfd::fields CASE [CASE ...]`.
fields *args:
    uv run python -m minimeater.mech.cfd.fields {{ args }}

# What the flap's unsteady force radiates, one JSON line per LES case: `just mech::cfd::whistle build/les/<case>...`.
whistle *args:
    uv run python -m minimeater.mech.cfd.whistle {{ args }}

# The duct's inlet contraction and tail cone as a designed pair of curves: `just mech::cfd::inlet [out.json]`.
inlet *args:
    uv run python -m minimeater.mech.cfd.inlet {{ args }}

# The fan's intake, steady RANS: `just mech::cfd::grille [--grille current|s-louvre] [--gap MM]`.
grille *args:
    uv run python -m minimeater.mech.cfd.grille {{ args }}

# The fan itself with its rotor in a rotating frame (needs Noctua's model, `just vendor::fetch`): `[--cfm Q] [--rpm N]`.
fan *args:
    uv run python -m minimeater.mech.cfd.fan {{ args }}
