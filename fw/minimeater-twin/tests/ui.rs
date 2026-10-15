//! The status LED and the button (`.4.30`) on the twin, in one run (the twin runs once per process): `identify` blinks
//! for 10 s, an alarm blinks until a short press of the button acknowledges it (the alarm itself stays reported), and
//! a press held for 10 s clears the saved WiFi network and nothing else, and one held for 30 s resets every setting.

use minimeater_app::config::{Mode, Settings, Wifi};
use minimeater_twin::{Action, Opts, Scenario, run};

fn edges(r: &minimeater_twin::Report, from: f64, to: f64) -> u32 {
    let at = |t: f64| r.samples.iter().rev().find(|s| s.t_s <= t).map_or(0, |s| s.led_edges);
    at(to) - at(from)
}

#[test]
fn the_led_blinks_for_identify_and_alarms_and_the_button_acknowledges_and_clears_wifi() {
    let mut sc = Scenario::kamado_hold(1.0);
    sc.settings.wifi = Some(Wifi {
        ssid: "net".try_into().unwrap(),
        pass: "pw".try_into().unwrap(),
    });
    sc.actions.extend([
        (
            600.0,
            Action::Mqtt {
                id: 1,
                topic: "cmd/identify",
                payload: "",
            },
        ),
        // a high limit under the pit's 250 F
        (
            1200.0,
            Action::Mqtt {
                id: 2,
                topic: "set/probe/0/hi",
                payload: "200",
            },
        ),
        (1800.0, Action::Button(true)),
        (1800.2, Action::Button(false)),
        (2400.0, Action::Button(true)),
        (2411.0, Action::Button(false)),
        (3000.0, Action::Button(true)),
        (3031.0, Action::Button(false)),
    ]);
    let r = run(sc, &Opts { speed: None }, |_, _| {});

    // cooking: one flash every 3 s is two edges, and the status carries the samples
    let quiet = edges(&r, 300.0, 540.0);
    assert!((100..=170).contains(&quiet), "{quiet} edges in 240 s of cooking");
    // identify: 5 Hz for 10 s is about 100 edges, then back to the cook's flash
    let identify = edges(&r, 590.0, 620.0);
    assert!((90..=130).contains(&identify), "{identify} edges while identifying");
    assert!(edges(&r, 640.0, 700.0) < 60, "identify ended");

    // the alarm blinks at 2 Hz (4 edges a second) until the press
    let alarm_at = r
        .samples
        .iter()
        .find(|s| s.t_s > 1200.0 && s.status.alarm_led)
        .map(|s| s.t_s);
    assert!(
        alarm_at.is_some_and(|t| t < 1215.0),
        "alarm LED requested at {alarm_at:?}"
    );
    let blinking = edges(&r, 1500.0, 1560.0);
    assert!((200..=260).contains(&blinking), "{blinking} edges in 60 s of alarm");
    let acked = r
        .samples
        .iter()
        .find(|s| s.t_s > 1800.0 && !s.status.alarm_led)
        .map(|s| s.t_s);
    assert!(acked.is_some_and(|t| t < 1815.0), "acknowledged by {acked:?}");
    assert!(
        r.samples
            .iter()
            .filter(|s| s.t_s > 1815.0 && s.t_s < 3000.0)
            .all(|s| s.status.alarms[0].is_some()),
        "the alarm is still reported after the acknowledgement"
    );
    assert!(edges(&r, 1900.0, 2300.0) < 400, "back to the cook's flash");

    // the long press clears the network, and only that
    let (_, before) = r.configs.iter().rev().find(|(t, _)| *t < 2400.0).unwrap();
    assert!(before.wifi.is_some());
    let (_, after) = r.configs.iter().rev().find(|(t, _)| *t < 3000.0).unwrap();
    assert!(after.wifi.is_none(), "wifi cleared by the long press");
    assert_eq!(after.probes[0].hi_f, Some(200.0), "and nothing else");
    assert_ne!(after.cook.mode, Mode::Off);

    // 30 s: everything, as the first boot has it
    let (t, reset) = r.configs.last().unwrap();
    assert!((3030.0..3040.0).contains(t), "reset at {t}");
    assert_eq!(*reset, Settings::default());
}
