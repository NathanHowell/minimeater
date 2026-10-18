//! Homing that finds no posts (`minimeat-bhp`): the servo's own end stops are past the sweep's limits, as with the posts
//! missing or the linkage off. Nothing is saved, `servo_home` says why, and `servo_cal` is raised while the unit runs on
//! the nominal pulses.

use minimeater_app::config::Mode;
use minimeater_app::drive::HomeFail;
use minimeater_app::pit::{Event, FaultCode};
use minimeater_sim::hal::ServoMech;
use minimeater_twin::{Opts, Scenario, run};

#[test]
fn no_posts_is_reported_and_leaves_servo_cal_raised() {
    let mut sc = Scenario::kamado_hold(1.0);
    sc.hours = 0.1;
    sc.servo = ServoMech {
        closed_us: 600.0,
        open_us: 2400.0,
        ..ServoMech::default()
    };
    sc.settings.servo = None;
    sc.settings.cook.mode = Mode::Off;
    sc.actions.clear();
    let r = run(sc, &Opts { speed: None }, |_, _| {});
    assert!(r.configs.iter().all(|(_, s)| s.servo.is_none()));
    let failed = r
        .events
        .iter()
        .find_map(|(t, e)| matches!(e, Event::ServoHome(Err(HomeFail::NoStopLow))).then_some(*t))
        .unwrap_or_else(|| panic!("events {:?}", r.events));
    let raised = r
        .events
        .iter()
        .find_map(|(t, e)| (*e == Event::Fault(FaultCode::ServoCal)).then_some(*t))
        .unwrap_or_else(|| panic!("events {:?}", r.events));
    // raised once the run has ended, not while it was still to come
    assert!(raised >= failed, "servo_cal at {raised} s, the run ended at {failed} s");
    // and stays raised: nothing calibrates it
    assert!(
        !r.events
            .iter()
            .any(|(_, e)| *e == Event::FaultClear(FaultCode::ServoCal)),
        "{:?}",
        r.events
    );
}
