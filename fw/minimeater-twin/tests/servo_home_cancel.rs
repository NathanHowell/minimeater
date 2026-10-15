//! A cook that starts while the servo is homing ends the run (`minimeat-xdn`): the calibration stays unset, the console
//! says why, and the flap goes where the controller wants it.

use minimeater_app::config::Mode;
use minimeater_app::pit::Command;
use minimeater_twin::{Action, Opts, Scenario, run};

#[test]
fn starting_a_cook_stops_the_homing_and_saves_nothing() {
    let mut sc = Scenario::kamado_hold(1.0);
    sc.hours = 0.1;
    sc.settings.servo = None;
    sc.settings.cook.mode = Mode::Off;
    sc.actions.clear();
    // homing begins at about 2 s (the supply has to be seen good first) and takes 15 s: the cook starts in the middle of the high sweep
    sc.actions.push((8.0, Action::Command(Command::Mode(Mode::Manual))));
    sc.actions.push((8.0, Action::Command(Command::ManualFlap(100.0))));
    sc.actions.push((100.0, Action::Serial("servo\r")));
    let r = run(sc, &Opts { speed: None }, |_, _| {});
    assert!(
        r.configs.iter().all(|(_, s)| s.servo.is_none()),
        "no calibration was saved: {:?}",
        r.configs.last().map(|c| c.1.servo)
    );
    let said: Vec<u8> = r
        .serial
        .iter()
        .filter(|(t, _)| *t >= 100.0)
        .flat_map(|(_, b)| b.clone())
        .collect();
    let said = String::from_utf8_lossy(&said);
    assert!(said.contains("not calibrated"), "{said}");
    assert!(said.contains("the last homing failed: stopped"), "{said}");
    let end = r.samples.last().unwrap().flap_deg;
    assert!((end - 90.0).abs() < 3.0, "the controller has the flap: {end}");
}
