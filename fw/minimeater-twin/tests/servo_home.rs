//! Hard-stop homing on the twin (`minimeat-xdn`): a unit with no stored calibration drives its servo into both stops at
//! power-on, reads the stall on the load switches' fault line, and saves the pulses it found. The sim's servo has
//! hard stops at its true pulses (here 1130 and 1870 us, not the nominal 1000 and 2000) and stalls, fault line low, once
//! pushed 30 us past one.

use minimeater_app::config::Mode;
use minimeater_app::pit::{Command, FaultCode};
use minimeater_sim::hal::ServoMech;
use minimeater_twin::{Action, Opts, Scenario, run};

fn line(t: f64, text: &'static str) -> (f64, Action) {
    (t, Action::Serial(text))
}

#[test]
fn a_bare_unit_homes_itself_at_power_on_and_the_console_can_do_it_again() {
    let mut sc = Scenario::kamado_hold(1.0);
    sc.hours = 0.4;
    sc.servo = ServoMech {
        closed_us: 1130.0,
        open_us: 1870.0,
        ..ServoMech::default()
    };
    sc.settings.servo = None;
    sc.settings.cook.mode = Mode::Off;
    sc.actions.clear();
    sc.actions.extend([
        // after the run: a manual cook at full travel is on the stops it found, 90 degrees exactly
        (120.0, Action::Command(Command::Mode(Mode::Manual))),
        (120.0, Action::Command(Command::ManualFlap(100.0))),
        (160.0, Action::Command(Command::ManualFlap(0.0))),
        (200.0, Action::Command(Command::Mode(Mode::Off))),
        // and again from the console, on a calibrated unit, which is refused while a cook runs but not now
        line(210.0, "servo home\r"),
        line(300.0, "servo\r"),
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
    let flap_at = |t: f64| r.samples.iter().rev().find(|s| s.t_s <= t).unwrap().flap_deg;

    // saved within the watchdog-sized window, and close to the true stops (the lead is an estimate, so a few us)
    let (when, found) = r
        .configs
        .iter()
        .find_map(|(t, s)| s.servo.map(|p| (*t, p)))
        .expect("the run saved a calibration");
    println!("homing saved {found:?} at {when:.1} s (true stops 1130 and 1870)");
    assert!(when < 60.0, "homed by {when} s");
    assert!(
        (f64::from(found.closed_us) - 1130.0).abs() <= 15.0 && (f64::from(found.open_us) - 1870.0).abs() <= 15.0,
        "found {found:?}"
    );
    // the watchdog was fed throughout: the run takes most of its 30 s
    assert!(
        r.wdt.longest_gap_s < 30.0,
        "longest gap between feeds {} s",
        r.wdt.longest_gap_s
    );
    // no load fault from the stalls: the load watch stays out of homing
    assert!(
        !r.events
            .iter()
            .any(|(_, e)| matches!(e, minimeater_app::pit::Event::Fault(FaultCode::Load))),
        "events {:?}",
        r.events
    );
    // and the flap uses what was found
    assert!((flap_at(150.0) - 90.0).abs() < 3.0, "open: {}", flap_at(150.0));
    assert!((flap_at(190.0) - 0.0).abs() < 3.0, "closed: {}", flap_at(190.0));

    let console = said(210.0, 300.0);
    assert!(console.contains("homing against the stops"), "{console}");
    assert!(console.contains("homed and saved: closed 11"), "{console}");
    assert!(
        said(300.0, 310.0).contains("calibrated: closed 11"),
        "{:?}",
        said(300.0, 310.0)
    );
}
