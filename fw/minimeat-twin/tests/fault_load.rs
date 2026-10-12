//! A load switch trips (a stalled servo, a jammed fan, a short): the controller raises `load` and cuts both loads for
//! good, with no retry loop (ADR 0004). The flap holds where it was, the fan stops, the probes keep reporting.

use minimeat_app::pit::{Event, FaultCode};
use minimeat_sim::hal::Faults;
use minimeat_twin::{Action, Opts, Scenario, run};

#[test]
fn a_tripped_load_switch_cuts_both_loads_until_reboot() {
    let from = 2.0 * 3600.0;
    let mut sc = Scenario::kamado_hold(3.0);
    sc.actions.push((
        from,
        Action::Faults(Faults {
            load_fault: true,
            ..Faults::default()
        }),
    ));
    // the line goes quiet again: the latch stays
    sc.actions.push((from + 600.0, Action::Faults(Faults::default())));
    let r = run(sc, &Opts { speed: None }, |_, _| {});

    let raised = r
        .events
        .iter()
        .find(|(_, e)| matches!(e, Event::Fault(FaultCode::Load)))
        .map(|(t, _)| *t)
        .expect("load fault raised");
    assert!((from..from + 10.0).contains(&raised), "raised at +{:.0} s", raised - from);
    assert!(
        !r.events.iter().any(|(_, e)| matches!(e, Event::FaultClear(FaultCode::Load))),
        "it is latched"
    );
    let held = r.samples.iter().find(|s| s.t_s >= raised + 5.0).unwrap().flap_deg;
    for s in r.samples.iter().filter(|s| s.t_s > raised + 5.0) {
        assert_eq!(s.flap_deg, held, "flap moved at +{:.0} s", s.t_s - from);
        assert!(s.status.faults.contains(FaultCode::Load));
    }
}
