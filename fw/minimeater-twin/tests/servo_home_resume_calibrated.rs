//! A calibrated unit that boots into a cook it is resuming never moves the flap to look for its stops (`minimeat-xdn`).

use minimeater_app::config::{Mode, ServoPulses};
use minimeater_app::pit::Event;
use minimeater_sim::hal::ServoMech;
use minimeater_twin::{Opts, Scenario, run};

#[test]
fn a_resumed_cook_on_a_calibrated_unit_never_homes() {
    let cal = ServoPulses {
        closed_us: 1130,
        open_us: 1870,
    };
    let mut sc = Scenario::kamado_hold(1.0);
    sc.hours = 0.1;
    sc.servo = ServoMech {
        closed_us: 1130.0,
        open_us: 1870.0,
        ..ServoMech::default()
    };
    sc.settings.servo = Some(cal);
    sc.settings.cook.mode = Mode::Run;
    sc.settings.cook.sp_f = 250.0;
    sc.actions.clear();
    let r = run(sc, &Opts { speed: None }, |_, _| {});
    assert!(
        !r.events.iter().any(|(_, e)| matches!(e, Event::ServoHome(_))),
        "{:?}",
        r.events
    );
    assert!(r.configs.iter().all(|(_, s)| s.servo == Some(cal)));
}
