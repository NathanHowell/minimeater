# Run a scenario on the host digital twin, e.g. `just twin --scenario kamado-brisket --speed 60x`.
twin *args:
    cd fw && cargo run --release -p minimeater-twin -- {{args}}

# The scenario suite on the twin (bounds + baselines), then a PNG of each run in fw/target/tmp/scenarios.
scenarios:
    cd fw && cargo test --release -p minimeater-twin --test suite -- --nocapture
    uv run python -I scripts/plot_scenarios.py

# Rewrite the suite's baselines from this run, after a change that is meant to move them.
scenarios-update:
    cd fw && UPDATE_BASELINES=1 cargo test --release -p minimeater-twin --test suite -- --nocapture

# The MQTT client against Mosquitto in a container (Apple's `container` CLI) and rumqttc.
mqtt-interop:
    -container rm -f mm-mosq >/dev/null 2>&1
    container run -d --name mm-mosq docker.io/library/eclipse-mosquitto:2 sh -c 'printf "listener 1883\nallow_anonymous true\n" > /tmp/m.conf; exec mosquitto -c /tmp/m.conf'
    sleep 2
    ip=$(container ls | awk '/mm-mosq/ {split($6, a, "/"); print a[1]}'); cd fw/mqtt-interop && MQTT_BROKER=$ip:1883 cargo test --release --test broker -- --ignored --nocapture; r=$?; container rm -f mm-mosq >/dev/null 2>&1; exit $r

# The MQTT client over TLS with a client certificate (ADR 0006) against a small TLS 1.3 MQTT peer that requires one. Needs openssl and uv.
tls-interop:
    cd fw/mqtt-interop && cargo test --release --test tls

# The same against Mosquitto in a container (MINIMEATER_CONTAINER, default podman): client certificate required, its name as the user, an ACL by name.
mosquitto-tls:
    cd fw/mqtt-interop && cargo test --release --test tls -- --ignored --nocapture

# Has Home Assistant's own MQTT discovery code validate the device-discovery message the firmware publishes.
ha-discovery-check:
    mkdir -p fw/target/tmp
    cd fw && DISCOVERY_OUT=$PWD/target/tmp/discovery.json cargo test -p minimeater-app --release --test proto discovery_matches
    uv run --no-project --python 3.14 --with homeassistant --with paho-mqtt python -I scripts/check_ha_discovery.py fw/target/tmp/discovery.json

# The OTLP exporter against a real OpenTelemetry collector (tools/otel-collector.yaml) in a container.
otlp-interop:
    -container rm -f mm-otel >/dev/null 2>&1
    container run -d --name mm-otel -v $PWD/tools/otel-collector.yaml:/etc/otelcol-contrib/config.yaml:ro docker.io/otel/opentelemetry-collector-contrib:latest
    sleep 4
    ip=$(container ls | awk '/mm-otel/ {split($6, a, "/"); print a[1]}'); cd fw/mqtt-interop && OTLP_COLLECTOR=$ip cargo test --release --test otlp -- --ignored --nocapture; r=$?; container rm -f mm-otel >/dev/null 2>&1; exit $r

# Mosquitto and an OpenTelemetry collector in containers, for `just twin-stack`.
stack-up:
    -container rm -f mm-mosq mm-otel >/dev/null 2>&1
    container run -d --name mm-mosq -v $PWD/tools/mosquitto.conf:/mosquitto/config/mosquitto.conf:ro docker.io/library/eclipse-mosquitto:2
    container run -d --name mm-otel -v $PWD/tools/otel-collector.yaml:/etc/otelcol-contrib/config.yaml:ro docker.io/otel/opentelemetry-collector-contrib:latest
    sleep 3
    @container ls | awk '/mm-mosq|mm-otel/ {split($6, a, "/"); print $1, a[1]}'

stack-down:
    -container rm -f mm-mosq mm-otel >/dev/null 2>&1

# The twin against the stack from `just stack-up`, e.g. `just twin-stack --scenario kamado-brisket --speed 600`.
twin-stack *args:
    #!/usr/bin/env bash
    set -euo pipefail
    mq=$(container ls | awk '/mm-mosq/ {split($6, a, "/"); print a[1]}')
    ot=$(container ls | awk '/mm-otel/ {split($6, a, "/"); print a[1]}')
    echo "broker $mq:1883, collector $ot:4318 (Prometheus text on $ot:8889/metrics)"
    cd fw && cargo run --release -p minimeater-twin -- --mqtt $mq:1883 --otlp $ot:4318 {{args}}

# What the device publishes on the stack's broker, for 30 s.
stack-watch:
    uv run --no-project --python 3.14 --with paho-mqtt python -I scripts/mqtt_watch.py $(container ls | awk '/mm-mosq/ {split($6, a, "/"); print a[1]}') 30

# Home Assistant itself (container) against the twin: onboards, adds MQTT, and checks the entities and a setpoint change.
ha-live-check:
    #!/usr/bin/env bash
    set -euo pipefail
    just stack-up
    container rm -f mm-ha >/dev/null 2>&1 || true
    rm -rf fw/target/tmp/ha-config && mkdir -p fw/target/tmp/ha-config
    container run -d --name mm-ha --memory 2G -v $PWD/fw/target/tmp/ha-config:/config ghcr.io/home-assistant/home-assistant:stable >/dev/null
    ip() { container ls | awk -v n="$1" '$1 == n {split($6, a, "/"); print a[1]}'; }
    mq=$(ip mm-mosq); ot=$(ip mm-otel); ha=$(ip mm-ha)
    for i in $(seq 1 30); do curl -sf -m 3 http://$ha:8123/api/onboarding >/dev/null && break; sleep 5; done
    (cd fw && cargo run --release -p minimeater-twin -- --scenario kamado-hold --speed 120 --mqtt $mq:1883 --otlp $ot:4318 >/tmp/mm_twin.log 2>&1 &)
    sleep 8
    r=0; uv run --no-project python -I scripts/ha_live_check.py $ha $mq || r=$?
    pkill -f minimeater-twin || true
    container rm -f mm-ha >/dev/null 2>&1 || true
    just stack-down
    exit $r

# Everything CI runs, on a clean checkout: Python models and CAD, firmware on the host and for the target, KiCad checks.
ci: ci-py ci-fw ci-hw

# Python: model and doc tests, the CAD tests (parts build, fit, printability), the STEP/STL export.
ci-py:
    uv run pytest -q
    uv run mech-export build/mech

# Firmware: format, clippy, host tests, the twin scenario suite, the ESP32-C6 build. The container interop tests are separate (`just mqtt-interop`).
ci-fw:
    cd fw && cargo fmt --all -- --check
    cd fw && cargo clippy --workspace --exclude minimeater-fw --all-targets -- -D warnings
    cd fw && cargo test --workspace --exclude minimeater-fw --release
    cd fw && cargo fw-build

# KiCad: ERC and DRC on the project. kicad-cli from the PATH, else the kicad container under podman (MINIMEATER_KICAD_IMAGE).
ci-hw:
    #!/usr/bin/env bash
    set -euo pipefail
    pro=$(ls hw/*.kicad_pro 2>/dev/null | head -1 || true)
    if [ -z "$pro" ]; then echo "hw: no KiCad project yet, skipped"; exit 0; fi
    base=${pro%.kicad_pro}
    mkdir -p build/hw
    if command -v kicad-cli >/dev/null; then
        kc() { kicad-cli "$@"; }
    else
        image=${MINIMEATER_KICAD_IMAGE:-docker.io/kicad/kicad:10.0}
        # the container has KiCad's stock libraries but no global library tables: seed them from its templates
        kc() {
            podman run --rm --userns=keep-id -v "$PWD":/work:Z -w /work -e HOME=/tmp "$image" sh -c \
                'd=/tmp/.config/kicad/10.0; mkdir -p $d; cp /usr/share/kicad/template/sym-lib-table /usr/share/kicad/template/fp-lib-table $d/; exec kicad-cli "$@"' kicad-cli "$@"
        }
    fi
    kc sch erc --exit-code-violations --output build/hw/erc.rpt "$base.kicad_sch"
    # errors only: the stock footprints differ between KiCad versions (lib_footprint_mismatch warnings), and the silk warnings are known
    kc pcb drc --severity-error --exit-code-violations --output build/hw/drc.rpt "$base.kicad_pcb"

# One duct CFD case (OpenFOAM in a container: `container` CLI, opencfd/openfoam-run arm64), e.g. `just cfd open` or
# `just cfd flap-45 --cfm 3`; the case and its results.json go to build/cfd/<case>.
cfd *args:
    uv run minimeater-cfd {{args}}

# One flap LES case (pimpleFoam + WALE; hours: a Linux box, docs/cfd.md): `just les --flap 33 --cfm 0.7 --res coarse`, `--sweep` for the
# list of operating points, `--dry` to write the case and print its time step, `--mesh-only` for the cell count, `--resume` after a stop.
# The case goes to build/les/<variant>-<deg>deg-<cfm>cfm-<res>.
les *args:
    uv run minimeater-les {{args}}

# The spectra, peaks and plot of finished LES cases (build/les/*); `just les-post --compare A B` checks that two resolutions agree.
les-post *args:
    uv run minimeater-les-post {{args}}
