# A local Mosquitto and OpenTelemetry collector in containers (Apple's `container` CLI), for the twin to cook against
# (`just fw::twin::stack`).
set working-directory := ".."

# Start Mosquitto (tools/mosquitto.conf) and the collector (tools/otel-collector.yaml); prints their addresses.
up:
    -container rm -f mm-mosq mm-otel >/dev/null 2>&1
    container run -d --name mm-mosq -v $PWD/tools/mosquitto.conf:/mosquitto/config/mosquitto.conf:ro docker.io/library/eclipse-mosquitto:2
    container run -d --name mm-otel -v $PWD/tools/otel-collector.yaml:/etc/otelcol-contrib/config.yaml:ro docker.io/otel/opentelemetry-collector-contrib:latest
    sleep 3
    @container ls | awk '/mm-mosq|mm-otel/ {split($6, a, "/"); print $1, a[1]}'

# Stop and remove both.
down:
    -container rm -f mm-mosq mm-otel >/dev/null 2>&1

# What the device publishes on the stack's broker, for `seconds`.
watch seconds="30":
    uv run --no-project --python 3.14 --with paho-mqtt python -I scripts/mqtt_watch.py $(container ls | awk '/mm-mosq/ {split($6, a, "/"); print a[1]}') {{ seconds }}
