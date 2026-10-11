//! Fault injection (`.4.16.4`): the probe ADC dies for three minutes in a hold. The controller
//! must fail safe (flap closed, fan off) once the pit reading is gone for good, say so, and carry
//! on when it is back.

use minimeat_app::pit::{Event, FaultCode, Phase};
use minimeat_sim::hal::Faults;
use minimeat_twin::{Action, Opts, Scenario, run};

#[test]
fn a_dead_adc_fails_safe_and_the_cook_resumes() {
    let (from, to, h) = (2.0 * 3600.0, 2.0 * 3600.0 + 180.0, 3600.0);
    let mut sc = Scenario::kamado_hold(4.0);
    sc.actions.push((
        from,
        Action::Faults(Faults {
            adc_down: true,
            ..Faults::default()
        }),
    ));
    sc.actions.push((to, Action::Faults(Faults::default())));
    let r = run(sc, &Opts { speed: None }, |_, _| {});

    let when = |f: fn(&Event) -> bool| r.events.iter().find(|(_, e)| f(e)).map(|(t, _)| *t);
    let adc = when(|e| matches!(e, Event::Fault(FaultCode::Adc))).expect("adc fault raised");
    let pit = when(|e| matches!(e, Event::Fault(FaultCode::PitProbe))).expect("pit probe fault raised");
    assert!((from..from + 20.0).contains(&adc), "adc fault at +{:.0} s", adc - from);
    assert!(
        (from + 20.0..from + 60.0).contains(&pit),
        "pit fault at +{:.0} s",
        pit - from
    );
    // the pit controller gives the reading 20 s of grace; after that nothing is driven
    for s in r.samples.iter().filter(|s| s.t_s > from + 60.0 && s.t_s < to) {
        assert_eq!(s.status.out.phase, Phase::Fault, "t {}", s.t_s - from);
        assert_eq!(
            (s.status.out.flap_pct, s.status.out.act.fan),
            (0.0, 0.0),
            "t {}",
            s.t_s - from
        );
        assert_eq!(s.status.temps, [None; 4]);
    }
    // back: both faults clear within a pass or two, and the pit is where it was
    let back = when(|e| matches!(e, Event::FaultClear(FaultCode::Adc))).expect("adc fault cleared");
    assert!((to..to + 30.0).contains(&back), "cleared at +{:.0} s", back - to);
    let after = r.samples.iter().find(|s| s.t_s >= to + 30.0 * 60.0).unwrap();
    assert!(after.status.faults.iter().next().is_none(), "{:?}", after.status.faults);
    assert!(
        matches!(after.status.out.phase, Phase::Hold),
        "{:?}",
        after.status.out.phase
    );
    assert!(
        (after.pit_f - 250.0).abs() < 4.0,
        "pit {:.1}F half an hour on",
        after.pit_f
    );
    // and it held 250F before
    let before = r.samples.iter().rev().find(|s| s.t_s < from).unwrap();
    assert!(
        (before.pit_f - 250.0).abs() < 3.0 && before.t_s > h,
        "{:.1}F",
        before.pit_f
    );
}
