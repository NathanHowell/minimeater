//! Fault injection (`.4.16.4`): the configuration flash fails every erase and write for an hour
//! (worn out, or the supply sagging). Saving is best effort: control must not slow or stop, and
//! the cook must not notice.

use minimeater_sim::hal::Faults;
use minimeater_twin::{Action, Opts, Scenario, run};

#[test]
fn a_failing_flash_does_not_touch_the_cook() {
    let h = 3600.0;
    let mut sc = Scenario::kamado_hold(4.0);
    sc.actions.push((
        1.0 * h,
        Action::Faults(Faults {
            flash_fail: true,
            ..Faults::default()
        }),
    ));
    sc.actions.push((2.0 * h, Action::Faults(Faults::default())));
    let r = run(sc, &Opts { speed: None }, |_, _| {});

    // every period ran, on time
    assert!(
        (r.samples.len() as f64 - 4.0 * 360.0).abs() <= 2.0,
        "{} samples",
        r.samples.len()
    );
    for w in r.samples.windows(2) {
        assert!((w[1].t_s - w[0].t_s - 10.0).abs() < 1e-6);
    }
    let worst = r
        .samples
        .iter()
        .filter(|s| s.t_s > 1.5 * h)
        .map(|s| (s.pit_f - 250.0).abs())
        .fold(0.0, f64::max);
    assert!(worst < 3.0, "pit {worst:.1}F off");
    assert!(
        r.events
            .iter()
            .all(|(_, e)| !matches!(e, minimeater_app::pit::Event::Fault(_)))
    );
}
