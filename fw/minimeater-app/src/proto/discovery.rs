//! Home Assistant MQTT discovery (`.5.3`, docs/protocol.md): one retained device-discovery message,
//! `homeassistant/device/<id>/config` (HA 2024.11+), listing every entity. Written with HA's
//! abbreviated keys. The message is 15-20 KB: render it into a static buffer once and publish it.

use core::fmt::Write;

use minimeater_core::food::Cut;
use minimeater_core::thermistor::Preset;

use super::json::Json;
use crate::config::{BUMP_MAX_F, BUMP_MIN_F, StallPolicy};
use crate::hal::PROBE_JACKS;

/// What the discovery message needs to know about this device.
#[derive(Clone, Copy, Debug)]
pub struct Device<'a> {
    /// Topic prefix, `minimeater` unless changed.
    pub prefix: &'a str,
    /// `mm-` and the low three MAC bytes.
    pub id: &'a str,
    pub sw: &'a str,
    pub hw: &'a str,
}

/// Every `event_type` the device publishes, for the `event` entity.
pub const EVENT_TYPES: [&str; 18] = [
    "boot",
    "alarm",
    "alarm_clear",
    "lid_open",
    "lid_closed",
    "fire_low",
    "fire_out",
    "setpoint_reached",
    "stall_start",
    "stall_end",
    "wrap_reminder",
    "stall_bump",
    "stall_restore",
    "autotune_done",
    "selfcheck",
    "rejected",
    "fault",
    "fault_clear",
];

const URL: &str = "https://github.com/NathanHowell/minimeater";

/// The discovery topic for a device.
pub fn topic<W: Write>(w: &mut W, d: &Device) -> core::fmt::Result {
    write!(w, "homeassistant/device/{}/config", d.id)
}

#[derive(Clone, Copy, PartialEq)]
enum Cat {
    None,
    Config,
    Diagnostic,
}

struct E<'a, 'j, W: Write> {
    j: &'j mut Json<W>,
    d: &'a Device<'a>,
}

impl<W: Write> E<'_, '_, W> {
    fn topic(&mut self, key: &str, tail: &str) {
        let (p, id) = (self.d.prefix, self.d.id);
        self.j.key(key).strf(format_args!("{p}/{id}/{tail}"));
    }

    /// Opens a component: `"key": {"p": platform, "uniq_id": .., "name": ..`; the caller adds the
    /// rest and calls [`Self::end`].
    fn begin(&mut self, key: &str, platform: &str, name: core::fmt::Arguments, cat: Cat) {
        let id = self.d.id;
        self.j.key(key).begin_obj();
        self.j.key("p").str(platform);
        self.j.key("uniq_id").strf(format_args!("{id}_{key}"));
        self.j.key("name").strf(name);
        match cat {
            Cat::None => {}
            Cat::Config => {
                self.j.key("ent_cat").str("config");
            }
            Cat::Diagnostic => {
                self.j.key("ent_cat").str("diagnostic");
            }
        }
    }

    fn end(&mut self) {
        self.j.end_obj();
    }

    /// A setting reads its state from `~/config`, not the device-level `~/state`.
    fn read_config_topic(&mut self) {
        self.topic("stat_t", "config");
    }

    fn tpl(&mut self, v: core::fmt::Arguments) {
        self.j.key("val_tpl").strf(v);
    }

    fn sensor(
        &mut self,
        key: &str,
        name: core::fmt::Arguments,
        tpl: core::fmt::Arguments,
        dev_cla: Option<&str>,
        unit: Option<&str>,
        cat: Cat,
    ) {
        self.begin(key, "sensor", name, cat);
        if let Some(c) = dev_cla {
            self.j.key("dev_cla").str(c);
        }
        if let Some(u) = unit {
            self.j.key("unit_of_meas").str(u);
            self.j.key("stat_cla").str("measurement");
        }
        self.tpl(tpl);
        self.end();
    }

    fn enum_sensor(&mut self, key: &str, name: core::fmt::Arguments, tpl: core::fmt::Arguments, options: &[&str]) {
        self.begin(key, "sensor", name, Cat::None);
        self.j.key("dev_cla").str("enum");
        self.j.key("ops").begin_arr();
        for o in options {
            self.j.str(o);
        }
        self.j.end_arr();
        self.tpl(tpl);
        self.end();
    }

    fn binary(&mut self, key: &str, name: &str, tpl: &str, dev_cla: &str) {
        self.begin(key, "binary_sensor", format_args!("{name}"), Cat::None);
        self.j.key("dev_cla").str(dev_cla);
        self.tpl(format_args!("{tpl}"));
        self.end();
    }

    fn select(
        &mut self,
        key: &str,
        name: core::fmt::Arguments,
        set: core::fmt::Arguments,
        tpl: core::fmt::Arguments,
        options: &[&str],
    ) {
        self.begin(key, "select", name, Cat::Config);
        self.read_config_topic();
        self.j
            .key("cmd_t")
            .strf(format_args!("{}/{}/set/{set}", self.d.prefix, self.d.id));
        self.j.key("ops").begin_arr();
        for o in options {
            self.j.str(o);
        }
        self.j.end_arr();
        self.tpl(tpl);
        self.end();
    }

    #[allow(clippy::too_many_arguments, reason = "HA's number options, spelled out at each call")]
    fn number(
        &mut self,
        key: &str,
        name: core::fmt::Arguments,
        set: core::fmt::Arguments,
        tpl: core::fmt::Arguments,
        (min, max, step): (f64, f64, f64),
        unit: Option<&str>,
    ) {
        self.begin(key, "number", name, Cat::Config);
        self.read_config_topic();
        self.j
            .key("cmd_t")
            .strf(format_args!("{}/{}/set/{set}", self.d.prefix, self.d.id));
        self.j.key("min").num(min, 1);
        self.j.key("max").num(max, 1);
        self.j.key("step").num(step, 1);
        self.j.key("mode").str("box");
        if let Some(u) = unit {
            self.j.key("unit_of_meas").str(u);
        }
        self.tpl(tpl);
        self.end();
    }

    fn button(&mut self, key: &str, name: &str, command: &str, payload: &str, dev_cla: Option<&str>, cat: Cat) {
        self.begin(key, "button", format_args!("{name}"), cat);
        if let Some(c) = dev_cla {
            self.j.key("dev_cla").str(c);
        }
        self.j
            .key("cmd_t")
            .strf(format_args!("{}/{}/cmd/{command}", self.d.prefix, self.d.id));
        self.j.key("pl_prs").str(payload);
        self.end();
    }
}

/// Writes the device-discovery message. `probe_names` are the probes' current names: a rename
/// republishes this (docs/protocol.md).
pub fn discovery<W: Write>(j: &mut Json<W>, d: &Device, probe_names: [&str; PROBE_JACKS]) {
    let (prefix, id, sw, hw) = (d.prefix, d.id, d.sw, d.hw);
    j.begin_obj();
    j.key("dev").begin_obj();
    j.key("ids").str(id);
    j.key("name").str("minimeater");
    j.key("mf").str("minimeater");
    j.key("mdl").str("minimeater v1");
    j.key("sw").str(sw);
    j.key("hw").str(hw);
    j.end_obj();
    j.key("o").begin_obj();
    j.key("name").str("minimeater");
    j.key("sw").str(sw);
    j.key("url").str(URL);
    j.end_obj();
    j.key("avty_t").strf(format_args!("{prefix}/{id}/status"));
    j.key("stat_t").strf(format_args!("{prefix}/{id}/state"));
    j.key("qos").int(1);
    j.key("cmps").begin_obj();
    let mut e = E { j, d };

    // the pit, as a thermostat
    e.begin("pit", "climate", format_args!("Pit"), Cat::None);
    e.j.key("modes").begin_arr().str("off").str("heat").end_arr();
    e.topic("mode_cmd_t", "set/mode");
    e.j.key("mode_cmd_tpl").str("{{ 'run' if value == 'heat' else 'off' }}");
    e.topic("mode_stat_t", "state");
    e.j.key("mode_stat_tpl")
        .str("{{ 'off' if value_json.mode == 'off' else 'heat' }}");
    e.topic("temp_cmd_t", "set/sp");
    e.topic("temp_stat_t", "state");
    e.j.key("temp_stat_tpl").str("{{ value_json.sp }}");
    e.topic("curr_temp_t", "state");
    e.j.key("curr_temp_tpl").str("{{ value_json.pit }}");
    e.topic("act_t", "state");
    e.j.key("act_tpl")
        .str("{{ 'off' if value_json.mode == 'off' else ('heating' if value_json.out.q > 0 else 'idle') }}");
    e.j.key("min_temp").int(150);
    e.j.key("max_temp").int(325);
    e.j.key("temp_step").int(5);
    e.j.key("temp_unit").str("F");
    e.j.key("precision").num(0.1, 1);
    e.end();

    // temperatures
    let f = Some("°F");
    let temp = Some("temperature");
    e.sensor(
        "pit_temp",
        format_args!("Pit temperature"),
        format_args!("{{{{ value_json.pit }}}}"),
        temp,
        f,
        Cat::None,
    );
    for (n, name) in probe_names.iter().enumerate() {
        let mut key = heapless::String::<8>::new();
        let _ = write!(key, "probe{n}");
        e.sensor(
            &key,
            format_args!("{name}"),
            format_args!("{{{{ value_json.probes[{n}].t }}}}"),
            temp,
            f,
            Cat::None,
        );
    }
    e.sensor(
        "ambient",
        format_args!("Ambient"),
        format_args!("{{{{ value_json.amb }}}}"),
        temp,
        f,
        Cat::None,
    );

    // what the controller is doing
    e.sensor(
        "airflow",
        format_args!("Airflow"),
        format_args!("{{{{ value_json.out.q }}}}"),
        Some("volume_flow_rate"),
        Some("ft³/min"),
        Cat::None,
    );
    e.sensor(
        "flap",
        format_args!("Flap"),
        format_args!("{{{{ value_json.out.flap }}}}"),
        None,
        Some("%"),
        Cat::None,
    );
    e.sensor(
        "fan",
        format_args!("Fan"),
        format_args!("{{{{ value_json.out.fan }}}}"),
        None,
        Some("%"),
        Cat::None,
    );
    e.sensor(
        "fan_rpm",
        format_args!("Fan speed"),
        format_args!("{{{{ value_json.out.rpm }}}}"),
        None,
        Some("rpm"),
        Cat::Diagnostic,
    );
    e.enum_sensor(
        "phase",
        format_args!("Phase"),
        format_args!("{{{{ value_json.phase }}}}"),
        &["idle", "startup", "hold", "lid", "recover", "manual", "fault"],
    );
    e.enum_sensor(
        "fire",
        format_args!("Fire"),
        format_args!("{{{{ value_json.fire }}}}"),
        &["ok", "low", "out", "unknown"],
    );
    e.binary("lid", "Lid open", "{{ 'ON' if value_json.lid else 'OFF' }}", "opening");
    e.binary(
        "problem",
        "Problem",
        "{{ 'ON' if value_json.faults | length > 0 else 'OFF' }}",
        "problem",
    );

    // settings
    e.select(
        "mode",
        format_args!("Mode"),
        format_args!("mode"),
        format_args!("{{{{ value_json.mode }}}}"),
        &["off", "run", "manual"],
    );
    e.select(
        "controller",
        format_args!("Controller"),
        format_args!("controller"),
        format_args!("{{{{ value_json.controller }}}}"),
        &["mpc", "ffpi"],
    );
    e.select(
        "pit_probe",
        format_args!("Pit probe"),
        format_args!("pit_probe"),
        format_args!("{{{{ value_json.pit_probe }}}}"),
        &["0", "1", "2", "3"],
    );

    // per probe: the food estimator, then the probe's own setup
    let mut types = [""; 4];
    for (t, p) in types.iter_mut().zip(Preset::ALL) {
        *t = p.name();
    }
    let kinds = [types[0], types[1], types[2], "custom"];
    let cuts = Cut::ALL.map(Cut::name);
    let policies = StallPolicy::ALL.map(StallPolicy::name);
    for (n, name) in probe_names.iter().enumerate() {
        let mut key = heapless::String::<20>::new();
        let mut k = |suffix: &str| {
            key.clear();
            let _ = write!(key, "probe{n}_{suffix}");
            key.clone()
        };
        e.sensor(
            &k("eta"),
            format_args!("{name} ETA"),
            format_args!(
                "{{{{ value_json.probes[{n}].food.eta if value_json.probes[{n}].food is not none else None }}}}"
            ),
            Some("duration"),
            Some("s"),
            Cat::None,
        );
        e.enum_sensor(
            &k("stall"),
            format_args!("{name} stall"),
            format_args!(
                "{{{{ value_json.probes[{n}].food.stall if value_json.probes[{n}].food is not none else None }}}}"
            ),
            &["none", "stall", "done"],
        );
        e.select(
            &k("type"),
            format_args!("{name} type"),
            format_args!("probe/{n}/type"),
            format_args!("{{{{ value_json.probes[{n}].type }}}}"),
            &kinds,
        );
        e.select(
            &k("cut"),
            format_args!("{name} cut"),
            format_args!("probe/{n}/cut"),
            format_args!("{{{{ value_json.probes[{n}].cut }}}}"),
            &cuts,
        );
        e.select(
            &k("on_stall"),
            format_args!("{name} on stall"),
            format_args!("probe/{n}/stall"),
            format_args!("{{{{ value_json.probes[{n}].stall }}}}"),
            &policies,
        );
        e.number(
            &k("bump"),
            format_args!("{name} stall bump"),
            format_args!("probe/{n}/bump"),
            format_args!("{{{{ value_json.probes[{n}].bump }}}}"),
            (BUMP_MIN_F, BUMP_MAX_F, 5.0),
            Some("°F"),
        );
        e.begin(&k("name"), "text", format_args!("Probe {} name", n + 1), Cat::Config);
        e.read_config_topic();
        e.j.key("cmd_t").strf(format_args!("{prefix}/{id}/set/probe/{n}/name"));
        e.j.key("min").int(1);
        e.j.key("max").int(24);
        e.tpl(format_args!("{{{{ value_json.probes[{n}].name }}}}"));
        e.end();
        // 0 switches an alarm limit off
        for (limit, label) in [("lo", "low limit"), ("hi", "high limit")] {
            e.number(
                &k(limit),
                format_args!("{name} {label}"),
                format_args!("probe/{n}/{limit}"),
                format_args!(
                    "{{{{ value_json.probes[{n}].{limit} if value_json.probes[{n}].{limit} is not none else 0 }}}}"
                ),
                (0.0, 700.0, 1.0),
                Some("°F"),
            );
        }
        e.number(
            &k("offset"),
            format_args!("{name} offset"),
            format_args!("probe/{n}/offset"),
            format_args!("{{{{ value_json.probes[{n}].offset }}}}"),
            (-10.0, 10.0, 0.1),
            Some("°F"),
        );
    }

    e.begin("lid_detect", "switch", format_args!("Lid detect"), Cat::Config);
    e.read_config_topic();
    e.topic("cmd_t", "set/lid_detect");
    e.tpl(format_args!("{{{{ 'ON' if value_json.lid_detect else 'OFF' }}}}"));
    e.end();
    e.number(
        "lid_boost",
        format_args!("Lid recovery boost"),
        format_args!("lid_boost"),
        format_args!("{{{{ value_json.lid_boost }}}}"),
        (1.0, 3.0, 0.1),
        None,
    );
    e.number(
        "manual_flap",
        format_args!("Manual flap"),
        format_args!("manual_flap"),
        format_args!("{{{{ value_json.manual_flap }}}}"),
        (0.0, 100.0, 1.0),
        Some("%"),
    );
    e.number(
        "manual_fan",
        format_args!("Manual fan"),
        format_args!("manual_fan"),
        format_args!("{{{{ value_json.manual_fan }}}}"),
        (0.0, 100.0, 1.0),
        Some("%"),
    );

    // actions
    e.button("autotune", "Autotune", "autotune", "press", None, Cat::None);
    e.button(
        "autotune_stop",
        "Stop autotune",
        "autotune_stop",
        "press",
        None,
        Cat::None,
    );
    e.button("selfcheck", "Probe self-check", "selfcheck", "press", None, Cat::None);
    e.button("alarm_ack", "Acknowledge alarms", "alarm_ack", "press", None, Cat::None);
    e.button("identify", "Identify", "identify", "press", None, Cat::None);
    e.button("reboot", "Reboot", "reboot", "reboot", Some("restart"), Cat::Diagnostic);

    e.begin("events", "event", format_args!("Events"), Cat::None);
    e.topic("stat_t", "event");
    e.j.key("evt_typ").begin_arr();
    for t in EVENT_TYPES {
        e.j.str(t);
    }
    e.j.end_arr();
    e.end();

    // the device's own health
    e.sensor(
        "rssi",
        format_args!("WiFi signal"),
        format_args!("{{{{ value_json.diag.rssi }}}}"),
        Some("signal_strength"),
        Some("dBm"),
        Cat::Diagnostic,
    );
    e.sensor(
        "board_temp",
        format_args!("Board temperature"),
        format_args!("{{{{ value_json.diag.board }}}}"),
        temp,
        Some("°C"),
        Cat::Diagnostic,
    );
    e.sensor(
        "uptime",
        format_args!("Uptime"),
        format_args!("{{{{ value_json.up }}}}"),
        Some("duration"),
        Some("s"),
        Cat::Diagnostic,
    );

    j.end_obj();
    j.end_obj();
}
