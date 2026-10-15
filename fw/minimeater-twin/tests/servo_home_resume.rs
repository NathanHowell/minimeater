//! A unit that boots into a cook it is resuming (`minimeat-xdn`) does not home, calibrated or not: the flap is not
//! moved to look for stops with a cook in progress.

use minimeater_app::config::Mode;
use minimeater_twin::{Action, Opts, Scenario, run};

#[test]
fn a_resumed_cook_never_homes() {
    let mut sc = Scenario::kamado_hold(1.0);
    sc.hours = 0.1;
    sc.settings.servo = None;
    // the cook in flash
    sc.settings.cook.mode = Mode::Run;
    sc.settings.cook.sp_f = 250.0;
    sc.actions.clear();
    sc.actions.push((100.0, Action::Serial("servo\r")));
    let r = run(sc, &Opts { speed: None }, |_, _| {});
    // a run would have found and saved the sim's stops
    assert!(r.configs.iter().all(|(_, s)| s.servo.is_none()), "nothing was homed");
    let said: Vec<u8> = r
        .serial
        .iter()
        .filter(|(t, _)| *t >= 100.0)
        .flat_map(|(_, b)| b.clone())
        .collect();
    let said = String::from_utf8_lossy(&said);
    assert!(said.contains("not calibrated") && !said.contains("homing"), "{said}");
}
