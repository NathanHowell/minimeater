# The Python plant, airflow, controller and probe models (src/minimeater/model) and the reports they write.
# The generated docs/modeling/*.md are only ever rewritten by these.
set working-directory := ".."

# Airflow and thermal report: docs/modeling/bge-large.md.
report *args:
    uv run minimeater-model {{ args }}

# Controller report: docs/modeling/control.md.
control *args:
    uv run minimeater-control {{ args }}

# Probe error budget: docs/modeling/probes.md.
probes *args:
    uv run minimeater-probes {{ args }}

# The probe front end's ngspice checks: docs/sim/probe-frontend.md.
sim *args:
    uv run minimeater-sim {{ args }}

# Parity fixtures for the Rust port from the Python reference: fw/minimeater-{core,sim}/tests/fixtures.
fixtures *args:
    uv run minimeater-fixtures {{ args }}

# The Rust thermistor test's REFERENCE table from the Python probes (paste into fw/minimeater-core/src/thermistor.rs).
thermistor-table:
    uv run minimeater-thermistor-table

# Every report and the probe front end's checks.
all: report control probes sim
