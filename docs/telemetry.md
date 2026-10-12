# Metrics export (OpenTelemetry)

The device POSTs its state as OTLP/HTTP protobuf to a collector's `/v1/metrics` (`minimeat-app::otlp`, `.4.11`). It is set up with the `otlp` setting (host, port, interval in seconds, 5-3600; 4318 is the collector's OTLP/HTTP default). It is a stored setting like WiFi and the broker, not an MQTT one. Nothing is sent until a collector is set and the clock is known (SNTP): a metric without a real time is worse than a gap.

Resource attributes: `service.name` = `minimeat`, `service.instance.id` = the device id, `service.version`. Instrumentation scope: `minimeat-fw`. Every metric is a gauge. In Prometheus, `service.instance.id` becomes `instance` and the unit becomes a suffix (`minimeat_pit_setpoint_degF`).

| Metric | Unit | Attributes | Notes |
|---|---|---|---|
| `minimeat.probe.temperature` | degF | `probe`, `name` | one point per probe with a reading |
| `minimeat.pit.temperature`, `.setpoint`, `.reference` | degF | | |
| `minimeat.ambient.temperature` | degF | | |
| `minimeat.airflow` | {cfm} | | the commanded airflow; the controller is model-predictive, so there are no P/I/D terms |
| `minimeat.flap.position`, `minimeat.fan.duty` | % | | |
| `minimeat.fan.speed` | {rpm} | | |
| `minimeat.lid.open` | 1 | | |
| `minimeat.phase`, `minimeat.fire` | 1 | `phase`, `state` | 1 for the current value |
| `minimeat.food.eta` | s | `probe`, `name` | |
| `minimeat.food.rate` | degF/h | `probe`, `name` | |
| `minimeat.fault.active` | 1 | `code` | one point per fault code, 0 or 1 |
| `minimeat.wifi.rssi` | dBm | | |
| `minimeat.uptime` | s | | |
| `minimeat.board.temperature` | Cel | | the ADC's die |
| `minimeat.heap.free` | By | | |

Try it: `just otlp-interop` starts a collector from `tools/otel-collector.yaml` (OTLP/HTTP in on 4318, Prometheus text out on 8889) in a container, runs the exporter against it and checks the scrape. Point a Prometheus at port 8889, and Grafana at that, for dashboards.
