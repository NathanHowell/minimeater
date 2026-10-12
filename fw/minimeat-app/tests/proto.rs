//! The protocol codec (`.5.6`) against docs/protocol.md: the writers must produce the doc's own
//! JSON examples, every event type and every `set`/`cmd` in the doc's tables must be handled, and
//! the parser must hold its limits.

use minimeat_app::alarms::Limit;
use minimeat_app::config::{ControllerKind, Cook, Mode, ProbeKind, Settings};
use minimeat_app::pit::{Command, Event, FaultCode, Output, Phase};
use minimeat_app::probes::{Check, ProbeSt};
use minimeat_app::proto::{self, ErrorCode, Json, ProbeChange, Request};
use minimeat_app::tasks::{AutotuneInfo, Faults, Status};
use minimeat_core::control::{Fire, Fopdt, Schedule};
use minimeat_core::food::{Cut, Food, Stall};
use minimeat_core::model::airflow::Actuation;
use minimeat_core::thermistor::{Preset, Thermistor};
use serde_json::{Value, json};

const DOC: &str = include_str!("../../../docs/protocol.md");

fn doc_examples() -> Vec<Value> {
    DOC.split("```json\n")
        .skip(1)
        .map(|b| serde_json::from_str(b.split("```").next().unwrap()).expect("doc example is JSON"))
        .collect()
}

fn doc_example(has: &str) -> Value {
    doc_examples()
        .into_iter()
        .find(|v| v.get(has).is_some())
        .unwrap_or_else(|| panic!("no example with {has}"))
}

fn write(f: impl FnOnce(&mut Json<heapless::String<2048>>)) -> Value {
    let mut j = Json::new(heapless::String::<2048>::new());
    f(&mut j);
    serde_json::from_str(j.finish().expect("fits").as_str()).expect("valid JSON")
}

/// The numbers compare equal when they round to the same value at the wire's precision.
fn same(a: &Value, b: &Value, path: &str) {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            assert!((x - y).abs() <= 1e-9 * x.abs().max(1.0), "{path}: {x} vs {y}");
        }
        (Value::Object(x), Value::Object(y)) => {
            let kx: std::collections::BTreeSet<_> = x.keys().collect();
            let ky: std::collections::BTreeSet<_> = y.keys().collect();
            assert_eq!(kx, ky, "{path}: keys");
            for (k, v) in x {
                same(v, &y[k], &format!("{path}.{k}"));
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            assert_eq!(x.len(), y.len(), "{path}: length");
            for (i, (a, b)) in x.iter().zip(y).enumerate() {
                same(a, b, &format!("{path}[{i}]"));
            }
        }
        _ => assert_eq!(a, b, "{path}"),
    }
}

fn status() -> Status {
    let cook = Cook {
        mode: Mode::Run,
        sp_f: 225.0,
        ..Cook::default()
    };
    Status {
        up_s: 5321,
        cook,
        out: Output {
            phase: Phase::Hold,
            q: 0.82,
            act: Actuation { damper: 0.0, fan: 0.0 },
            flap_pct: 34.0,
            ref_f: 225.0,
            lid: false,
            fire: Fire::Ok,
        },
        temps: [Some(224.6), Some(158.3), None, None],
        probe_st: [ProbeSt::Ok, ProbeSt::Ok, ProbeSt::Open, ProbeSt::Open],
        alarms: [None; 4],
        alarm_led: false,
        adc_die_c: Some(41.2),
        fan_rpm: Some(0.0),
        faults: Faults::default(),
        autotune: AutotuneInfo {
            exciting: false,
            accepted: 0,
            window: 0.35,
        },
        foods: [
            None,
            Some(Food {
                rate_f_h: Some(1.4),
                eta_s: Some(21600.0),
                stall: Stall::In { since_s: 600 },
            }),
            None,
            None,
        ],
        // 64.2F
        ambient_c: Some((64.2 - 32.0) / 1.8),
    }
}

#[test]
fn state_is_the_docs_example() {
    let got = write(|j| proto::state(j, &status(), Some(-61)));
    same(&got, &doc_example("phase"), "state");
}

#[test]
fn state_reports_faults_alarms_and_missing_values() {
    let mut s = status();
    s.cook.mode = Mode::Off;
    s.temps = [None; 4];
    s.probe_st = [ProbeSt::Fault; 4];
    s.alarms = [Some(Limit::Hi), None, Some(Limit::Lo), None];
    s.faults = Faults::default();
    s.ambient_c = None;
    s.fan_rpm = None;
    s.adc_die_c = None;
    s.foods = [None; 4];
    let got = write(|j| proto::state(j, &s, None));
    assert_eq!(got["pit"], Value::Null);
    assert_eq!(got["amb"], Value::Null);
    assert_eq!(got["autotune"], Value::Null, "only while running");
    assert_eq!(got["out"]["rpm"], Value::Null);
    assert_eq!(got["diag"], json!({"rssi": null, "board": null}));
    assert_eq!(got["probes"][0]["alarm"], "hi");
    assert_eq!(got["probes"][2]["alarm"], "lo");
    assert_eq!(got["probes"][0]["st"], "fault");
    assert_eq!(got["mode"], "off");
    // faults come out by wire name
    let mut s = status();
    let mut f = Faults::default();
    for c in [FaultCode::FanStall, FaultCode::Adc] {
        f.set(c, true);
    }
    s.faults = f;
    let got = write(|j| proto::state(j, &s, None));
    assert_eq!(got["faults"], json!(["fan_stall", "adc"]));
}

fn doc_settings() -> Settings {
    let mut s = Settings::default();
    s.cook.mode = Mode::Run;
    s.cook.sp_f = 225.0;
    s.cook.controller = ControllerKind::Mpc;
    s.lid.boost = 1.5;
    s.probes[0].name = "Pit".try_into().unwrap();
    s.probes[1].name = "Brisket".try_into().unwrap();
    s.probes[1].offset_f = -0.4;
    s.probes[1].hi_f = Some(203.0);
    s.probes[1].cut = Cut::Brisket;
    s.probes[2].name = "Probe 3".try_into().unwrap();
    s.probes[3].name = "Probe 4".try_into().unwrap();
    s.probes[3].kind = ProbeKind::Custom(Thermistor {
        a: 7.3431e-4,
        b: 2.1574e-4,
        c: 9.5157e-8,
    });
    s.schedule = Schedule::new(&[Fopdt {
        sp_f: 225.0,
        k: 116.0,
        tau_s: 480.0,
        theta_s: 110.0,
    }]);
    s
}

#[test]
fn config_is_the_docs_example() {
    let got = write(|j| proto::config(j, &doc_settings()));
    same(&got, &doc_example("model"), "config");
}

#[test]
fn config_model_is_empty_until_something_is_learned() {
    let got = write(|j| proto::config(j, &Settings::default()));
    assert_eq!(got["model"], json!([]));
    assert_eq!(got["probes"][0]["type"], "thermoworks_pro");
    assert_eq!(got["probes"][0]["coeffs"], Value::Null);
}

#[test]
fn events_are_the_docs_examples_and_every_type_is_listed() {
    let model = Fopdt {
        sp_f: 225.0,
        k: 116.0,
        tau_s: 480.0,
        theta_s: 110.0,
    };
    let evs = doc_examples()
        .into_iter()
        .filter(|v| v.get("event_type").is_some())
        .collect::<Vec<_>>();
    let find = |t: &str| {
        evs.iter()
            .find(|v| v["event_type"] == t)
            .unwrap_or_else(|| panic!("no {t} example"))
    };
    same(
        &write(|j| {
            proto::event(
                j,
                &Event::Alarm {
                    probe: 1,
                    which: Limit::Hi,
                    t_f: 203.4,
                },
            )
        }),
        find("alarm"),
        "alarm",
    );
    same(
        &write(|j| proto::event(j, &Event::AutotuneDone { model, accepted: 6 })),
        find("autotune_done"),
        "autotune_done",
    );
    same(
        &write(|j| proto::rejected(j, "set/sp", "out_of_range", "400 > 325")),
        find("rejected"),
        "rejected",
    );

    // one of every event the control loop raises, against the doc's table of event types
    let listed: Vec<String> = doc_example("cmps")["cmps"]["events"]["evt_typ"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let all = [
        Event::LidOpen { pit_f: 220.0 },
        Event::LidClosed { pit_f: 240.0 },
        Event::FireLow { pit_f: 200.0, q: 3.0 },
        Event::FireOut { pit_f: 180.0, q: 2.0 },
        Event::SetpointReached { sp_f: 225.0 },
        Event::StallStart { probe: 1, t_f: 150.0 },
        Event::StallEnd { probe: 1, t_f: 170.0 },
        Event::AutotuneDone { model, accepted: 0 },
        Event::Alarm {
            probe: 0,
            which: Limit::Lo,
            t_f: 100.0,
        },
        Event::AlarmClear {
            probe: 0,
            which: Limit::Lo,
            t_f: 106.0,
        },
        Event::SelfCheck {
            results: [
                Check::Delta(0.4),
                Check::Delta(-3.1),
                Check::St(ProbeSt::Open),
                Check::St(ProbeSt::Short),
            ],
        },
        Event::Fault(FaultCode::PitProbe),
        Event::FaultClear(FaultCode::PitProbe),
    ];
    for e in all {
        let v = write(|j| proto::event(j, &e));
        assert_eq!(v["v"], 1);
        assert!(
            listed.contains(&v["event_type"].as_str().unwrap().to_string()),
            "{e:?} is not in the doc's list"
        );
    }
    let sc = write(|j| {
        proto::event(
            j,
            &Event::SelfCheck {
                results: [
                    Check::Delta(0.44),
                    Check::Delta(-3.1),
                    Check::St(ProbeSt::Open),
                    Check::St(ProbeSt::Short),
                ],
            },
        )
    });
    assert_eq!(
        sc["results"],
        json!([{"probe": 0, "delta": 0.4}, {"probe": 1, "delta": -3.1}, {"probe": 2, "st": "open"}, {"probe": 3, "st": "short"}])
    );
    let boot = write(|j| proto::boot(j, proto::BootReason::PowerOn, "0.1.0"));
    assert_eq!(
        boot,
        json!({"v": 1, "event_type": "boot", "reason": "power_on", "sw": "0.1.0"})
    );
}

// ---------------------------------------------------------------------------------- the parser

fn ok(topic: &str, payload: &str) -> Request {
    proto::parse(topic, payload.as_bytes(), false).unwrap_or_else(|e| panic!("{topic} {payload:?}: {e:?}"))
}

fn err(topic: &str, payload: &str) -> ErrorCode {
    match proto::parse(topic, payload.as_bytes(), false) {
        Ok(r) => panic!("{topic} {payload:?} was accepted: {r:?}"),
        Err(e) => e.code,
    }
}

#[test]
fn every_setting_and_command_in_the_docs_tables_is_handled() {
    // a valid payload for each row of the set and cmd tables
    let sets = [
        ("mode", "run"),
        ("sp", "225"),
        ("pit_probe", "2"),
        ("controller", "ffpi"),
        ("lid_detect", "ON"),
        ("lid_boost", "1.5"),
        ("manual_flap", "40"),
        ("manual_fan", "10"),
        ("probe/<n>/name", "Brisket"),
        ("probe/<n>/type", "maverick_et732"),
        ("probe/<n>/coeffs", r#"{"a":7.3431e-4,"b":2.1574e-4,"c":9.5157e-8}"#),
        ("probe/<n>/offset", "-0.4"),
        ("probe/<n>/lo", "150"),
        ("probe/<n>/hi", "off"),
        ("probe/<n>/cut", "pork_shoulder"),
    ];
    let cmds = [
        ("autotune", ""),
        ("autotune_stop", ""),
        ("model_reset", "reset"),
        ("selfcheck", ""),
        ("alarm_ack", ""),
        ("identify", ""),
        ("reboot", "reboot"),
    ];
    // every name in the first column of the doc's tables
    let section = |from: &str, to: &str| DOC.split(from).nth(1).unwrap().split(to).next().unwrap();
    let names = |sect: &str| -> Vec<String> {
        sect.lines()
            .filter(|l| l.starts_with("| `"))
            .flat_map(|l| {
                l.split('|')
                    .nth(1)
                    .unwrap()
                    .split('`')
                    .skip(1)
                    .step_by(2)
                    .map(String::from)
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    for n in names(section("## `~/set/<setting>`", "## `~/cmd/<command>`")) {
        assert!(sets.iter().any(|(s, _)| *s == n), "doc setting {n} has no case");
    }
    for n in names(section("## `~/cmd/<command>`", "## `~/event`")) {
        // `ota` is reserved and answered `not_allowed`
        assert!(
            n == "ota" || cmds.iter().any(|(c, _)| *c == n),
            "doc command {n} has no case"
        );
    }
    for (name, payload) in sets {
        let topic = format!("set/{}", name.replace("<n>", "1"));
        proto::parse(&topic, payload.as_bytes(), false).unwrap_or_else(|e| panic!("{topic} {payload}: {e:?}"));
    }
    for (name, payload) in cmds {
        proto::parse(&format!("cmd/{name}"), payload.as_bytes(), false).unwrap_or_else(|e| panic!("{name}: {e:?}"));
    }
}

#[test]
fn settings_parse_to_the_right_requests() {
    use Command::*;
    assert_eq!(ok("set/mode", "heat"), Request::Pit(Mode(self::Mode::Run)));
    assert_eq!(ok("set/mode", "off"), Request::Pit(Mode(self::Mode::Off)));
    assert_eq!(ok("set/sp", "225.5"), Request::Pit(Sp(225.5)));
    assert_eq!(ok("set/sp", " 150\n"), Request::Pit(Sp(150.0)), "whitespace is ignored");
    assert_eq!(ok("set/pit_probe", "3"), Request::Pit(PitProbe(3)));
    assert_eq!(
        ok("set/controller", "mpc"),
        Request::Pit(Controller(ControllerKind::Mpc))
    );
    assert_eq!(ok("set/lid_detect", "OFF"), Request::Pit(LidDetect(false)));
    assert_eq!(ok("set/lid_detect", "on"), Request::Pit(LidDetect(true)));
    assert_eq!(ok("set/lid_boost", "3"), Request::Pit(LidBoost(3.0)));
    assert_eq!(ok("set/manual_fan", "0"), Request::Pit(ManualFan(0.0)));
    let probe = |n, change| Request::Probe { n, change };
    assert_eq!(
        ok("set/probe/2/name", "Ribs ☃"),
        probe(2, ProbeChange::Name("Ribs ☃".try_into().unwrap()))
    );
    assert_eq!(
        ok("set/probe/0/type", "generic_b3950"),
        probe(0, ProbeChange::Kind(ProbeKind::Preset(Preset::GenericB3950)))
    );
    assert_eq!(ok("set/probe/1/offset", "10"), probe(1, ProbeChange::Offset(10.0)));
    assert_eq!(ok("set/probe/1/lo", "0"), probe(1, ProbeChange::Lo(None)));
    assert_eq!(ok("set/probe/1/hi", "OFF"), probe(1, ProbeChange::Hi(None)));
    assert_eq!(ok("set/probe/1/hi", "203"), probe(1, ProbeChange::Hi(Some(203.0))));
    assert_eq!(ok("set/probe/3/cut", "ribs"), probe(3, ProbeChange::Cut(Cut::Ribs)));
    let Request::Probe {
        change: ProbeChange::Kind(ProbeKind::Custom(t)),
        ..
    } = ok(
        "set/probe/3/coeffs",
        r#" { "c": 9.5157e-8, "a": 7.3431e-4 , "b": 2.1574e-4 } "#,
    )
    else {
        panic!("not custom")
    };
    assert_eq!(
        (t.a, t.b, t.c),
        (7.3431e-4, 2.1574e-4, 9.5157e-8),
        "any order, whitespace allowed"
    );
}

#[test]
fn commands_parse_and_the_guards_hold() {
    assert_eq!(ok("cmd/autotune", ""), Request::Pit(Command::Autotune));
    assert_eq!(
        ok("cmd/autotune", "PRESS"),
        Request::Pit(Command::Autotune),
        "HA button payloads are ignored"
    );
    assert_eq!(ok("cmd/selfcheck", ""), Request::Pit(Command::SelfCheck));
    assert_eq!(ok("cmd/alarm_ack", ""), Request::Pit(Command::AlarmAck));
    assert_eq!(ok("cmd/identify", ""), Request::Identify);
    assert_eq!(ok("cmd/reboot", "reboot"), Request::Reboot);
    assert_eq!(
        err("cmd/reboot", ""),
        ErrorCode::BadPayload,
        "a stray publish does not reboot"
    );
    assert_eq!(err("cmd/reboot", "now"), ErrorCode::BadPayload);
    assert_eq!(err("cmd/model_reset", ""), ErrorCode::BadPayload);
    assert_eq!(err("cmd/ota", ""), ErrorCode::NotAllowed);
    assert_eq!(err("cmd/nope", ""), ErrorCode::Unknown);
}

#[test]
fn limits_and_errors_use_the_documented_codes() {
    assert_eq!(err("set/sp", "400"), ErrorCode::OutOfRange);
    assert_eq!(err("set/sp", "149.9"), ErrorCode::OutOfRange);
    assert_eq!(err("set/sp", "hot"), ErrorCode::BadPayload);
    assert_eq!(err("set/sp", "nan"), ErrorCode::BadPayload);
    assert_eq!(err("set/sp", "inf"), ErrorCode::BadPayload);
    assert_eq!(err("set/sp", ""), ErrorCode::BadPayload);
    assert_eq!(err("set/mode", "cook"), ErrorCode::BadPayload);
    assert_eq!(err("set/controller", "pid"), ErrorCode::BadPayload);
    assert_eq!(err("set/lid_detect", "maybe"), ErrorCode::BadPayload);
    assert_eq!(err("set/lid_boost", "0.5"), ErrorCode::OutOfRange);
    assert_eq!(err("set/lid_boost", "3.1"), ErrorCode::OutOfRange);
    assert_eq!(err("set/manual_flap", "-1"), ErrorCode::OutOfRange);
    assert_eq!(err("set/manual_fan", "101"), ErrorCode::OutOfRange);
    assert_eq!(err("set/pit_probe", "4"), ErrorCode::OutOfRange);
    assert_eq!(err("set/pit_probe", "1.5"), ErrorCode::BadPayload);
    assert_eq!(
        err("set/pit_probe", "tc"),
        ErrorCode::NotAllowed,
        "no thermocouple fitted"
    );
    assert_eq!(err("set/probe/4/name", "x"), ErrorCode::Unknown);
    assert_eq!(err("set/probe/x/name", "x"), ErrorCode::Unknown);
    assert_eq!(err("set/probe/0", "x"), ErrorCode::Unknown);
    assert_eq!(err("set/probe/0/colour", "x"), ErrorCode::Unknown);
    assert_eq!(err("set/probe/0/name", ""), ErrorCode::OutOfRange);
    assert_eq!(err("set/probe/0/name", &"x".repeat(25)), ErrorCode::OutOfRange);
    assert!(
        proto::parse("set/probe/0/name", "é".repeat(12).as_bytes(), false).is_ok(),
        "24 bytes of UTF-8"
    );
    assert_eq!(err("set/probe/0/type", "k_type"), ErrorCode::BadPayload);
    // `custom` parses (the HA select offers it); applying it needs coefficients already set
    assert_eq!(
        ok("set/probe/0/type", "custom"),
        Request::Probe {
            n: 0,
            change: ProbeChange::KeepCustom
        }
    );
    assert_eq!(err("set/probe/0/offset", "10.5"), ErrorCode::OutOfRange);
    assert_eq!(err("set/probe/0/hi", "701"), ErrorCode::OutOfRange);
    assert_eq!(err("set/probe/0/lo", "-5"), ErrorCode::OutOfRange);
    assert_eq!(err("set/probe/0/cut", "chicken"), ErrorCode::BadPayload);
    assert_eq!(err("set/nope", "1"), ErrorCode::Unknown);
    assert_eq!(err("state", ""), ErrorCode::Unknown);
    // coefficient payloads
    assert_eq!(err("set/probe/0/coeffs", "7e-4"), ErrorCode::BadPayload);
    assert_eq!(err("set/probe/0/coeffs", r#"{"a":1,"b":2}"#), ErrorCode::BadPayload);
    assert_eq!(
        err("set/probe/0/coeffs", r#"{"a":1,"b":2,"c":3,"d":4}"#),
        ErrorCode::BadPayload
    );
    assert_eq!(
        err("set/probe/0/coeffs", r#"{"a":1,"a":2,"c":3}"#),
        ErrorCode::BadPayload
    );
    assert_eq!(
        err("set/probe/0/coeffs", r#"{"a":"x","b":2,"c":3}"#),
        ErrorCode::BadPayload
    );
    // a curve that is not an NTC is out of range, not a parse error
    assert_eq!(
        err("set/probe/0/coeffs", r#"{"a":1e-3,"b":-2e-4,"c":0}"#),
        ErrorCode::OutOfRange
    );
    assert_eq!(
        err("set/probe/0/coeffs", r#"{"a":0,"b":0,"c":0}"#),
        ErrorCode::OutOfRange
    );
    // not text at all
    assert_eq!(
        proto::parse("set/sp", &[0xff, 0xfe], false).unwrap_err().code,
        ErrorCode::BadPayload
    );
}

#[test]
fn a_retained_command_is_dropped_whatever_it_says() {
    for (t, p) in [("set/sp", "225"), ("cmd/reboot", "reboot"), ("set/nope", "")] {
        assert_eq!(
            proto::parse(t, p.as_bytes(), true).unwrap_err().code,
            ErrorCode::Retained,
            "{t}"
        );
    }
}

#[test]
fn error_details_say_what_was_wrong_and_fit() {
    let e = proto::parse("set/sp", b"400", false).unwrap_err();
    assert_eq!(e.code.name(), "out_of_range");
    assert_eq!(e.detail.as_str(), "sp 400 outside 150..325");
    let e = proto::Error::new(ErrorCode::BadPayload, &"x".repeat(200));
    assert_eq!(e.detail.len(), 48, "a long detail is cut");
}

// ------------------------------------------------------------------ Home Assistant discovery

fn discovery() -> (String, Value) {
    let d = proto::Device {
        prefix: "minimeat",
        id: "mm-a1b2c3",
        sw: "0.1.0",
        hw: "A",
    };
    let text = proto::render::<24576>(|j| proto::discovery(j, &d, ["Pit", "Brisket", "Probe 3", "Probe 4"]))
        .expect("fits 24 KB");
    let v: Value = serde_json::from_str(&text).expect("valid JSON");
    (text.as_str().to_string(), v)
}

#[test]
fn discovery_matches_the_docs_excerpt() {
    let (text, got) = discovery();
    println!("discovery message: {} bytes", text.len());
    let doc = doc_example("cmps");
    // the device-level fields are the doc's, exactly
    for k in ["dev", "o", "avty_t", "stat_t", "qos"] {
        same(&got[k], &doc[k], k);
    }
    // and the three components it excerpts
    for k in ["pit", "probe1", "events"] {
        same(&got["cmps"][k], &doc["cmps"][k], k);
    }
}

#[test]
fn every_entity_is_unique_and_named() {
    let (_, v) = discovery();
    let cmps = v["cmps"].as_object().unwrap();
    assert!(cmps.len() >= 60, "{} components", cmps.len());
    let mut ids = std::collections::BTreeSet::new();
    for (key, c) in cmps {
        assert_eq!(c["uniq_id"], format!("mm-a1b2c3_{key}"), "{key}");
        assert!(
            ids.insert(c["uniq_id"].as_str().unwrap().to_string()),
            "{key}: duplicate unique_id"
        );
        assert!(
            c["p"].is_string() && c["name"].as_str().is_some_and(|n| !n.is_empty()),
            "{key}"
        );
    }
    // every platform the doc's table lists is there
    for p in [
        "climate",
        "sensor",
        "binary_sensor",
        "select",
        "text",
        "number",
        "switch",
        "button",
        "event",
    ] {
        assert!(cmps.values().any(|c| c["p"] == p), "no {p}");
    }
    // probe entities follow the probe names
    assert_eq!(cmps["probe1"]["name"], "Brisket");
    assert_eq!(cmps["probe1_eta"]["name"], "Brisket ETA");
    assert_eq!(cmps["probe3_hi"]["name"], "Probe 4 high limit");
    assert_eq!(cmps["probe3_name"]["name"], "Probe 4 name");
}

#[test]
fn every_command_an_entity_sends_is_accepted_by_the_parser() {
    let (_, v) = discovery();
    let suffix = |topic: &str| {
        topic
            .strip_prefix("minimeat/mm-a1b2c3/")
            .unwrap_or_else(|| panic!("{topic}"))
            .to_string()
    };
    let accepts = |topic: &str, payload: &str, what: &str| {
        proto::parse(&suffix(topic), payload.as_bytes(), false)
            .unwrap_or_else(|e| panic!("{what}: {topic} <- {payload:?}: {e:?}"));
    };
    for (key, c) in v["cmps"].as_object().unwrap() {
        match c["p"].as_str().unwrap() {
            "select" => {
                for o in c["ops"].as_array().unwrap() {
                    accepts(c["cmd_t"].as_str().unwrap(), o.as_str().unwrap(), key);
                }
            }
            "number" => {
                let t = c["cmd_t"].as_str().unwrap();
                for b in ["min", "max"] {
                    accepts(t, &format!("{}", c[b].as_f64().unwrap()), key);
                }
            }
            "text" => accepts(c["cmd_t"].as_str().unwrap(), "x", key),
            "switch" => {
                accepts(c["cmd_t"].as_str().unwrap(), "ON", key);
                accepts(c["cmd_t"].as_str().unwrap(), "OFF", key);
            }
            "button" => accepts(c["cmd_t"].as_str().unwrap(), c["pl_prs"].as_str().unwrap(), key),
            "climate" => {
                accepts(c["mode_cmd_t"].as_str().unwrap(), "run", key);
                accepts(c["mode_cmd_t"].as_str().unwrap(), "off", key);
                for t in [c["min_temp"].as_f64().unwrap(), c["max_temp"].as_f64().unwrap()] {
                    accepts(c["temp_cmd_t"].as_str().unwrap(), &format!("{t:.1}"), key);
                }
            }
            _ => {}
        }
    }
}

/// `value_json.a.b[2].c` paths in a template.
fn paths(tpl: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = tpl;
    while let Some(i) = rest.find("value_json.") {
        rest = &rest[i + "value_json.".len()..];
        let end = rest
            .find(|c: char| !(c.is_alphanumeric() || "._[]".contains(c)))
            .unwrap_or(rest.len());
        out.push(rest[..end].to_string());
        rest = &rest[end..];
    }
    out
}

fn resolve(root: &Value, path: &str) -> bool {
    let mut v = root;
    for seg in path.split('.') {
        let (name, idx) = match seg.split_once('[') {
            Some((n, i)) => (n, Some(i.trim_end_matches(']').parse::<usize>().unwrap())),
            None => (seg, None),
        };
        if v.is_null() {
            return true; // guarded by an `is not none` in the template
        }
        let Some(next) = v.get(name) else { return false };
        v = next;
        if let Some(i) = idx {
            let Some(next) = v.get(i) else { return false };
            v = next;
        }
    }
    true
}

#[test]
fn every_template_reads_a_field_the_device_publishes() {
    let (_, v) = discovery();
    let state = write(|j| proto::state(j, &status(), Some(-61)));
    let config = write(|j| proto::config(j, &doc_settings()));
    for (key, c) in v["cmps"].as_object().unwrap() {
        // settings read ~/config, the rest ~/state
        let from_config = c["stat_t"].as_str().is_some_and(|t| t.ends_with("/config"));
        let root = if from_config { &config } else { &state };
        for field in ["val_tpl", "mode_stat_tpl", "temp_stat_tpl", "curr_temp_tpl", "act_tpl"] {
            let Some(tpl) = c[field].as_str() else { continue };
            for p in paths(tpl) {
                assert!(
                    resolve(root, &p),
                    "{key}.{field}: `{p}` is not in {}",
                    if from_config { "~/config" } else { "~/state" }
                );
            }
        }
        // ... and the topics it reads are ones the device publishes
        for t in ["stat_t", "mode_stat_t", "temp_stat_t", "curr_temp_t", "act_t"] {
            if let Some(t) = c[t].as_str() {
                assert!(
                    [
                        "minimeat/mm-a1b2c3/state",
                        "minimeat/mm-a1b2c3/config",
                        "minimeat/mm-a1b2c3/event"
                    ]
                    .contains(&t),
                    "{key}: {t}"
                );
            }
        }
    }
}

#[test]
fn discovery_escapes_probe_names() {
    let d = proto::Device {
        prefix: "minimeat",
        id: "mm-000001",
        sw: "0.1.0",
        hw: "A",
    };
    let text = proto::render::<24576>(|j| proto::discovery(j, &d, ["Pit \"A\"", "Ribs\\x", "é☃", "Probe 4"])).unwrap();
    let v: Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(v["cmps"]["probe0"]["name"], "Pit \"A\"");
    assert_eq!(v["cmps"]["probe1_eta"]["name"], "Ribs\\x ETA");
    assert_eq!(v["cmps"]["probe2"]["name"], "é☃");
    let mut topic = heapless::String::<64>::new();
    proto::discovery::topic(&mut topic, &d).unwrap();
    assert_eq!(topic.as_str(), "homeassistant/device/mm-000001/config");
}
