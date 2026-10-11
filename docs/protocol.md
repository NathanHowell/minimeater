# MQTT protocol, schema v1

Normative. The implementation lives in `minimeat-core::proto`; its tests check every JSON example here (`tests/test_protocol_doc.py` checks they parse). Rationale is in [ADR 0003](adr/0003-control-protocol.md).

## Conventions

- **Transport:** MQTT 5 to a LAN broker. Clean start, session expiry 0, keepalive 30 s.
- **Device id:** `mm-` plus the low 3 bytes of the WiFi MAC in lowercase hex, e.g. `mm-a1b2c3`.
- **Base topic:** `<prefix>/<id>`. The prefix defaults to `minimeat` and can be set over the USB console. Below, `~` stands for the base topic.
- **Units:**

| Quantity | Unit | Precision |
|---|---|---|
| Temperature | °F | 1 decimal place |
| Airflow | CFM (ft³/min) | 2 decimal places |
| Flap and fan | % of travel or duty | |
| Time | seconds | |

- **Payloads:**
  - Everything the device publishes is a UTF-8 JSON object carrying `"v": 1`.
  - Commands sent to the device are plain text, so Home Assistant number/select/text entities can publish them with no template. The exception is the one JSON-valued setting, `coeffs`.
  - Consumers must ignore unknown fields.
  - Adding fields does not bump `v`. Renaming, removing or changing the meaning of a field does, and the device then republishes discovery.
- **Missing values** are `null`. Examples: an unplugged probe, or the pit temperature when the pit probe is missing.

## Topics

| Topic | Direction | QoS | Retained | When |
|---|---|---|---|---|
| `~/status` | device → | 1 | yes | `online` after connect; `offline` as the Last Will and before a clean disconnect |
| `~/state` | device → | 0 | yes | every control period (10 s), and immediately on a mode or phase change |
| `~/config` | device → | 1 | yes | on connect and after every accepted setting change |
| `~/event` | device → | 1 | no | when something happens (below) |
| `~/set/<setting>` | → device | 1 | **must not be** | change a setting |
| `~/cmd/<command>` | → device | 1 | **must not be** | one-shot actions |
| `homeassistant/device/<id>/config` | device → | 1 | yes | on connect, on `homeassistant/status` = `online`, and when a probe name changes |

The device subscribes to `~/set/#`, `~/cmd/#` and `homeassistant/status`.

**Command handling rules** (ADR 0003, safety):
- A `set` or `cmd` message that arrives with the retain flag set is dropped and answered with `rejected` / `retained`.
- Senders should set a message expiry of 30 s or less.
- Every accepted `set` is followed by a fresh `~/config`, so the retained config is the acknowledgement for clients that only speak MQTT 3 or HA.
- If the message carries an MQTT 5 response topic, the device also publishes the result there, echoing the correlation data:
  - `{"v":1,"ok":true}`
  - or `{"v":1,"ok":false,"error":"<code>","detail":"..."}`
- Every rejection is also published on `~/event` as `rejected`.

Error codes:

| Code | Meaning |
|---|---|
| `bad_payload` | can't parse |
| `out_of_range` | outside the documented limits |
| `unknown` | no such setting, command or probe index |
| `not_allowed` | wrong mode or phase for this command, e.g. autotune while in `off` |
| `retained` | the message had the retain flag set |

## `~/state`

```json
{
  "v": 1,
  "up": 5321,
  "mode": "run",
  "phase": "hold",
  "sp": 225.0,
  "ref": 225.0,
  "pit": 224.6,
  "amb": 64.2,
  "probes": [
    {"t": 224.6, "st": "ok", "alarm": null},
    {"t": 151.3, "st": "ok", "alarm": "hi"},
    {"t": null, "st": "open", "alarm": null},
    {"t": null, "st": "open", "alarm": null}
  ],
  "tc": null,
  "out": {"q": 0.82, "flap": 34.0, "fan": 0.0, "rpm": 0},
  "lid": false,
  "fire": "ok",
  "autotune": null,
  "faults": [],
  "diag": {"rssi": -61, "board": 41.2}
}
```

**Top-level fields:**
- `up`: uptime in seconds.
- `sp`: user setpoint.
- `ref`: shaped reference the controller is currently tracking. It equals `sp` except during `startup` and after a setpoint increase.
- `pit`: the probe selected by `pit_probe`, with offset applied.
- `amb`: on-board outside-air sensor (TMP117).
- `tc`: the thermocouple reading in °F when the MAX31856 is fitted, otherwise `null`.

**`mode`** (user-selected):

| Value | Meaning |
|---|---|
| `off` | flap closed, fan stopped, alarms still active |
| `run` | automatic control |
| `manual` | actuators follow `manual_flap` and `manual_fan` |

**`phase`** (what the controller is doing):

| Value | Meaning |
|---|---|
| `idle` | mode `off` |
| `startup` | light-off or setpoint-increase reference shaping |
| `hold` | at setpoint |
| `lid` | lid open: flap closed, fan stopped |
| `recover` | after the lid closes, replaying `lid_boost` times the pre-lid airflow |
| `manual` | mode `manual` |
| `fault` | no pit reading or a hardware fault: flap closed, fan stopped |

**`probes[n]`** (n = 0..3, in jack order):
- `t`: temperature in °F with offset applied, or `null`.
- `st` (status):

| Value | Meaning |
|---|---|
| `ok` | valid reading |
| `open` | nothing plugged in, or the probe is broken |
| `short` | |
| `range` | reading outside -20 to 700F |
| `disagree` | failed the power-up self-check against ambient |

- `alarm`: `null`, `"lo"` or `"hi"`. It stays latched until the reading moves 5F back inside the limit. It is reported even after `alarm_ack`, which only silences the LED.

**`out`** (actuators):
- `q`: commanded airflow in CFM.
- `flap`: flap position, 0-100 % of travel.
- `fan`: fan duty, 0-100 %, averaged over the duty-cycling period.
- `rpm`: fan tachometer reading.

**Status fields:**
- `lid`: `true` while the controller thinks the lid is open. With `lid_detect` on it works this out from the pit probe alone: a fall of 10F at 0.3F/s or faster opens it, within 20-30 s of the lid going up. It closes when the pit turns back up, or after 15 min regardless. Nothing else an egg does falls that fast. A probe pulled out of the pit looks like a lid until the 15 min run out.
- `fire`: `ok`, `low` (fuel running low), `out`, or `unknown`. `unknown` covers startup and the first hour after a setpoint change, while the estimator settles.
- `autotune`: `{"exciting": bool, "accepted": n, "window": 0.0-1.0}`. The controller learns its model all the time from closed-loop data (`.4.4`); `exciting` is true while the `autotune` command's setpoint dither runs, `accepted` counts fits taken into `model` since boot, and `window` is how full the current 60 min data window is. Autotune never takes the pit away from the controller and has no failure state.
- `faults`: empty, or a list of codes:

| Code | Meaning |
|---|---|
| `board_hot` | board above 80C: the device keeps running and warns |
| `pit_probe` | the selected pit probe isn't `ok` |
| `fan_stall` | duty above 30 % but tach reads 0 |
| `adc` | ADS1220 not responding |
| `ambient` | TMP117 not responding: feedforward falls back to 20C |

- `diag`: diagnostics for HA. `rssi` is in dBm; `board` is the board temperature in °C (ADS1220 internal sensor). Detailed diagnostics go to OTLP, not here.

## `~/config`

The current value of every setting. It is the same set the device stores in flash.

```json
{
  "v": 1,
  "mode": "run",
  "sp": 225.0,
  "pit_probe": 0,
  "controller": "mpc",
  "lid_detect": true,
  "lid_boost": 1.5,
  "manual_flap": 0.0,
  "manual_fan": 0.0,
  "probes": [
    {"name": "Pit", "type": "thermoworks_pro", "coeffs": null, "offset": 0.0, "lo": null, "hi": null},
    {"name": "Brisket", "type": "thermoworks_pro", "coeffs": null, "offset": -0.4, "lo": null, "hi": 203.0},
    {"name": "Probe 3", "type": "thermoworks_pro", "coeffs": null, "offset": 0.0, "lo": null, "hi": null},
    {"name": "Probe 4", "type": "custom", "coeffs": {"a": 7.3431e-4, "b": 2.1574e-4, "c": 9.5157e-8}, "offset": 0.0, "lo": null, "hi": null}
  ],
  "model": [
    {"sp": 225.0, "k": 116.0, "tau": 480.0, "dead": 110.0}
  ]
}
```

`model` is read-only: it is the FOPDT schedule learned from closed-loop data (`k` in F/CFM, `tau` and `dead` in seconds), one entry per 25F scheduling point that has been learned. It persists in flash, so a power cut keeps it. It is empty until the first accepted fit, and the controller uses built-in defaults until then.

## `~/set/<setting>`

| Setting | Payload | Limits | Notes |
|---|---|---|---|
| `mode` | `off` \| `run` \| `manual` | | `heat` is accepted as an alias of `run` for the HA climate entity |
| `sp` | number | 150-325 | outside this is `out_of_range`, never clamped silently |
| `pit_probe` | `0`-`3` \| `tc` | `tc` only if the thermocouple is fitted | |
| `controller` | `mpc` \| `ffpi` | | FF+PI is the fallback controller (control ADR) |
| `lid_detect` | `ON` \| `OFF` | | |
| `lid_boost` | number | 1.0-3.0 | recovery speed against overshoot |
| `manual_flap` | number | 0-100 | takes effect only in `manual` mode |
| `manual_fan` | number | 0-100 | takes effect only in `manual` mode |
| `probe/<n>/name` | text | 1-24 UTF-8 bytes | republishes discovery |
| `probe/<n>/type` | `thermoworks_pro` \| `maverick_et732` \| `generic_b3950` \| `custom` | | presets from `probes.py` |
| `probe/<n>/coeffs` | `{"a":..,"b":..,"c":..}` | the curve must be monotonic and give 20 Ω-50 MΩ over -20 to 700F | Steinhart-Hart. Setting it switches `type` to `custom` |
| `probe/<n>/offset` | number | ±10 | °F, added after conversion |
| `probe/<n>/lo`, `probe/<n>/hi` | number, or `0`/`off` to disable | 0-700 | 5F hysteresis |

## `~/cmd/<command>`

| Command | Payload | Allowed when | Effect |
|---|---|---|---|
| `autotune` | empty | `run` | adds a ±3F square-wave dither (30 min half-period) to the setpoint, so the learner sees enough excitation; it stops by itself once the model at this setpoint is well determined (typically 2-3 h) or after 5 h. The controller stays in charge throughout, and the command cannot fail: poor data is discarded and the model is left as it was. |
| `autotune_stop` | empty | any time | ends the dither early; whatever was learned is kept |
| `model_reset` | `reset` | any time | clears the learned schedule |
| `selfcheck` | empty | any time | compares every `ok` probe against ambient, ±2F |
| `alarm_ack` | empty | any time | silences the LED for the alarms active now |
| `identify` | empty | any time | blinks the LED for 10 s |
| `reboot` | `reboot` | any time | the literal payload guards against stray publishes |
| `ota` | reserved | | `.4.13` |

## `~/event`

The payload's `event_type` names the event, which is the field HA's MQTT `event` entity requires.

```json
{"v": 1, "event_type": "alarm", "probe": 1, "which": "hi", "t": 203.4}
```

```json
{"v": 1, "event_type": "rejected", "topic": "set/sp", "error": "out_of_range", "detail": "400 > 325"}
```

```json
{"v": 1, "event_type": "autotune_done", "sp": 225.0, "k": 116.0, "tau": 480.0, "dead": 110.0, "accepted": 6}
```

| `event_type` | Extra fields |
|---|---|
| `boot` | `reason` (`power_on` \| `watchdog` \| `panic` \| `ota` \| `user`), `sw` |
| `alarm`, `alarm_clear` | `probe`, `which`, `t` |
| `lid_open`, `lid_closed` | `pit` |
| `fire_low`, `fire_out` | `pit`, `q` |
| `setpoint_reached` | `sp` |
| `autotune_done` | `sp`, `k`, `tau`, `dead` (the model at that setpoint now), `accepted` (fits taken during the dither; 0 means the model is unchanged) |
| `selfcheck` | `results`: one per jack, each `{"probe": n, "delta": F}` or `{"probe": n, "st": "open"}` |
| `rejected` | `topic`, `error`, `detail` |
| `fault`, `fault_clear` | `code` |

## Home Assistant discovery

There is one retained device-discovery message, `homeassistant/device/<id>/config` (HA 2024.11+).

**Shared options:**
- `state_topic` is `~/state` and `availability_topic` is `~/status`.
- Every component reads from them through a `value_template`. Settings read `~/config`.

**Components** (the key is the `object_id`; `unique_id` is `<id>_<key>`):

| Key | Platform | Source / command | Notes |
|---|---|---|---|
| `pit` | `climate` | modes `off`/`heat` ↔ `set/mode`; target ↔ `set/sp`; current = `pit`; action = `heating` when `out.q > 0` | `min_temp` 150, `max_temp` 325, `temp_step` 5, `temperature_unit` `F` |
| `pit_temp`, `probe0`..`probe3`, `ambient` | `sensor` | `device_class` temperature, °F, `state_class` measurement | probe entity names follow the probe names |
| `airflow` | `sensor` | `device_class` volume_flow_rate, `ft³/min` | |
| `flap`, `fan` | `sensor` | % | |
| `fan_rpm` | `sensor` | rpm | diagnostic |
| `phase`, `fire` | `sensor` | `device_class` enum | |
| `lid` | `binary_sensor` | `device_class` opening | |
| `problem` | `binary_sensor` | `device_class` problem, on when `faults` is non-empty | |
| `mode`, `controller`, `pit_probe` | `select` | `set/...` | `config` entity category |
| `probe0_type`..`probe3_type` | `select` | `set/probe/<n>/type` | `config` |
| `probe0_name`..`probe3_name` | `text` | `set/probe/<n>/name` | `config` |
| `probe<n>_lo`, `probe<n>_hi`, `probe<n>_offset` | `number` | `set/probe/<n>/...` | `config`; 0 disables lo/hi |
| `lid_detect` | `switch` | `set/lid_detect` | `config` |
| `lid_boost`, `manual_flap`, `manual_fan` | `number` | `set/...` | `config` |
| `autotune`, `autotune_stop`, `selfcheck`, `alarm_ack`, `identify` | `button` | `cmd/...` | |
| `reboot` | `button` | `cmd/reboot`, payload `reboot` | `device_class` restart, diagnostic |
| `events` | `event` | `~/event` | `event_types` from the table above |
| `rssi`, `board_temp`, `uptime` | `sensor` | `diag.*`, `up` | diagnostic |

Excerpt, using HA's abbreviated keys:

```json
{
  "dev": {"ids": "mm-a1b2c3", "name": "minimeat", "mf": "minimeat", "mdl": "minimeat v1", "sw": "0.1.0", "hw": "A"},
  "o": {"name": "minimeat", "sw": "0.1.0", "url": "https://github.com/NathanHowell/minimeat"},
  "avty_t": "minimeat/mm-a1b2c3/status",
  "stat_t": "minimeat/mm-a1b2c3/state",
  "qos": 1,
  "cmps": {
    "pit": {
      "p": "climate", "uniq_id": "mm-a1b2c3_pit", "name": "Pit",
      "modes": ["off", "heat"],
      "mode_cmd_t": "minimeat/mm-a1b2c3/set/mode",
      "mode_cmd_tpl": "{{ 'run' if value == 'heat' else 'off' }}",
      "mode_stat_t": "minimeat/mm-a1b2c3/state",
      "mode_stat_tpl": "{{ 'off' if value_json.mode == 'off' else 'heat' }}",
      "temp_cmd_t": "minimeat/mm-a1b2c3/set/sp",
      "temp_stat_t": "minimeat/mm-a1b2c3/state",
      "temp_stat_tpl": "{{ value_json.sp }}",
      "curr_temp_t": "minimeat/mm-a1b2c3/state",
      "curr_temp_tpl": "{{ value_json.pit }}",
      "act_t": "minimeat/mm-a1b2c3/state",
      "act_tpl": "{{ 'off' if value_json.mode == 'off' else ('heating' if value_json.out.q > 0 else 'idle') }}",
      "min_temp": 150, "max_temp": 325, "temp_step": 5, "temp_unit": "F", "precision": 0.1
    },
    "probe1": {
      "p": "sensor", "uniq_id": "mm-a1b2c3_probe1", "name": "Brisket",
      "dev_cla": "temperature", "unit_of_meas": "°F", "stat_cla": "measurement",
      "val_tpl": "{{ value_json.probes[1].t }}"
    },
    "events": {
      "p": "event", "uniq_id": "mm-a1b2c3_events", "name": "Events",
      "stat_t": "minimeat/mm-a1b2c3/event",
      "evt_typ": ["boot", "alarm", "alarm_clear", "lid_open", "lid_closed", "fire_low", "fire_out",
                  "setpoint_reached", "autotune_done", "selfcheck", "rejected",
                  "fault", "fault_clear"]
    }
  }
}
```

**Sizing:** the full discovery message is about 10-12 KB. The MQTT TX buffer must hold it, or the client must stream the publish body. Everything else fits in 2 KB.
