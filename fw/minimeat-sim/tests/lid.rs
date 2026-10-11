//! Lid-open detection and recovery (`.4.6`) in closed loop on the plant model: the detector, seeing
//! only the noisy pit probe, catches every lid opening of 20 s or more within 30 s, lets go once the
//! lid is shut, and recovers about as well as if the controller had been told about the lid. Nothing
//! else an egg does (light-off, setpoint moves, weather, food, a dying fire) opens it.

mod common;

use common::{Ev, Lid, Scenario, Trace, run};
use minimeat_core::model::Egg;
use minimeat_sim::{Fuel, uncertainty_band};

struct Recovery {
    /// Lid up to detected, and lid shut (or detected, if later) to released, s.
    detect_s: f64,
    release_s: f64,
    /// Peak above setpoint in the hour after the lid shuts, F.
    overshoot_f: f64,
    /// Lid shut to back within 5F, min.
    recover_min: f64,
}

fn recovery(tr: &Trace, at: f64, dur: f64) -> Recovery {
    let shut = at + dur;
    let first = |from: f64, want: bool| {
        tr.t.iter()
            .zip(&tr.open)
            .find(|&(&t, &o)| t >= from && o == want)
            .map_or(f64::INFINITY, |(&t, _)| t - from)
    };
    let after: Vec<(f64, f64, f64)> = (0..tr.t.len())
        .filter(|&k| tr.t[k] >= shut && tr.t[k] < shut + 3600.0)
        .map(|k| (tr.t[k], tr.pit[k], tr.sp[k]))
        .collect();
    let overshoot_f = after.iter().map(|&(_, y, sp)| y - sp).fold(f64::NEG_INFINITY, f64::max);
    let recover_min = after
        .iter()
        .find(|&&(_, y, sp)| (y - sp).abs() < 5.0)
        .map_or(f64::INFINITY, |&(t, _, _)| (t - shut) / 60.0);
    let detect_s = first(at, true);
    Recovery {
        detect_s,
        release_s: first(shut.max(at + detect_s), false),
        overshoot_f,
        recover_min,
    }
}

const LID_AT: f64 = 3600.0;

#[test]
fn detects_every_lid_and_recovers_like_the_oracle() {
    let mut worst = (0.0f64, 0.0f64, 0.0f64);
    for sp in [150.0, 225.0, 325.0] {
        for (name, egg) in uncertainty_band(Egg::default()) {
            for dur in [20.0, 60.0, 120.0, 300.0] {
                for mpc in [true, false] {
                    let sc = Scenario {
                        egg,
                        mpc,
                        sp,
                        events: vec![Ev::LidOpen(LID_AT, dur)],
                        hours: 2.5,
                        seed: 3,
                        ..Scenario::default()
                    };
                    let go = |lid| recovery(&run(&Scenario { lid, ..sc.clone() }), LID_AT, dur);
                    let (d, o, x) = (go(Lid::Detect), go(Lid::Oracle), go(Lid::Ignore));
                    let kind = if mpc { "mpc " } else { "ffpi" };
                    println!(
                        "{sp}F {name:9} {kind} lid {dur:3}s: detect {:3.0}s release {:4.0}s | overshoot \
                         {:5.1}F (oracle {:5.1}, none {:5.1}) | back in {:5.1} min (oracle {:5.1}, none {:5.1})",
                        d.detect_s,
                        d.release_s,
                        d.overshoot_f,
                        o.overshoot_f,
                        x.overshoot_f,
                        d.recover_min,
                        o.recover_min,
                        x.recover_min
                    );
                    let early = early_release(&run(&sc));
                    assert_eq!(early, 0, "{sp} {name} {dur}: released with the lid up");
                    assert!(d.detect_s <= 30.0, "{sp} {name} {dur}: detected after {}s", d.detect_s);
                    assert!(
                        d.release_s <= 120.0,
                        "{sp} {name} {dur}: released after {}s",
                        d.release_s
                    );
                    // the detector is a sample or two late either way, but never worse than knowing
                    assert!(d.overshoot_f <= o.overshoot_f + 1.0, "{sp} {name} {dur}: overshoot");
                    assert!(d.recover_min <= o.recover_min + 3.0, "{sp} {name} {dur}: recovery");
                    worst.0 = worst.0.max(d.overshoot_f - o.overshoot_f);
                    worst.1 = worst.1.max(d.recover_min - o.recover_min);
                    worst.2 = worst.2.max(d.overshoot_f);
                }
            }
        }
    }
    println!(
        "worst vs oracle: overshoot +{:.1}F, recovery +{:.1} min; worst overshoot {:.1}F",
        worst.0, worst.1, worst.2
    );
}

/// Samples where the detector let go while the lid was still up.
fn early_release(tr: &Trace) -> usize {
    (1..tr.t.len())
        .filter(|&k| tr.up[k] && tr.open[k - 1] && !tr.open[k])
        .count()
}

/// Everything else an egg does, with the detector on and the lid shut throughout.
#[test]
fn nothing_but_the_lid_opens_it() {
    let mut scenarios = common::everyday();
    scenarios.push((
        "fuel runs out (fire dies)",
        Scenario {
            sp: 250.0,
            fuel: Fuel {
                kg: 0.5,
                ..Fuel::default()
            },
            hours: 8.0,
            ..Scenario::default()
        },
    ));
    for (name, sc) in scenarios {
        for (band, egg) in uncertainty_band(Egg::default()) {
            for mpc in [true, false] {
                for seed in [1, 2] {
                    let tr = run(&Scenario {
                        egg,
                        mpc,
                        seed,
                        ..sc.clone()
                    });
                    let opened = tr.open.iter().position(|&o| o);
                    let lo = tr.pit.iter().copied().fold(f64::INFINITY, f64::min);
                    println!("{name:26} {band:9} mpc {mpc:5} seed {seed}: low {lo:5.1}F, opened {opened:?}");
                    assert_eq!(
                        opened,
                        None,
                        "{name} {band}: false lid at t={:?}",
                        opened.map(|k| tr.t[k])
                    );
                }
            }
        }
    }
}
