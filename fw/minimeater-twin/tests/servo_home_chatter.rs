//! Homing against a servo whose stall makes the fault line come and go (`minimeat-xdn`): the TPS2553 in current limit
//! pulls the servo's supply down to about the limit times the motor's resistance, which can reset a digital servo's logic,
//! and the line is then low for a few reads in every few steps. Counting the steps the line was seen low in a window finds
//! the stops all the same, if a little further in. The current sense is switched off here (`servo_sense_down`): with it
//! the stall is seen in the current before the switch limits, and the line is not needed.

use minimeater_app::config::Mode;
use minimeater_sim::hal::{Faults, ServoMech};
use minimeater_twin::{Action, Opts, Scenario, run};

#[test]
fn a_chattering_fault_line_still_homes() {
    let mut sc = Scenario::kamado_hold(1.0);
    sc.hours = 0.1;
    sc.servo = ServoMech {
        closed_us: 1130.0,
        open_us: 1870.0,
        // three low reads in every 40 while stalled: one step in four sees the line low
        chatter_reads: 40,
        ..ServoMech::default()
    };
    sc.settings.servo = None;
    sc.settings.cook.mode = Mode::Off;
    sc.actions.clear();
    sc.actions.push((
        0.0,
        Action::Faults(Faults {
            servo_sense_down: true,
            ..Faults::default()
        }),
    ));
    let r = run(sc, &Opts { speed: None }, |_, _| {});
    let (when, found) = r
        .configs
        .iter()
        .find_map(|(t, s)| s.servo.map(|p| (*t, p)))
        .expect("the run saved a calibration");
    println!("chattering line: homing saved {found:?} at {when:.1} s (true stops 1130 and 1870)");
    assert!(
        (f64::from(found.closed_us) - 1130.0).abs() <= 70.0 && (f64::from(found.open_us) - 1870.0).abs() <= 70.0,
        "found {found:?}"
    );
}
