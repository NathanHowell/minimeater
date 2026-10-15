# MQTT protocol, schema v1

Normative. The implementation lives in `minimeater-app::proto`; its tests (`fw/minimeater-app/tests/proto.rs`) check every JSON example here, and `tests/test_protocol_doc.py` checks they parse. Rationale is in [ADR 0003](adr/0003-control-protocol.md).

## Conventions

- **Transport:** MQTT 5 to a LAN broker. Clean start, session expiry 0, keepalive 30 s.
- **Device id:** `mm-` plus the low 3 bytes of the WiFi MAC in lowercase hex, e.g. `mm-a1b2c3`.
- **Base topic:** `<prefix>/<id>`. The prefix defaults to `minimeater` and can be set over the USB console. Below, `~` stands for the base topic.
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
    {"t": 224.6, "st": "ok", "alarm": null, "food": null},
    {"t": 158.3, "st": "ok", "alarm": null, "food": {"rate": 1.4, "eta": 21600, "stall": "stall"}},
    {"t": null, "st": "open", "alarm": null, "food": null},
    {"t": null, "st": "open", "alarm": null, "food": null}
  ],
  "tc": null,
  "out": {"q": 0.82, "flap": 34.0, "fan": 0.0, "rpm": 0},
  "lid": false,
  "fire": "ok",
  "autotune": {"exciting": false, "accepted": 0, "window": 0.35},
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
- `tc`: the thermocouple reading in °F (no thermocouple input in this revision, ADR 0002 amendment), always `null`.

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
| `fault` | the probe ADC is not answering |

- `alarm`: `null`, `"lo"` or `"hi"`. It stays latched until the reading moves 5F back inside the limit. It is reported even after `alarm_ack`, which only silences the LED.
- `food`: the food estimator (`.4.22`), or `null` for the pit probe and a probe without a reading. It is learned from the probe's own curve; the probe's `cut` only sets priors.
  - `rate`: core rate of rise in F/h over the last 20 min, or `null` for the first 20 min.
  - `eta`: seconds until the probe reaches its target, or `null`. The target is `hi` if set, else the cut's default (brisket and pork shoulder 203F, ribs 195F, poultry 165F). It is `null` without a target, or when the pit is not at least 2F above it. A stalling cut gets its typical stall length added until the stall has happened. During a stall, the ETA is the finish if the stall broke now. After it, the ETA assumes the heating rate recovers over about 3 h. On the plant model that is within 0.5 h of the real finish from the end of the stall.
  - `stall`: `none`, `stall`, or `done`. A stall is the core rising less than 5F/h above 120F while it is still well short of the pit, for 10 min. It ends when the rate is back above 5F/h for 10 min. There is one stall per cook. Lid openings are left out of the rate. A reading 20F below the last one starts the estimate over (a new piece of meat).

**`out`** (actuators):
- `q`: commanded airflow in CFM.
- `flap`: flap position, 0-100 % of travel.
- `fan`: fan duty, 0-100 %, averaged over the duty-cycling period.
- `rpm`: fan tachometer reading.

**Status fields:**
- `lid`: `true` while the controller thinks the lid is open. With `lid_detect` on it works this out from the pit probe alone: a fall of 10F at 0.3F/s or faster opens it, within 20-30 s of the lid going up. It closes when the pit turns back up, or after 15 min regardless. Nothing else an egg does falls that fast. A probe pulled out of the pit looks like a lid until the 15 min run out.
- `fire`: `ok`, `low`, `out`, or `unknown`. Values:
  - `out`: the pit has kept falling for 9 min, 15F or more under setpoint, while the controller gives it clearly more air than the setpoint needs. The fire is out or nearly out. A cold load or a front also drops the pit, but extra air catches it within minutes.
  - `low`: the controller has been at full air for 10 min and the pit is still 10F or more under setpoint. The fuel is running low, or the egg can't reach this temperature in this weather.
  - `unknown`: during light-off, a setpoint increase, or while the lid is open or recovering, when being under setpoint is expected. `out` is still reported during these.
  - `low` and `out` stay latched until the pit has been back within 5F of setpoint, below full air, for 5 min.
- `autotune`: `null` unless `mode` is `run`, else `{"exciting": bool, "accepted": n, "window": 0.0-1.0}`. The controller learns its model all the time from closed-loop data (`.4.4`); `exciting` is true while the `autotune` command's setpoint dither runs, `accepted` counts fits taken into `model` since boot, and `window` is how full the current 60 min data window is. Autotune never takes the pit away from the controller and has no failure state.
- `faults`: empty, or a list of codes:

| Code | Meaning |
|---|---|
| `board_hot` | board above 80C (the ADC die or the ambient sensor): the device keeps running and warns |
| `pit_probe` | the selected pit probe has not been `ok` for two control periods (10-20 s): the flap closes and the fan stops until it is back |
| `fan_stall` | duty above 30 % but tach reads 0 |
| `adc` | ADS122C04 not responding |
| `ambient` | TMP117 not responding: feedforward falls back to 20C |
| `load` | a load switch is limiting its current (stalled servo, jammed fan, short): the fan and servo are cut and stay off until reboot; the fault line is shared, so it does not say which |
| `supply` | the USB-C source advertises less than 1.5 A (or nothing): the fan and servo are off, probes and alarms keep running |

- `diag`: diagnostics for HA. `rssi` is in dBm; `board` is the board temperature in °C (ADS122C04 internal sensor). Detailed diagnostics go to OTLP, not here.

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
    {"name": "Pit", "type": "thermoworks_pro", "coeffs": null, "offset": 0.0, "lo": null, "hi": null, "cut": "other", "stall": "notify", "bump": 25.0},
    {"name": "Brisket", "type": "thermoworks_pro", "coeffs": null, "offset": -0.4, "lo": null, "hi": 203.0, "cut": "brisket", "stall": "bump", "bump": 25.0},
    {"name": "Probe 3", "type": "thermoworks_pro", "coeffs": null, "offset": 0.0, "lo": null, "hi": null, "cut": "other", "stall": "notify", "bump": 25.0},
    {"name": "Probe 4", "type": "custom", "coeffs": {"a": 7.3431e-4, "b": 2.1574e-4, "c": 9.5157e-8}, "offset": 0.0, "lo": null, "hi": null, "cut": "other", "stall": "notify", "bump": 25.0}
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
| `probe/<n>/type` | `thermoworks_pro` \| `maverick_et732` \| `generic_b3950` \| `custom` | | presets from `probes.py`. `custom` is accepted only for a probe that already has coefficients (`not_allowed` otherwise) |
| `probe/<n>/coeffs` | `{"a":..,"b":..,"c":..}` | the curve must be monotonic and give 20 Ω-50 MΩ over -20 to 700F | Steinhart-Hart. Setting it switches `type` to `custom` |
| `probe/<n>/offset` | number | ±10 | °F, added after conversion |
| `probe/<n>/lo`, `probe/<n>/hi` | number, or `0`/`off` to disable | 0-700 | 5F hysteresis; `hi` is also the food target for `eta` |
| `probe/<n>/cut` | `other` \| `brisket` \| `pork_shoulder` \| `ribs` \| `poultry` | | priors and default target for the food estimator |
| `probe/<n>/stall` | `notify` \| `wrap` \| `bump` | | what this probe's stall does (see [Stall policy](#stall-policy)) |
| `probe/<n>/bump` | number | 5-50 | °F the `bump` policy adds to the setpoint |

### Stall policy

`stall_start` and `stall_end` are raised whatever the policy. `notify` (the default) does nothing more. `wrap` adds a `wrap_reminder` event at the stall's start. `bump` raises the setpoint by the probe's `bump` (capped at 325F) while the stall lasts: it is an ordinary setpoint change, so the approach is shaped from where the pit is and does not overshoot, and `sp` in the state shows the raised value. `stall_bump` is raised when the setpoint moves, and `stall_restore` when it goes back to what it was, as the stall breaks. Setting `sp` yourself while bumped keeps your value and drops the restore. The pit probe is never a stall probe. If the device restarts mid-stall it starts the estimate over, so a bump with no stall in progress is put back after 15 min (and made again if the stall is found again). The setpoint is also put back if the mode leaves `run` or the probe loses its reading for 15 min.

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
| `stall_start`, `stall_end` | `probe`, `t` |
| `wrap_reminder` | `probe`, `t` |
| `stall_bump` | `probe`, `sp` (the new setpoint) |
| `stall_restore` | `sp` (the setpoint it went back to) |
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
| `probe0_eta`..`probe3_eta` | `sensor` | `probes[n].food.eta` | `device_class` duration, s |
| `probe0_stall`..`probe3_stall` | `sensor` | `probes[n].food.stall` | `device_class` enum |
| `probe0_type`..`probe3_type`, `probe0_cut`..`probe3_cut`, `probe0_on_stall`..`probe3_on_stall` | `select` | `set/probe/<n>/type`, `.../cut`, `.../stall` | `config` |
| `probe0_name`..`probe3_name` | `text` | `set/probe/<n>/name` | `config` |
| `probe<n>_lo`, `probe<n>_hi`, `probe<n>_offset`, `probe<n>_bump` | `number` | `set/probe/<n>/...` | `config`; 0 disables lo/hi |
| `lid_detect` | `switch` | `set/lid_detect` | `config` |
| `lid_boost`, `manual_flap`, `manual_fan` | `number` | `set/...` | `config` |
| `autotune`, `autotune_stop`, `selfcheck`, `alarm_ack`, `identify` | `button` | `cmd/...` | |
| `reboot` | `button` | `cmd/reboot`, payload `reboot` | `device_class` restart, diagnostic |
| `events` | `event` | `~/event` | `event_types` from the table above |
| `rssi`, `board_temp`, `uptime` | `sensor` | `diag.*`, `up` | diagnostic |

Excerpt, using HA's abbreviated keys:

```json
{
  "dev": {"ids": "mm-a1b2c3", "name": "minimeater", "mf": "minimeater", "mdl": "minimeater v1", "sw": "0.1.0", "hw": "A"},
  "o": {"name": "minimeater", "sw": "0.1.0", "url": "https://github.com/NathanHowell/minimeater"},
  "avty_t": "minimeater/mm-a1b2c3/status",
  "stat_t": "minimeater/mm-a1b2c3/state",
  "qos": 1,
  "cmps": {
    "pit": {
      "p": "climate", "uniq_id": "mm-a1b2c3_pit", "name": "Pit",
      "modes": ["off", "heat"],
      "mode_cmd_t": "minimeater/mm-a1b2c3/set/mode",
      "mode_cmd_tpl": "{{ 'run' if value == 'heat' else 'off' }}",
      "mode_stat_t": "minimeater/mm-a1b2c3/state",
      "mode_stat_tpl": "{{ 'off' if value_json.mode == 'off' else 'heat' }}",
      "temp_cmd_t": "minimeater/mm-a1b2c3/set/sp",
      "temp_stat_t": "minimeater/mm-a1b2c3/state",
      "temp_stat_tpl": "{{ value_json.sp }}",
      "curr_temp_t": "minimeater/mm-a1b2c3/state",
      "curr_temp_tpl": "{{ value_json.pit }}",
      "act_t": "minimeater/mm-a1b2c3/state",
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
      "stat_t": "minimeater/mm-a1b2c3/event",
      "evt_typ": ["boot", "alarm", "alarm_clear", "lid_open", "lid_closed", "fire_low", "fire_out",
                  "setpoint_reached", "stall_start", "stall_end", "wrap_reminder", "stall_bump",
                  "stall_restore", "autotune_done", "selfcheck", "rejected", "fault", "fault_clear"]
    }
  }
}
```

**Sizing:** the full discovery message is about 16 KB (72 components). The MQTT TX buffer must hold it, or the client must stream the publish body. Everything else fits in 2 KB.
