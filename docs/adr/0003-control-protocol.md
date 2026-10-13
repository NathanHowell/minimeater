# ADR 0003: Control and integration protocol

- Status: accepted (2026-10-11)
- Bead: `minimeat-seq.1.4`
- Schema: [`docs/protocol.md`](../protocol.md) (`.5.1`)

## Context

The controller has to show state (pit, food probes, airflow, alarms) and accept commands (setpoint, mode, probe settings, autotune) from somewhere other than its one button. The user runs Home Assistant. HeaterMeter offered a web UI and an HTTP/JSON API.

The candidates were:

| | MQTT 5 + HA discovery | Matter (rs-matter) | HomeKit HAP | Own HTTP API + web UI |
|---|---|---|---|---|
| Rust `no_std` client on embassy | `rust-mqtt` 0.6 (MQTT 5, `embedded-io-async`, no alloc) or `minimq` 0.13 | `rs-matter` 0.4 / `rs-matter-embassy`; about 600 KB flash for a minimal image | no maintained `no_std` implementation | `picoserve` 0.20 |
| Expresses everything we have (4 probes with roles and alarms, airflow, flap, fan, lid, autotune, self-check) | yes, free-form | partly: no BBQ device type (see below) | partly | yes |
| Home Assistant | first-class: device discovery, climate/number/select/button/event entities | yes, through the Matter integration | yes, through the HomeKit Controller | needs a custom integration |
| Apple Home / Google | no | yes | Apple only | no |
| Flash/RAM cost | tens of KB | hundreds of KB plus commissioning (BLE or SoftAP) | n/a | tens of KB plus UI assets |

## Decision

**v1 speaks MQTT 5 to a LAN broker and announces itself with Home Assistant device discovery.** Matter is a stretch goal (`.5.4`). HomeKit and a built-in web UI are out.

- **Client crate: `rust-mqtt`.** It runs over embassy-net TCP with no heap. It implements MQTT 5 only, which Mosquitto (including the HA add-on) supports. Reconnect, keepalive and retry are left to us, which suits embassy: a single `mqtt` task owns the connection and backs off exponentially from 1 s to 60 s. `minimq` is the fallback.
- **Why MQTT 5 features are used:**
  - Request/response: if a command carries a response topic, the device replies with the result. HA ignores this, but a CLI or test harness gets per-command acknowledgement.
  - Message expiry on commands, so a command queued at the broker during an outage can't be applied minutes late.
  - A session expiry of 0, so every connect starts clean and stale subscriptions don't pile up.
- **Wire unit is °F.** The project specifies everything in °F, and HA converts `device_class: temperature` entities for metric users. With °C on the wire, a 225F setpoint becomes 107.22C and HA's number step rounding makes it drift.
- **HA integration uses device discovery** (one retained `homeassistant/device/<id>/config` message listing every entity, HA 2024.11+), not one config message per entity. It is atomic and smaller. The pit is a `climate` entity with modes `off`/`heat`, so the stock thermostat card works. Everything else is a sensor, number, select, text, button, binary sensor or event entity.
- **Schema owner:** `docs/protocol.md` is normative. The firmware implementation lives in one module, `minimeater-core::proto` (topics, payload types, HA discovery tables). Host tests serialise every payload type against examples in the doc, so the doc and code can't drift silently. Any change to topics or payloads updates both in the same commit, and a breaking change bumps the schema version (`v`).

### Safety does not depend on the network

The device controls a fire, and on a plain-TCP LAN broker anyone on the network can publish commands. The rules below hold no matter what arrives over MQTT:

1. **The firmware enforces the envelope, not the client.**
   - The setpoint is clamped to the 150-325F scope; anything outside it is rejected, and the rejection is published as an event.
   - Probe offsets and alarm limits are range-checked.
   - Manual mode cannot exceed the actuator limits.
2. **Losing the network or broker changes nothing.** The cook continues at the last setpoint, with local LED alarms, and the device reconnects in the background. It never turns off, and never jumps to a default setpoint, because of a comms fault. Settings persist in flash (`.4.9`), so a reboot resumes the cook.
3. **Retained commands are ignored.** A command delivered with the retain flag set (one somebody accidentally published retained) is dropped. Otherwise a reboot would re-apply an old setpoint or mode. Device state and discovery are retained; commands never are.
4. **Authentication:** the broker username and password are stored in flash and provisioned over USB (`.4.10`). TLS (`esp-mbedtls`, on port 8883) is deferred (`.5.5`). Most HA Mosquitto installs are LAN-only plaintext, and items 1-3 bound the worst case.

### Telemetry is separate

High-rate and diagnostic data (per-conversion probe readings, MPC internals, solver timing, heap and stack watermarks) goes to OTLP/HTTP (`.4.11`), not MQTT. MQTT carries what a person or an automation acts on, at the 10 s control period. That keeps HA's recorder small and lets the OTel side change freely without a schema version bump.

### Provisioning

WiFi credentials, broker address and broker credentials are set over the USB-C port's serial console (USB-Serial-JTAG), since the box already has the cable. This avoids a SoftAP/captive portal in v1. BLE provisioning arrives with Matter commissioning if that stretch goal lands.

### Matter later (`.5.4`)

- **Mapping:**
  - The pit becomes a Thermostat endpoint, heating only (`SystemMode` Off/Heat, `OccupiedHeatingSetpoint`).
  - Each food probe becomes a Temperature Sensor endpoint.
  - Matter 1.3's Oven/Cooktop device types are closer in meaning, but controllers barely support them.
- **Main risk:**
  - The Thermostat cluster allows a heating setpoint up to 327.67C, so 325F (162.8C) is representable.
  - Controller UIs (Apple Home in particular) clamp heating setpoints to a room range of about 30-32C, so the setpoint may only be readable from those ecosystems, not settable.
  - Prototype this before investing.
- **Cost:** `rs-matter-embassy` adds about 600 KB of flash. The 8 MB module was chosen with two Matter-sized OTA slots in mind (ADR 0001).
- **Coexistence:** Matter runs alongside MQTT, not instead of it.

## Consequences

- `.5.1` writes `docs/protocol.md` against these rules. `.5.2` (client) and `.5.3` (discovery) implement it in `minimeater-core::proto` plus a firmware `mqtt` task.
- A device without a broker is still fully usable: button, LED, and the USB console for status and settings.
- There is no phone UI outside Home Assistant until Matter lands. That is accepted for a DIY build whose user already runs HA.
