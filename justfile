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
