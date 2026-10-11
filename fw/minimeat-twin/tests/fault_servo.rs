//! Fault injection (`.4.16.4`): the damper servo sticks for twenty minutes while the setpoint goes
//! up. Nothing tells the firmware (there is no position feedback), so the reaction to assert is
//! that the controller carries on, does not wind up into a runaway, and finds the setpoint once
//! the flap is free.

use minimeat_app::config::Mode;
use minimeat_app::pit::{Command, Event, Phase};
use minimeat_sim::hal::Faults;
use minimeat_twin::{Action, Opts, Scenario, run};

#[test]
fn a_stuck_flap_costs_time_not_control() {
    let h = 3600.0;
    let (stick, free) = (2.0 * h, 2.0 * h + 1200.0);
    let mut sc = Scenario::kamado_hold(5.0);
    sc.actions.push((
        stick,
        Action::Faults(Faults {
            servo_stuck: true,
            ..Faults::default()
        }),
    ));
    sc.actions.push((stick + 60.0, Action::Command(Command::Sp(300.0))));
    sc.actions.push((free, Action::Faults(Faults::default())));
    let r = run(sc, &Opts { speed: None }, |_, _| {});

    let (mut peak, mut peak_t): (f64, f64) = (0.0, 0.0);
    for s in r.samples.iter().filter(|s| s.t_s > stick) {
        if s.pit_f > peak {
            (peak, peak_t) = (s.pit_f, s.t_s);
        }
        assert!(s.status.out.phase != Phase::Fault, "no fault at {}", s.t_s);
    }
    println!("peak {peak:.1}F at +{:.0} min after the stick", (peak_t - stick) / 60.0);
    assert!(peak < 312.0, "peak {peak:.1}F against a 300F setpoint");
    let end = r.samples.last().unwrap();
    assert!((end.pit_f - 300.0).abs() < 3.0, "ended at {:.1}F", end.pit_f);
    assert!(end.status.out.phase == Phase::Hold);
    assert!(
        r.events
            .iter()
            .all(|(_, e)| !matches!(e, Event::Fault(_) | Event::FireOut { .. })),
        "{:?}",
        r.events
    );
    assert_eq!(end.status.cook.mode, Mode::Run);
}
