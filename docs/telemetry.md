# Metrics export (OpenTelemetry)

The device POSTs its state as OTLP/HTTP protobuf to a collector's `/v1/metrics` (`minimeater-app::otlp`, `.4.11`). It is set up with the `otlp` setting (host, port, interval in seconds, 5-3600; 4318 is the collector's OTLP/HTTP default). It is a stored setting like WiFi and the broker, not an MQTT one. Nothing is sent until a collector is set and the clock is known (SNTP): a metric without a real time is worse than a gap.

Resource attributes: `service.name` = `minimeater`, `service.instance.id` = the device id, `service.version`. Instrumentation scope: `minimeater-fw`. Every metric is a gauge. In Prometheus, `service.instance.id` becomes `instance` and the unit becomes a suffix (`minimeater_pit_setpoint_degF`).

| Metric | Unit | Attributes | Notes |
|---|---|---|---|
| `minimeater.probe.temperature` | degF | `probe`, `name` | one point per probe with a reading |
| `minimeater.pit.temperature`, `.setpoint`, `.reference` | degF | | |
| `minimeater.ambient.temperature` | degF | | |
| `minimeater.airflow` | {cfm} | | the commanded airflow; the controller is model-predictive, so there are no P/I/D terms |
| `minimeater.flap.position`, `minimeater.fan.duty` | % | | |
| `minimeater.fan.speed` | {rpm} | | |
| `minimeater.servo.current` | mA | `stat` | the servo's last move: `peak` and `mean` (`out.servo_ma`, docs/protocol.md) |
| `minimeater.lid.open` | 1 | | |
| `minimeater.phase`, `minimeater.fire` | 1 | `phase`, `state` | 1 for the current value |
| `minimeater.food.eta` | s | `probe`, `name` | |
| `minimeater.food.rate` | degF/h | `probe`, `name` | |
| `minimeater.fault.active` | 1 | `code` | one point per fault code, 0 or 1 |
| `minimeater.wifi.rssi` | dBm | | |
| `minimeater.uptime` | s | | |
| `minimeater.board.temperature` | Cel | | the ADC's die |
| `minimeater.heap.free` | By | | |

Try it: `just fw::interop::otlp` starts a collector from `tools/otel-collector.yaml` (OTLP/HTTP in on 4318, Prometheus text out on 8889) in a container, runs the exporter against it and checks the scrape. Point a Prometheus at port 8889, and Grafana at that, for dashboards.
