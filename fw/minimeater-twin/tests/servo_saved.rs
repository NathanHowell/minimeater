//! A servo calibration in flash at power-on (`minimeat-bkz`) is the one the actuators use from the first period, and
//! forgetting it with the console puts the nominal 1 to 2 ms back.

use minimeater_app::config::{Mode, ServoPulses};
use minimeater_app::pit::Command;
use minimeater_sim::hal::ServoMech;
use minimeater_twin::{Action, Opts, Scenario, run};

#[test]
fn the_saved_calibration_is_used_from_boot_and_clearing_it_goes_back() {
    let mut sc = Scenario::kamado_hold(1.0);
    sc.hours = 0.3;
    // this servo is mounted the other way round: closed at 1900 us
    sc.servo = ServoMech {
        closed_us: 1900.0,
        open_us: 1100.0,
        ..ServoMech::default()
    };
    sc.settings.servo = Some(ServoPulses {
        closed_us: 1900,
        open_us: 1100,
    });
    sc.settings.cook.mode = Mode::Off;
    sc.actions.clear();
    sc.actions.extend([
        (1.0, Action::Command(Command::Mode(Mode::Manual))),
        (1.0, Action::Command(Command::ManualFlap(100.0))),
        (300.0, Action::Command(Command::ManualFlap(0.0))),
        (600.0, Action::Command(Command::ManualFlap(100.0))),
        (610.0, Action::Serial("servo clear\r")),
    ]);
    let r = run(sc, &Opts { speed: None }, |_, _| {});
    let flap_at = |t: f64| r.samples.iter().rev().find(|s| s.t_s <= t).unwrap().flap_deg;

    assert!(
        (flap_at(250.0) - 90.0).abs() < 1.0,
        "open on the saved stops: {}",
        flap_at(250.0)
    );
    assert!(
        (flap_at(550.0) - 0.0).abs() < 1.0,
        "closed on the saved stops: {}",
        flap_at(550.0)
    );
    // with the calibration gone the app drives the nominal 1000 us closed, 2000 us open into a servo that is not that:
    // 90 % of 1000..2000 is 1900 us, which this servo takes as closed
    assert!(
        (flap_at(1000.0) - 0.0).abs() < 1.0,
        "nominal pulses on this servo: {}",
        flap_at(1000.0)
    );
    assert_eq!(r.configs.last().unwrap().1.servo, None);
}
