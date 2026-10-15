//! Fault injection: the ambient sensor on the board's underside reads hot (`.4.15`). `board_hot` is raised at 80 C, held
//! through the hysteresis band down to 75 C and cleared below it, and the cook goes on (the fault only warns).

use minimeater_app::pit::{Event, FaultCode, Phase};
use minimeater_sim::hal::Faults;
use minimeater_twin::{Action, Opts, Scenario, run};

#[test]
fn a_hot_ambient_sensor_raises_board_hot_with_hysteresis_and_the_cook_goes_on() {
    let h = 3600.0;
    let (hot, band, cool) = (2.0 * h, 2.0 * h + 600.0, 2.0 * h + 1200.0);
    let offset = |c: f64| {
        Action::Faults(Faults {
            ambient_offset_c: c,
            ..Faults::default()
        })
    };
    let mut sc = Scenario::kamado_hold(3.0);
    // the sensor reads 21.8 C here: +60 is 82, +55 is 77 (between 75 and 80), back to 21.8
    sc.actions
        .extend([(hot, offset(60.0)), (band, offset(55.0)), (cool, offset(0.0))]);
    let r = run(sc, &Opts { speed: None }, |_, _| {});

    let when = |f: fn(&Event) -> bool| r.events.iter().find(|(_, e)| f(e)).map(|(t, _)| *t);
    let raised = when(|e| matches!(e, Event::Fault(FaultCode::BoardHot))).expect("board_hot raised");
    assert!((hot..hot + 20.0).contains(&raised), "raised at +{:.0} s", raised - hot);
    let cleared = when(|e| matches!(e, Event::FaultClear(FaultCode::BoardHot))).expect("board_hot cleared");
    assert!(
        (cool..cool + 20.0).contains(&cleared),
        "cleared at +{:.0} s, not inside the band",
        cleared - cool
    );
    // one raise and one clear: the band did not make it flap
    let n = r
        .events
        .iter()
        .filter(|(_, e)| matches!(e, Event::Fault(FaultCode::BoardHot)))
        .count();
    assert_eq!(n, 1);

    // the cook is not touched: still holding, and the implausible reading never reached the airflow plan
    let window = || r.samples.iter().filter(|s| s.t_s > hot && s.t_s < cool + 600.0);
    assert!(window().all(|s| s.status.out.phase == Phase::Hold));
    for s in window() {
        let pit = s.status.temps[0].expect("pit reading");
        assert!((pit - s.status.cook.sp_f).abs() < 1.5, "t {} pit {pit}", s.t_s - hot);
        assert!(
            s.status.ambient_c.is_none_or(|c| c < 30.0),
            "t {} amb {:?}",
            s.t_s - hot,
            s.status.ambient_c
        );
    }
}
