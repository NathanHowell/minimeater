//! Fault injection (`.4.16.4`): noisy conversions and EMI spikes on the probe ADC, and an ambient
//! sensor that stops answering, during a hold. None of it may look like a lid, a dying fire or a
//! fault to the controller, and the pit must stay on setpoint.

use minimeat_app::pit::{Event, FaultCode};
use minimeat_sim::hal::Faults;
use minimeat_twin::{Action, Opts, Scenario, run};

#[test]
fn noise_and_spikes_are_filtered_and_a_silent_ambient_sensor_is_a_warning() {
    let h = 3600.0;
    let mut sc = Scenario::kamado_hold(5.0);
    // 2F rms of noise per conversion and one conversion in 20 garbage, for 2 h; the ambient sensor
    // is gone for the second hour of it
    let noisy = Faults {
        adc_noise: 3e-3,
        adc_glitch: 0.05,
        ..Faults::default()
    };
    sc.actions.push((2.0 * h, Action::Faults(noisy)));
    sc.actions.push((
        3.0 * h,
        Action::Faults(Faults {
            ambient_nack: true,
            ..noisy
        }),
    ));
    sc.actions.push((4.0 * h, Action::Faults(Faults::default())));
    let r = run(sc, &Opts { speed: None }, |_, _| {});

    let alarms: Vec<_> = r
        .events
        .iter()
        .filter(|(_, e)| !matches!(e, Event::SelfCheck { .. } | Event::SetpointReached { .. }))
        .collect();
    let amb: Vec<_> = alarms
        .iter()
        .filter(|(_, e)| {
            matches!(
                e,
                Event::Fault(FaultCode::Ambient) | Event::FaultClear(FaultCode::Ambient)
            )
        })
        .map(|(t, _)| t / h)
        .collect();
    assert_eq!(amb.len(), 2, "ambient fault raised and cleared once: {alarms:?}");
    assert!(
        (3.0..3.01).contains(&amb[0]) && (4.0..4.01).contains(&amb[1]),
        "{amb:?}"
    );
    assert_eq!(alarms.len(), 2, "nothing else happened: {alarms:?}");

    let (mut worst, mut worst_read): (f64, f64) = (0.0, 0.0);
    for s in r.samples.iter().filter(|s| s.t_s > 1.5 * h) {
        worst = worst.max((s.pit_f - 250.0).abs());
        if let Some(t) = s.status.temps[0] {
            worst_read = worst_read.max((t - s.pit_f).abs());
        }
        if s.t_s > 2.0 * h && s.t_s < 4.0 * h {
            assert!(s.status.temps[0].is_some(), "the pit reading never dropped out");
        }
    }
    println!("pit within {worst:.1}F of 250; reading within {worst_read:.1}F of truth");
    assert!(worst < 4.0, "pit {worst:.1}F off");
    assert!(
        worst_read < 4.0,
        "filtered reading {worst_read:.1}F off the probe's true value"
    );
}
