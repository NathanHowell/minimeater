//! A cook that starts while the servo is homing does not stop it (`minimeat-ixz`): the run takes about 30 s, and a cook on
//! the nominal pulses could push the flap against a post and trip the load switch. The run finishes and saves what it
//! found, `servo_home` reports it, and the flap then goes where the controller wants it.

use minimeater_app::config::Mode;
use minimeater_app::pit::{Command, Event, FaultCode};
use minimeater_sim::hal::ServoMech;
use minimeater_twin::{Action, Opts, Scenario, run};

#[test]
fn a_cook_started_during_homing_waits_for_it() {
    let mut sc = Scenario::kamado_hold(1.0);
    sc.hours = 0.1;
    sc.servo = ServoMech {
        closed_us: 1130.0,
        open_us: 1870.0,
        ..ServoMech::default()
    };
    sc.settings.servo = None;
    sc.settings.cook.mode = Mode::Off;
    sc.actions.clear();
    // homing begins at about 2 s (the supply has to be seen good first): the cook starts in the middle of the low sweep
    sc.actions.push((8.0, Action::Command(Command::Mode(Mode::Manual))));
    sc.actions.push((8.0, Action::Command(Command::ManualFlap(100.0))));
    let r = run(sc, &Opts { speed: None }, |_, _| {});
    let (when, found) = r
        .configs
        .iter()
        .find_map(|(t, s)| s.servo.map(|p| (*t, p)))
        .expect("the run finished and saved a calibration");
    assert!(
        (f64::from(found.closed_us) - 1130.0).abs() <= 15.0 && (f64::from(found.open_us) - 1870.0).abs() <= 15.0,
        "found {found:?} at {when:.1} s"
    );
    assert!(
        r.events.iter().any(|(_, e)| *e == Event::ServoHome(Ok(found))),
        "events {:?}",
        r.events
    );
    // no load fault, and never reported uncalibrated: the run was always going to finish
    assert!(
        !r.events
            .iter()
            .any(|(_, e)| matches!(e, Event::Fault(FaultCode::Load) | Event::Fault(FaultCode::ServoCal))),
        "events {:?}",
        r.events
    );
    let end = r.samples.last().unwrap().flap_deg;
    assert!((end - 90.0).abs() < 3.0, "the controller has the flap: {end}");
}
