//! The serial REPL on the twin (`minimeat-8h6`): a terminal types at the USB port, the app's own Improv/REPL loop reads
//! it, sends requests to the running control task, and prints the answers. Checks both what was printed and what the
//! running app then did.

use minimeater_twin::{Action, Opts, Scenario, run};

fn line(t: f64, text: &'static str) -> (f64, Action) {
    (t, Action::Serial(text))
}

#[test]
fn typed_commands_reach_the_running_app_and_are_answered() {
    let mut sc = Scenario::kamado_hold(1.0);
    sc.hours = 0.2;
    sc.actions.extend([
        line(60.0, "setpoint 275\r"),
        line(70.0, "setpoint 999\r"),
        line(80.0, "status\r"),
        line(90.0, "config\r"),
        line(100.0, "wifi set egg-net hunter22\r"),
        line(110.0, "mqtt set broker.lan 8883 mm pw\r"),
        line(120.0, "config\r"),
        line(130.0, "factory-reset\r"),
        line(140.0, "mqtt ca\r"),
        line(150.0, "identity\r"),
        line(180.0, "nonsense\r"),
        line(190.0, "help\r"),
    ]);
    let r = run(sc, &Opts { speed: None }, |_, _| {});

    let said = |from: f64, to: f64| {
        let mut b = Vec::new();
        for (t, bytes) in &r.serial {
            if (from..to).contains(t) {
                b.extend_from_slice(bytes);
            }
        }
        String::from_utf8_lossy(&b).into_owned()
    };
    // Improv announced itself first, and the REPL shares its bytes
    assert!(r.serial[0].1.starts_with(b"IMPROV"), "{:?}", r.serial[0]);

    assert!(
        said(60.0, 70.0).contains("minimeater> "),
        "the prompt comes back: {:?}",
        said(60.0, 70.0)
    );
    assert!(said(70.0, 80.0).contains("out of range"), "{:?}", said(70.0, 80.0));
    assert!(
        said(80.0, 90.0).contains("275"),
        "status shows the new setpoint: {:?}",
        said(80.0, 90.0)
    );
    let config = said(120.0, 130.0);
    assert!(
        config.contains("egg-net") && config.contains("broker.lan:8883"),
        "{config}"
    );
    assert!(
        !config.contains("hunter22") && !config.contains("pw\r"),
        "passwords are never printed: {config}"
    );
    assert!(
        said(130.0, 140.0).contains("factory-reset erase"),
        "{:?}",
        said(130.0, 140.0)
    );
    assert!(said(140.0, 150.0).contains("tls off"), "{:?}", said(140.0, 150.0));
    assert!(
        said(150.0, 175.0).contains("no answer from the secure element"),
        "the twin has no chip: {:?}",
        said(150.0, 175.0)
    );
    assert!(
        said(180.0, 190.0).contains("unknown command"),
        "{:?}",
        said(180.0, 190.0)
    );
    assert!(said(190.0, 200.0).contains("setpoint"), "help lists the commands");

    // what the app did: the accepted setpoint and the credentials, in the settings it published; the rejected one nowhere
    let last = &r.configs.last().expect("a config went out").1;
    assert_eq!(last.cook.sp_f, 275.0);
    assert_eq!(last.wifi.as_ref().map(|w| w.ssid.as_str()), Some("egg-net"));
    assert_eq!(
        last.mqtt.as_ref().map(|m| (m.host.as_str(), m.port)),
        Some(("broker.lan", 8883))
    );
    assert!(r.configs.iter().all(|(_, c)| c.cook.sp_f != 999.0));
    let s = r.samples.iter().find(|s| s.t_s >= 85.0).unwrap();
    assert_eq!(s.status.cook.sp_f, 275.0);
}
