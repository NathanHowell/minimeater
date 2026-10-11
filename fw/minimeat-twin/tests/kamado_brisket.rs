//! The whole application on the twin (`.4.16.2`), flat out: a brisket cook from a cold egg. The
//! tasks, the bus, the settings in flash and the simulated board are the ones the firmware uses.

use minimeat_app::alarms::Limit;
use minimeat_app::pit::{Event, Phase};
use minimeat_twin::{Opts, Scenario, run};

#[test]
fn a_brisket_cook_runs_end_to_end() {
    let sc = Scenario::kamado_brisket();
    let t0 = std::time::Instant::now();
    let r = run(sc, &Opts { speed: None }, |_, _| {});
    let wall = t0.elapsed();
    let h = 3600.0;

    // the control period is 10 s of virtual time
    let n = r.samples.len();
    assert!((n as f64 - 18.0 * 360.0).abs() <= 2.0, "{n} samples");
    let dt = r.samples[1].t_s - r.samples[0].t_s;
    assert!((dt - 10.0).abs() < 1e-6, "period {dt}");

    // the pit holds 250F outside light-off and the lid (the first lid also puts a cold 6 kg brisket
    // on: give it half an hour)
    let mut worst: f64 = 0.0;
    let lid_until = |t: f64| {
        r.events
            .iter()
            .any(|(at, e)| matches!(e, Event::LidOpen { .. }) && t >= at - 60.0 && t < at + 1200.0)
    };
    for s in &r.samples {
        if s.t_s > 1.5 * h && s.status.out.phase == Phase::Hold && !lid_until(s.t_s) {
            worst = worst.max((s.pit_f - 250.0).abs());
        }
    }

    let count = |f: fn(&Event) -> bool| r.events.iter().filter(|(_, e)| f(e)).count();
    let lids = count(|e| matches!(e, Event::LidOpen { .. }));
    let stall: Vec<f64> = r
        .events
        .iter()
        .filter(|(_, e)| matches!(e, Event::StallStart { .. } | Event::StallEnd { .. }))
        .map(|(t, _)| t / h)
        .collect();
    let done = r
        .samples
        .iter()
        .find(|s| s.cores_f[0].is_some_and(|c| c >= 203.0))
        .map(|s| s.t_s / h);
    let eta_at_end = r
        .samples
        .iter()
        .find(|s| stall.len() == 2 && s.t_s / h >= stall[1])
        .and_then(|s| s.status.foods[1].and_then(|f| f.eta_s).map(|e| (s.t_s + e) / h));
    println!(
        "{n} samples in {wall:?}; pit within {worst:.1}F at hold; {lids} lids; stall {stall:.2?} h; \
         done at {done:.2?} h, predicted at the stall's end {eta_at_end:.2?} h"
    );
    assert!(worst <= 3.0, "pit {worst:.1}F off at hold");
    assert_eq!(lids, 5, "every lid opening seen once");
    assert_eq!(stall.len(), 2, "one stall start and end: {stall:?}");
    let done = done.expect("the brisket finished");
    assert!((eta_at_end.unwrap() - done).abs() <= 1.0);
    // the brisket's `hi` limit is its 203F target: one alarm, as the probe gets there
    let alarms: Vec<_> = r
        .events
        .iter()
        .filter_map(|(t, e)| match e {
            Event::Alarm { probe, which, .. } => Some((t / h, *probe, *which)),
            Event::AlarmClear { .. } => panic!("an alarm cleared: {e:?}"),
            _ => None,
        })
        .collect();
    assert!(
        matches!(alarms[..], [(t, 1, Limit::Hi)] if (t - done).abs() < 0.25),
        "{alarms:?} for a finish at {done:.2} h"
    );
    assert_eq!(r.samples.last().unwrap().status.alarms[1], Some(Limit::Hi));
    assert!(r.samples.last().unwrap().status.alarm_led);
    assert_eq!(
        count(|e| matches!(e, Event::Fault(_) | Event::FireLow { .. } | Event::FireOut { .. })),
        0
    );
    assert!(r.samples.last().unwrap().status.out.phase == Phase::Hold);
}
