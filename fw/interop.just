# The firmware's clients against real services (runs in fw/). The container ones use Apple's `container` CLI, except
# mosquitto-tls (MINIMEATER_CONTAINER, default podman).

# The MQTT client against Mosquitto in a container, and against rumqttc.
mqtt:
    -container rm -f mm-mosq >/dev/null 2>&1
    container run -d --name mm-mosq docker.io/library/eclipse-mosquitto:2 sh -c 'printf "listener 1883\nallow_anonymous true\n" > /tmp/m.conf; exec mosquitto -c /tmp/m.conf'
    sleep 2
    ip=$(container ls | awk '/mm-mosq/ {split($6, a, "/"); print a[1]}'); cd mqtt-interop && MQTT_BROKER=$ip:1883 cargo test --release --test broker -- --ignored --nocapture; r=$?; container rm -f mm-mosq >/dev/null 2>&1; exit $r

# The MQTT client over TLS 1.3 with a client certificate (ADR 0006) against a small Python peer that requires one (needs openssl and uv).
tls:
    cd mqtt-interop && cargo test --release --test tls

# The same against Mosquitto in a container: client certificate required, its name as the user, an ACL by name.
mosquitto-tls:
    cd mqtt-interop && cargo test --release --test tls -- --ignored --nocapture

# The OTLP exporter against a real OpenTelemetry collector (tools/otel-collector.yaml) in a container.
otlp:
    -container rm -f mm-otel >/dev/null 2>&1
    container run -d --name mm-otel -v $PWD/../tools/otel-collector.yaml:/etc/otelcol-contrib/config.yaml:ro docker.io/otel/opentelemetry-collector-contrib:latest
    sleep 4
    ip=$(container ls | awk '/mm-otel/ {split($6, a, "/"); print a[1]}'); cd mqtt-interop && OTLP_COLLECTOR=$ip cargo test --release --test otlp -- --ignored --nocapture; r=$?; container rm -f mm-otel >/dev/null 2>&1; exit $r

# Home Assistant's own MQTT discovery code validates the device-discovery message the firmware publishes.
ha-discovery:
    mkdir -p target/tmp
    DISCOVERY_OUT=$PWD/target/tmp/discovery.json cargo test -p minimeater-app --release --test proto discovery_matches
    cd .. && uv run --no-project --python 3.14 --with homeassistant --with paho-mqtt python -I scripts/check_ha_discovery.py fw/target/tmp/discovery.json

# A real Home Assistant (container) against the twin: onboards, adds MQTT, checks the entities and a setpoint change.
ha-live:
    #!/usr/bin/env bash
    set -euo pipefail
    just -f ../justfile stack::up
    container rm -f mm-ha >/dev/null 2>&1 || true
    rm -rf target/tmp/ha-config && mkdir -p target/tmp/ha-config
    container run -d --name mm-ha --memory 2G -v $PWD/target/tmp/ha-config:/config ghcr.io/home-assistant/home-assistant:stable >/dev/null
    ip() { container ls | awk -v n="$1" '$1 == n {split($6, a, "/"); print a[1]}'; }
    mq=$(ip mm-mosq); ot=$(ip mm-otel); ha=$(ip mm-ha)
    for i in $(seq 1 30); do curl -sf -m 3 http://$ha:8123/api/onboarding >/dev/null && break; sleep 5; done
    (cargo run --release -p minimeater-twin -- --scenario kamado-hold --speed 120 --mqtt $mq:1883 --otlp $ot:4318 >target/tmp/twin-ha.log 2>&1 &)
    sleep 8
    r=0; (cd .. && uv run --no-project python -I scripts/ha_live_check.py $ha $mq) || r=$?
    pkill -f minimeater-twin || true
    container rm -f mm-ha >/dev/null 2>&1 || true
    just -f ../justfile stack::down
    exit $r
