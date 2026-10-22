//! A food probe chosen as the pit probe (ADR 0008): with the real pit probe in jack 2 and a brisket in jack 0 selected
//! by hand, the controller drives the fire on the brisket's slow climb, the mismatch watch trips (`slow`: the fan has run
//! an hour with the probe 40 F under the setpoint), and with `stop` the flap closes and the fan stops. One run: the twin
//! runs once per process.

use minimeater_app::config::PitSel;
use minimeater_app::pit::{Command, Event, FaultCode, Phase};
use minimeater_core::control::MismatchPolicy;
use minimeater_core::thermistor::Preset;
use minimeater_sim::Meat;
use minimeater_sim::hal::Wiring;
use minimeater_twin::{Action, Opts, Scenario, run};

#[test]
fn a_food_probe_picked_as_the_pit_trips_the_watch_and_stop_shuts_the_air() {
    let th = Preset::ThermoworksPro.thermistor();
    let mut sc = Scenario::kamado_hold(2.5);
    sc.jacks = [Wiring::Food(0, th), Wiring::Empty, Wiring::Pit(th), Wiring::Empty];
    sc.actions.extend([
        (
            0.0,
            Action::Meat {
                slot: 0,
                meat: Some(Meat::brisket()),
            },
        ),
        (0.0, Action::Command(Command::PitProbe(PitSel::Jack(0)))),
        (0.0, Action::Command(Command::PitMismatch(MismatchPolicy::Stop))),
    ]);
    let r = run(sc, &Opts { speed: None }, |_, _| {});

    let tripped = r
        .events
        .iter()
        .find_map(|(t, e)| matches!(e, Event::Fault(FaultCode::PitMismatch)).then_some(*t))
        .expect("pit_mismatch raised");
    assert!(
        (3600.0..3900.0).contains(&tripped),
        "tripped at {:.1} min",
        tripped / 60.0
    );
    // stopped: no air until the cook ends
    for s in r.samples.iter().filter(|s| s.t_s > tripped + 40.0) {
        assert_eq!(s.status.out.phase, Phase::Fault, "t {:.0}", s.t_s);
        assert!(s.flap_deg < 1.0 && s.status.out.act.fan == 0.0, "t {:.0}", s.t_s);
        assert!(s.status.faults.contains(FaultCode::PitMismatch));
        assert!(!s.status.faults.contains(FaultCode::PitProbe));
    }
}
