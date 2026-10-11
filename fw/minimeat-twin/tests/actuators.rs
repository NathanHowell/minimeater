//! The actuator drivers on the twin (`.4.8`): the servo follows the controller's flap through the
//! calibration, the tach reads what the fan does, and a jammed fan is a fault until it is freed.
//! One run (the twin is one per process) checks all of it.

use minimeat_app::pit::{Event, FaultCode};
use minimeat_sim::hal::Faults;
use minimeat_twin::{Action, Opts, Scenario, run};

#[test]
fn the_drivers_on_a_light_off_with_a_fan_jam() {
    let mut sc = Scenario::kamado_brisket();
    sc.hours = 1.5;
    // light-off runs the fan hard from the start; jam it for ten minutes
    sc.actions.retain(|(t, _)| *t == 0.0);
    let jam = Faults {
        fan_jam: true,
        ..Faults::default()
    };
    sc.actions.push((300.0, Action::Faults(jam)));
    sc.actions.push((900.0, Action::Faults(Faults::default())));
    let r = run(sc, &Opts { speed: None }, |_, _| {});

    let when = |f: fn(&Event) -> bool| r.events.iter().find(|(_, e)| f(e)).map(|(t, _)| *t);
    let raised = when(|e| matches!(e, Event::Fault(FaultCode::FanStall))).expect("fan_stall raised");
    let cleared = when(|e| matches!(e, Event::FaultClear(FaultCode::FanStall))).expect("fan_stall cleared");
    assert!((300.0..=320.0).contains(&raised), "raised at {raised} s");
    assert!((900.0..=920.0).contains(&cleared), "cleared at {cleared} s");
    let faults = |t: f64| r.samples.iter().find(|s| s.t_s >= t).unwrap().status.faults;
    assert!(faults(600.0).contains(FaultCode::FanStall));
    assert!(!faults(1200.0).contains(FaultCode::FanStall));

    // the tach read the fan running before the jam, and stopped during it
    let rpm = |t: f64| r.samples.iter().find(|s| s.t_s >= t).unwrap().status.fan_rpm;
    assert!(rpm(200.0).is_some_and(|v| v > 500.0), "{:?}", rpm(200.0));
    assert!(rpm(600.0).is_some_and(|v| v < 1.0), "{:?}", rpm(600.0));

    // the sim servo has its own ground-truth calibration, so a wrong pulse width, a missed power-up
    // or a bad slew shows as a flap that is not where the controller put it. The flap at a sample
    // is the previous period's command: the actuators follow within a couple of seconds.
    let (mut moved, mut worst): (u32, f64) = (0, 0.0);
    for w in r.samples.windows(2) {
        let want = w[0].status.out.flap_pct / 100.0 * 90.0;
        worst = worst.max((w[1].flap_deg - want).abs());
        moved += u32::from(want > 1.0);
    }
    assert!(moved > 100, "the flap opened for {moved} periods");
    assert!(worst < 0.6, "flap {worst:.2} deg from the command");
}
