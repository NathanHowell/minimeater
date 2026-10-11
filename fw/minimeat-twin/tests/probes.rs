//! The probe pipeline on the twin (`.4.7`): what each jack reports, the power-up self-check against
//! ambient, and a probe plugged in later.

use minimeat_app::alarms::Limit;
use minimeat_app::config::Mode;
use minimeat_app::pit::{Command, Event};
use minimeat_app::probes::{Check, ProbeSt};
use minimeat_core::thermistor::Preset;
use minimeat_sim::hal::Wiring;
use minimeat_twin::{Action, Opts, Scenario, run};

#[test]
fn jacks_report_their_state_and_the_power_up_check_flags_a_wrong_type() {
    let tw = Preset::ThermoworksPro.thermistor();
    let mut sc = Scenario::kamado_brisket();
    sc.hours = 0.2;
    sc.actions.clear();
    sc.settings.cook.mode = Mode::Off;
    // jack 0 is right; jack 1 is a Maverick the settings call a ThermoWorks; jack 2 is empty,
    // jack 3 is shorted and a ThermoWorks goes into jack 2 after 6 min
    sc.jacks = [
        Wiring::Pit(tw),
        Wiring::Pit(Preset::MaverickEt732.thermistor()),
        Wiring::Empty,
        Wiring::Short,
    ];
    sc.actions.push((
        360.0,
        Action::Wire {
            jack: 2,
            wiring: Wiring::Pit(tw),
        },
    ));
    // a limit the room is already over: the alarm rings from the first period, and `alarm_ack`
    // quietens the LED but not the report
    sc.settings.probes[0].hi_f = Some(60.0);
    sc.actions.push((120.0, Action::Command(Command::AlarmAck)));
    let r = run(sc, &Opts { speed: None }, |_, _| {});

    let checks: Vec<_> = r
        .events
        .iter()
        .filter_map(|(t, e)| match e {
            Event::SelfCheck { results } => Some((*t, *results)),
            _ => None,
        })
        .collect();
    assert_eq!(checks.len(), 1, "one power-up check: {checks:?}");
    let (t, res) = checks[0];
    assert!(t < 30.0, "before light-off: {t} s");
    assert!(matches!(res[0], Check::Delta(d) if d.abs() < 1.0), "{res:?}");
    assert!(matches!(res[1], Check::Delta(d) if d.abs() > 5.0), "{res:?}");
    assert_eq!(res[2], Check::St(ProbeSt::Open));
    assert_eq!(res[3], Check::St(ProbeSt::Short));

    let at = |t: f64| r.samples.iter().find(|s| s.t_s >= t).unwrap().status;
    assert_eq!(
        at(60.0).probe_st,
        [ProbeSt::Ok, ProbeSt::Disagree, ProbeSt::Open, ProbeSt::Short]
    );
    assert!(at(60.0).temps[1].is_some(), "a flagged probe still reads");
    let late = at(600.0);
    assert_eq!(
        late.probe_st[2],
        ProbeSt::Ok,
        "plugged in later, and not second-guessed"
    );
    assert!(late.temps[2].is_some_and(|t| (t - 68.0).abs() < 3.0));
    assert!(late.adc_die_c.is_some());

    let rung: Vec<_> = r
        .events
        .iter()
        .filter(|(_, e)| matches!(e, Event::Alarm { .. }))
        .collect();
    assert!(
        matches!(rung[..], [(t, Event::Alarm { probe: 0, which: Limit::Hi, .. })] if *t < 30.0),
        "{rung:?}"
    );
    assert!(at(60.0).alarm_led && at(60.0).alarms[0] == Some(Limit::Hi));
    assert!(
        !at(200.0).alarm_led && at(200.0).alarms[0] == Some(Limit::Hi),
        "acknowledged, still reported"
    );
}
