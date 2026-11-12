# Python: the models, the enclosure and the doc checks (tests/).
set working-directory := ".."

# The test suite, e.g. `just py::test tests/test_mech.py -k explode`.
test *args:
    uv run pytest {{ args }}

# What CI runs: the suite, then the enclosure's STEP/STL export.
ci:
    uv run pytest -q
    uv run mech-export build/mech
