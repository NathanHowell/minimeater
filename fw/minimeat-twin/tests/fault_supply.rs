//! A USB-C source that advertises less than 1.5 A (ADR 0004): the fan and the servo are off, the controller says
//! `supply`, and everything resumes when a proper source is back. There is no reduced mode.

use minimeat_app::pit::{Command, Event, FaultCode};
use minimeat_twin::{Action, Opts, Scenario, run};

#[test]
fn a_default_class_source_stops_the_actuators_and_says_so() {
    let h = 3600.0;
    let (from, to) = (2.0 * h, 2.0 * h + 1200.0);
    let mut sc = Scenario::kamado_hold(4.0);
    sc.actions.push((from, Action::Usb(Some(500))));
    // a setpoint change the flap would follow if it were powered
    sc.actions.push((from + 60.0, Action::Command(Command::Sp(300.0))));
    sc.actions.push((to, Action::Usb(Some(1500))));
    let r = run(sc, &Opts { speed: None }, |_, _| {});

    let at = |f: fn(&Event) -> bool| r.events.iter().find(|(_, e)| f(e)).map(|(t, _)| *t);
    assert!(
        !r.events.iter().any(|(t, e)| *t < from && matches!(e, Event::Fault(FaultCode::Supply))),
        "a proper source raises nothing"
    );
    let raised = at(|e| matches!(e, Event::Fault(FaultCode::Supply))).expect("supply fault raised");
    assert!((from..from + 10.0).contains(&raised), "raised at +{:.0} s", raised - from);
    // from the moment it is raised the flap does not move
    let held = r.samples.iter().find(|s| s.t_s >= raised + 5.0).unwrap().flap_deg;
    for s in r.samples.iter().filter(|s| s.t_s > raised + 5.0 && s.t_s < to) {
        assert_eq!(s.flap_deg, held, "flap moved at +{:.0} s", s.t_s - from);
        assert!(s.status.faults.contains(FaultCode::Supply));
    }
    let cleared = at(|e| matches!(e, Event::FaultClear(FaultCode::Supply))).expect("supply fault cleared");
    assert!((to..to + 10.0).contains(&cleared), "cleared at +{:.0} s", cleared - to);
    let end = r.samples.last().unwrap();
    assert!(!end.status.faults.contains(FaultCode::Supply));
    assert!((end.pit_f - 300.0).abs() < 4.0, "ended at {:.1}F", end.pit_f);
}
