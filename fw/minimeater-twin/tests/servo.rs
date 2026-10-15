//! The damper servo's bring-up on the twin (`minimeat-bkz`): a servo whose pulse range is not the nominal 1 to 2 ms is
//! jogged to its two stops from the serial console, the stops are marked and saved, and the flap then travels exactly
//! its 90 degrees on the pulses the app computes. The sim's servo is the ground truth: its flap angle follows the pulse
//! it really got.

use minimeater_app::config::{Mode, ServoPulses};
use minimeater_app::pit::Command;
use minimeater_sim::hal::ServoMech;
use minimeater_twin::{Action, Opts, Scenario, run};

fn line(t: f64, text: &'static str) -> (f64, Action) {
    (t, Action::Serial(text))
}

#[test]
fn the_console_finds_the_stops_and_the_flap_then_uses_them() {
    let mut sc = Scenario::kamado_hold(1.0);
    sc.hours = 0.5;
    // this servo travels 1130 to 1870 us, not the nominal 1000 to 2000
    sc.servo = ServoMech {
        closed_us: 1130.0,
        open_us: 1870.0,
        ..ServoMech::default()
    };
    // no cook yet: the bring-up happens on a bench
    sc.actions.clear();
    sc.settings.cook.mode = Mode::Off;
    sc.actions.extend([
        line(60.0, "servo\r"),
        line(70.0, "servo mark closed\r"),
        line(80.0, "servo jog 1130\r"),
        line(85.0, "servo mark closed\r"),
        line(90.0, "servo jog +370\r"),
        line(100.0, "servo jog 3000\r"),
        line(110.0, "servo jog 1870\r"),
        line(115.0, "servo save\r"),
        line(120.0, "servo mark open\r"),
        line(130.0, "servo save\r"),
        line(150.0, "config\r"),
        line(160.0, "servo\r"),
        // a manual cook at full travel shows where the app puts the flap now, and a cook is no time to jog
        (165.0, Action::Command(Command::Mode(Mode::Manual))),
        (165.0, Action::Command(Command::ManualFlap(100.0))),
        line(200.0, "servo jog 1200\r"),
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
    let flap_at = |t: f64| {
        // the last sample at or before `t`
        r.samples.iter().rev().find(|s| s.t_s <= t).unwrap().flap_deg
    };

    assert!(
        said(60.0, 70.0).contains("calibrated: closed 1000 us, open 2000 us"),
        "{:?}",
        said(60.0, 70.0)
    );
    assert!(
        said(70.0, 80.0).contains("jog the servo there first"),
        "{:?}",
        said(70.0, 80.0)
    );
    assert!(said(80.0, 90.0).contains("holding 1130 us"), "{:?}", said(80.0, 90.0));
    assert!(
        said(90.0, 100.0).contains("holding 1500 us"),
        "a step from the held pulse: {:?}",
        said(90.0, 100.0)
    );
    assert!(said(100.0, 110.0).contains("500 to 2500"), "{:?}", said(100.0, 110.0));
    // the servo really went where it was told, not through the app's idea of the angle
    assert!((flap_at(88.0) - 0.0).abs() < 1.0, "closed stop: {}", flap_at(88.0));
    assert!((flap_at(98.0) - 45.0).abs() < 1.0, "half way: {}", flap_at(98.0));
    assert!((flap_at(114.0) - 90.0).abs() < 1.0, "open stop: {}", flap_at(114.0));
    // the second save has both marks
    assert!(
        said(115.0, 120.0).contains("mark both closed and open"),
        "{:?}",
        said(115.0, 120.0)
    );
    assert!(said(130.0, 150.0).contains("saved"), "{:?}", said(130.0, 150.0));
    assert!(
        said(150.0, 160.0).contains("servo    closed 1130 us, open 1870 us"),
        "{:?}",
        said(150.0, 160.0)
    );
    assert!(
        said(160.0, 170.0).contains("calibrated: closed 1130 us, open 1870 us"),
        "{:?}",
        said(160.0, 170.0)
    );
    assert!(said(160.0, 170.0).contains("not holding"), "saving ends the jog");
    // 1870 us is this servo's open stop; on the nominal 1 to 2 ms it would be 78 degrees
    assert!(
        (flap_at(195.0) - 90.0).abs() < 1.0,
        "full open on the saved stops: {}",
        flap_at(195.0)
    );
    assert!(
        said(200.0, 215.0).contains("not while a cook"),
        "{:?}",
        said(200.0, 215.0)
    );

    // the settings reached `~/config`
    let last = r.configs.last().expect("a config went out");
    assert_eq!(
        last.1.servo,
        Some(ServoPulses {
            closed_us: 1130,
            open_us: 1870
        })
    );
}
