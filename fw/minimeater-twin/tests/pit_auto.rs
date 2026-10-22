//! `pit_probe: auto` on the twin (ADR 0008): with a brisket in jack 0 and the pit probe in jack 2, the loop finds the
//! pit probe on its own, locks it, and holds the setpoint on it. One run: the twin runs once per process.

use minimeater_app::pit::{Event, FaultCode};
use minimeater_core::thermistor::Preset;
use minimeater_sim::Meat;
use minimeater_sim::hal::Wiring;
use minimeater_twin::{Action, Opts, Scenario, run};

#[test]
fn the_pit_probe_in_jack_two_among_food_is_found_and_held() {
    let th = Preset::ThermoworksPro.thermistor();
    let mut sc = Scenario::kamado_hold(2.0);
    sc.jacks = [Wiring::Food(0, th), Wiring::Empty, Wiring::Pit(th), Wiring::Empty];
    sc.actions.push((
        0.0,
        Action::Meat {
            slot: 0,
            meat: Some(Meat::brisket()),
        },
    ));
    let r = run(sc, &Opts { speed: None }, |_, _| {});

    let found = r
        .events
        .iter()
        .find_map(|(t, e)| matches!(e, Event::PitFound { probe: 2 }).then_some(*t))
        .expect("jack 2 found");
    assert!(found < 20.0 * 60.0, "found at {:.1} min", found / 60.0);
    assert!(
        !r.events
            .iter()
            .any(|(_, e)| matches!(e, Event::PitFound { probe } if *probe != 2)),
        "{:?}",
        r.events
    );
    assert!(
        !r.events
            .iter()
            .any(|(_, e)| matches!(e, Event::Fault(FaultCode::PitMismatch))),
        "no mismatch"
    );

    let end = r.samples.last().unwrap();
    assert_eq!(end.status.cook.pit_jack(), Some(2));
    // the cook holds at the setpoint on the jack it found
    for s in r.samples.iter().filter(|s| s.t_s > 1.5 * 3600.0) {
        assert!((s.pit_f - 250.0).abs() < 8.0, "t {:.0} s: pit {:.1}", s.t_s, s.pit_f);
    }
    assert!(end.status.pit_f.is_some_and(|y| (y - 250.0).abs() < 8.0));
}
