//! The request path on the twin (`.5.6`): MQTT-style `set`/`cmd` messages are parsed, applied by the
//! control task, answered, and change what `~/config` and `~/state` report. Every status of a run is
//! also written as `~/state` JSON, which must fit the buffer the MQTT client will use.

use minimeater_app::config::{ControllerKind, Mode};
use minimeater_app::proto::{self, ErrorCode};
use minimeater_twin::{Action, Opts, Scenario, run};

fn msg(t: f64, id: u16, topic: &'static str, payload: &'static str) -> (f64, Action) {
    (t, Action::Mqtt { id, topic, payload })
}

#[test]
fn messages_change_the_cook_and_every_state_fits_its_buffer() {
    let mut sc = Scenario::kamado_brisket();
    sc.hours = 6.0;
    sc.actions.extend([
        msg(60.0, 1, "set/sp", "275"),
        msg(65.0, 2, "set/sp", "400"),
        msg(70.0, 3, "set/probe/0/name", "Egg"),
        msg(75.0, 4, "set/probe/1/hi", "195"),
        msg(80.0, 5, "cmd/reboot", ""),
        msg(85.0, 6, "set/controller", "ffpi"),
        msg(90.0, 7, "set/probe/0/offset", "5"),
        msg(95.0, 8, "set/lid_boost", "2.5"),
        msg(100.0, 9, "set/probe/2/cut", "ribs"),
        msg(105.0, 10, "cmd/selfcheck", ""),
        msg(
            110.0,
            11,
            "set/probe/3/coeffs",
            r#"{"a":7.3431e-4,"b":2.1574e-4,"c":9.5157e-8}"#,
        ),
        msg(115.0, 18, "set/probe/3/type", "custom"),
        msg(120.0, 19, "set/probe/0/type", "custom"),
        msg(7200.0, 12, "set/mode", "off"),
        msg(7201.0, 13, "cmd/autotune", ""),
        msg(7202.0, 14, "set/mode", "manual"),
        msg(7203.0, 15, "set/manual_flap", "40"),
        msg(7204.0, 16, "set/nonsense", "1"),
        msg(7500.0, 17, "set/mode", "heat"),
    ]);
    let r = run(sc, &Opts { speed: None }, |_, _| {});

    let reply = |id: u16| {
        &r.replies
            .iter()
            .find(|(_, i, _)| *i == id)
            .unwrap_or_else(|| panic!("no reply to {id}"))
            .2
    };
    for id in [1, 3, 4, 6, 7, 8, 9, 10, 11, 12, 14, 15, 17] {
        assert!(reply(id).is_ok(), "request {id}: {:?}", reply(id));
    }
    let code = |id: u16| reply(id).as_ref().unwrap_err().code;
    assert_eq!(code(2), ErrorCode::OutOfRange);
    assert_eq!(code(5), ErrorCode::BadPayload, "a reboot needs its guard word");
    assert_eq!(code(13), ErrorCode::NotAllowed, "autotune in off mode");
    assert_eq!(code(16), ErrorCode::Unknown);
    assert!(reply(18).is_ok(), "custom on a probe that has coefficients");
    assert_eq!(code(19), ErrorCode::NotAllowed, "custom without coefficients");

    // `~/config` went out at boot and after the accepted changes, and carries them
    let last = |t: f64| r.configs.iter().rev().find(|(at, _)| *at <= t).map(|(_, c)| c).unwrap();
    assert!(r.configs[0].0 < 1.0, "once at boot");
    let c = last(150.0);
    assert_eq!(c.cook.sp_f, 275.0);
    assert_eq!(c.cook.controller, ControllerKind::FfPi);
    assert_eq!(c.probes[0].name.as_str(), "Egg");
    assert_eq!(c.probes[0].offset_f, 5.0);
    assert_eq!(c.probes[1].hi_f, Some(195.0));
    assert_eq!(c.probes[2].cut, minimeater_core::food::Cut::Ribs);
    assert!(matches!(c.probes[3].kind, minimeater_app::config::ProbeKind::Custom(_)));
    assert_eq!(c.lid.boost, 2.5);
    assert_eq!(c.cook.mode, Mode::Run);
    assert_eq!(last(7300.0).cook.mode, Mode::Manual);
    assert_eq!(last(7300.0).cook.manual_flap, 40.0);
    assert_eq!(last(7600.0).cook.mode, Mode::Run);
    // a rejected request does not republish
    let at = |t: f64| r.configs.iter().filter(|(a, _)| (t..t + 4.0).contains(a)).count();
    assert_eq!(at(65.0), 0, "the rejected sp 400");
    assert_eq!(at(80.0), 0, "the rejected reboot");
    assert_eq!(at(85.0), 1, "an accepted controller change");

    // and the live state followed: setpoint, the offset on the pit reading, the cut
    let s = |t: f64| r.samples.iter().find(|s| s.t_s >= t).unwrap();
    assert_eq!(s(150.0).status.cook.sp_f, 275.0);
    assert_eq!(s(7300.0).status.out.phase, minimeater_app::pit::Phase::Manual);
    assert_eq!(
        (s(7300.0).status.out.flap_pct, s(7300.0).status.cook.mode),
        (40.0, Mode::Manual)
    );
    let off = s(150.0).status.temps[0].unwrap() - s(150.0).pit_f;
    assert!((off - 5.0).abs() < 1.0, "pit reads {off:.1}F above the probe's truth");

    // every status of the cook as `~/state`, in the buffer the MQTT client will use
    let mut longest = 0;
    for s in &r.samples {
        let out = proto::render::<1024>(|j| proto::state(j, &s.status, Some(-60)))
            .unwrap_or_else(|| panic!("a status at {} s does not fit 1 KB", s.t_s));
        longest = longest.max(out.len());
    }
    println!("longest ~/state: {longest} bytes");
    assert!(longest < 900, "{longest} bytes leaves little headroom in 1 KB");
    let config = proto::render::<2048>(|j| proto::config(j, &r.configs.last().unwrap().1)).expect("fits 2 KB");
    println!("~/config: {} bytes", config.len());
}
