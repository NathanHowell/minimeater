//! The pit probe pulled mid-hold (`.4.24`'s acceptance): the flap closes and the fan stops within 35 s (20 s of grace in 10 s periods, then the servo), the cook
//! resumes when the probe is plugged back in, and the control task feeds the hardware watchdog all the while. One run:
//! the twin runs once per process.

use minimeater_app::pit::{Event, FaultCode, Phase};
use minimeater_twin::{Opts, Scenario, run};

#[test]
fn pulling_the_pit_probe_closes_the_flap_and_stops_the_fan_and_the_cook_resumes() {
    let (from, back) = (3.0 * 3600.0, 3.0 * 3600.0 + 300.0);
    let sc = Scenario::named("probe-unplug").unwrap();
    let r = run(sc, &Opts { speed: None }, |_, _| {});

    // the hold had the flap open before the probe went
    let before = r.samples.iter().rev().find(|s| s.t_s < from).unwrap();
    assert!(
        before.flap_deg > 0.0 || before.status.out.act.fan > 0.0,
        "{:?}",
        before.status.out
    );
    let safe = r
        .samples
        .iter()
        .find(|s| s.t_s >= from && s.flap_deg < 1.0 && s.status.out.act.fan == 0.0)
        .expect("flap closed and fan stopped");
    // PROBE_GRACE_S (20 s) is counted in whole control periods, so the third period without the probe is the one that
    // fails safe: 20-30 s after the pull, plus the servo's slew to closed
    assert!(safe.t_s - from <= 35.0, "safe at +{:.2} s", safe.t_s - from);
    let raised = r
        .events
        .iter()
        .find(|(_, e)| matches!(e, Event::Fault(FaultCode::PitProbe)))
        .map(|(t, _)| *t)
        .expect("pit_probe fault raised");
    assert!(
        (from + 20.0..from + 32.0).contains(&raised),
        "raised at +{:.2} s",
        raised - from
    );
    // and it stays that way until the probe is back
    for s in r.samples.iter().filter(|s| s.t_s > from + 30.0 && s.t_s < back) {
        assert_eq!(s.status.out.phase, Phase::Fault, "t {}", s.t_s - from);
        assert!(s.flap_deg < 1.0 && s.status.out.act.fan == 0.0, "t {}", s.t_s - from);
    }
    // plugged back in: the fault clears and the controller holds the pit again
    let cleared = r
        .events
        .iter()
        .find(|(_, e)| matches!(e, Event::FaultClear(FaultCode::PitProbe)))
        .map(|(t, _)| *t)
        .expect("pit_probe fault cleared");
    assert!(
        (back..back + 60.0).contains(&cleared),
        "cleared at +{:.0} s",
        cleared - back
    );
    let end = r.samples.last().unwrap();
    assert!(
        matches!(end.status.out.phase, Phase::Hold),
        "{:?}",
        end.status.out.phase
    );

    // the control task armed the hardware watchdog and kept feeding it through all of it (a period is 10 s)
    assert_eq!(r.wdt.timeout_s, Some(30.0), "armed with the 30 s timeout");
    assert!(!r.wdt.barked, "{:?}", r.wdt);
    assert!(r.wdt.longest_gap_s < 15.0, "{:?}", r.wdt);
}
