//! A unit that boots into a cook it is resuming homes first if it has no calibration (`minimeat-ixz`): about 30 s with
//! the fan off beats a cook on pulses that may push the flap against a post (`servo_home_resume_calibrated.rs`: a
//! calibrated one never moves the flap to look).

use minimeater_app::config::Mode;
use minimeater_app::pit::Event;
use minimeater_sim::hal::ServoMech;
use minimeater_twin::{Opts, Scenario, run};

fn resumed() -> minimeater_twin::Report {
    let mut sc = Scenario::kamado_hold(1.0);
    sc.hours = 0.1;
    sc.servo = ServoMech {
        closed_us: 1130.0,
        open_us: 1870.0,
        ..ServoMech::default()
    };
    sc.settings.servo = None;
    // the cook in flash
    sc.settings.cook.mode = Mode::Run;
    sc.settings.cook.sp_f = 250.0;
    sc.actions.clear();
    run(sc, &Opts { speed: None }, |_, _| {})
}

#[test]
fn a_resumed_cook_with_no_calibration_homes_first() {
    let r = resumed();
    let (when, found) = r
        .configs
        .iter()
        .find_map(|(t, s)| s.servo.map(|p| (*t, p)))
        .expect("the run saved a calibration");
    assert!(when < 60.0, "homed by {when} s");
    assert!(
        (f64::from(found.closed_us) - 1130.0).abs() <= 15.0 && (f64::from(found.open_us) - 1870.0).abs() <= 15.0,
        "found {found:?}"
    );
    assert!(r.events.iter().any(|(_, e)| *e == Event::ServoHome(Ok(found))));
}
