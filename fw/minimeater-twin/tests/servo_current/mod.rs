//! Shared by the servo current twin tests (`minimeat-y8a`): a hold at 225F with a fault injected from 2 h to 2.5 h.

use minimeater_app::pit::{Command, Event, FaultCode, Phase};
use minimeater_sim::hal::Faults;
use minimeater_twin::{Action, Opts, Report, Scenario, run};

const H: f64 = 3600.0;

/// A hold at 225F with `fault` from 2 h to 2.5 h and the setpoint up to 275F a minute in, so the flap must move.
pub fn with(fault: Faults) -> Report {
    let mut sc = Scenario::kamado_hold(3.0);
    sc.actions.push((2.0 * H, Action::Faults(fault)));
    sc.actions.push((2.0 * H + 60.0, Action::Command(Command::Sp(275.0))));
    sc.actions.push((2.5 * H, Action::Faults(Faults::default())));
    run(sc, &Opts { speed: None }, |_, _| {})
}

#[allow(dead_code)]
pub fn when(r: &Report, f: impl Fn(&Event) -> bool) -> Option<f64> {
    r.events.iter().find(|(_, e)| f(e)).map(|(t, _)| *t)
}

#[allow(dead_code)]
pub fn check(name: &str, r: &Report) {
    let raised = when(r, |e| matches!(e, Event::Fault(FaultCode::Servo))).expect("servo raised");
    let cleared = when(r, |e| matches!(e, Event::FaultClear(FaultCode::Servo))).expect("servo cleared");
    println!(
        "{name}: raised at +{:.0} s, cleared {:.0} s after the fix",
        raised - 2.0 * H,
        cleared - 2.5 * H
    );
    assert!((2.0 * H..2.0 * H + 600.0).contains(&raised), "raised at {raised}");
    assert!((2.5 * H..2.5 * H + 900.0).contains(&cleared), "cleared at {cleared}");
    assert!(
        r.events
            .iter()
            .all(|(_, e)| !matches!(e, Event::Fault(FaultCode::Load))),
        "{:?}",
        r.events
    );
    for s in r.samples.iter().filter(|s| s.t_s > 2.0 * H) {
        assert!(s.status.out.phase != Phase::Fault, "no fault phase at {}", s.t_s);
    }
    let end = r.samples.last().unwrap();
    assert!((end.pit_f - 275.0).abs() < 5.0, "ended at {:.1}F", end.pit_f);
}
