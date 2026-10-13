//! The stall bump's bookkeeping (`.4.23`): what `Pit::bump` and the responder do to the setpoint in
//! the cases the scenario suite does not reach. The whole policy on a cook is in the twin
//! (`stall_policy.rs` there).

use minimeater_app::config::{Mode, Probe, Settings};
use minimeater_app::food::{Foods, StallResponder};
use minimeater_app::hal::PROBE_JACKS;
use minimeater_app::pit::{Command, Event, PERIOD_S, Pit};
use minimeater_core::model::Ambient;
use minimeater_core::model::airflow::AirPath;

fn pit(sp: f64, mode: Mode) -> Pit {
    let mut s = Settings::default();
    s.cook.mode = mode;
    s.cook.sp_f = sp;
    Pit::new(&s, AirPath::default(), Ambient::default())
}

#[test]
fn a_bump_is_capped_remembered_and_undone() {
    let mut p = pit(250.0, Mode::Run);
    assert_eq!(p.bump(25.0), Some(275.0));
    assert_eq!(p.cook().base_sp, Some(250.0));
    assert_eq!(p.bump(25.0), None, "once");
    assert_eq!(p.unbump(), Some(250.0));
    assert_eq!((p.cook().sp_f, p.cook().base_sp), (250.0, None));
    assert_eq!(p.unbump(), None);

    let mut p = pit(310.0, Mode::Run);
    assert_eq!(p.bump(50.0), Some(325.0), "capped at the setpoint limit");
    assert_eq!(p.unbump(), Some(310.0));
    let mut p = pit(325.0, Mode::Run);
    assert_eq!(p.bump(25.0), None, "nowhere to go");
    assert_eq!(p.cook().base_sp, None);

    assert_eq!(pit(250.0, Mode::Off).bump(25.0), None, "only a cook in run mode");
    assert_eq!(pit(250.0, Mode::Manual).bump(25.0), None);
}

#[test]
fn a_setpoint_set_while_bumped_stands() {
    let mut p = pit(250.0, Mode::Run);
    p.bump(25.0).unwrap();
    p.command(Command::Sp(300.0)).unwrap();
    assert_eq!((p.cook().sp_f, p.cook().base_sp), (300.0, None));
    assert_eq!(p.unbump(), None, "nothing to put back");
}

#[test]
fn a_bump_with_no_stall_is_put_back_after_a_quarter_hour() {
    // what a power cut mid-stall leaves: a bumped cook, and estimators that start over
    let probes: [Probe; PROBE_JACKS] = Settings::default().probes;
    let foods = Foods::new(&probes);
    let temps = [None; PROBE_JACKS];
    let mut p = pit(250.0, Mode::Run);
    p.bump(25.0).unwrap();
    let mut r = StallResponder::default();
    let mut t = 0.0;
    let back = loop {
        t += PERIOD_S;
        if let Some(e) = r.step(&foods, &temps, 0, &probes, &mut p, PERIOD_S) {
            break (t, e);
        }
        assert!(t < 2000.0, "never put back");
    };
    assert!((900.0..=910.0).contains(&back.0), "{} s", back.0);
    assert_eq!(back.1, Event::StallRestore { sp_f: 250.0 });
    assert_eq!(p.cook().sp_f, 250.0);
}

#[test]
fn leaving_run_mode_puts_the_setpoint_back_at_once() {
    let probes: [Probe; PROBE_JACKS] = Settings::default().probes;
    let foods = Foods::new(&probes);
    let mut p = pit(250.0, Mode::Run);
    p.bump(25.0).unwrap();
    p.command(Command::Mode(Mode::Off)).unwrap();
    let mut r = StallResponder::default();
    let e = r.step(&foods, &[None; PROBE_JACKS], 0, &probes, &mut p, PERIOD_S);
    assert_eq!(e, Some(Event::StallRestore { sp_f: 250.0 }));
}
