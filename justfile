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

# The MQTT client against Mosquitto in a container (Apple's `container` CLI) and rumqttc.
mqtt-interop:
    -container rm -f mm-mosq >/dev/null 2>&1
    container run -d --name mm-mosq docker.io/library/eclipse-mosquitto:2 sh -c 'printf "listener 1883\nallow_anonymous true\n" > /tmp/m.conf; exec mosquitto -c /tmp/m.conf'
    sleep 2
    ip=$(container ls | awk '/mm-mosq/ {split($6, a, "/"); print a[1]}'); cd fw/mqtt-interop && MQTT_BROKER=$ip:1883 cargo test --release -- --ignored --nocapture; r=$?; container rm -f mm-mosq >/dev/null 2>&1; exit $r

# Has Home Assistant's own MQTT discovery code validate the device-discovery message the firmware publishes.
ha-discovery-check:
    mkdir -p fw/target/tmp
    cd fw && DISCOVERY_OUT=$PWD/target/tmp/discovery.json cargo test -p minimeat-app --release --test proto discovery_matches
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
    cd fw && cargo run --release -p minimeat-twin -- --mqtt $mq:1883 --otlp $ot:4318 {{args}}

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
    (cd fw && cargo run --release -p minimeat-twin -- --scenario kamado-hold --speed 120 --mqtt $mq:1883 --otlp $ot:4318 >/tmp/mm_twin.log 2>&1 &)
    sleep 8
    r=0; uv run --no-project python -I scripts/ha_live_check.py $ha $mq || r=$?
    pkill -f minimeat-twin || true
    container rm -f mm-ha >/dev/null 2>&1 || true
    just stack-down
    exit $r
