//! Fire-out / fuel-low detection (`.4.19`) in closed loop on the plant model: a fire that goes out
//! and a fire that runs out of fuel are both reported within 15 min, under either controller and
//! across the egg uncertainty band, and nothing else an egg does (light-off, setpoint moves, weather,
//! cold food, the lid) raises either alarm.

mod common;

use common::{Ev, Lid, Scenario, Trace, run};
use minimeater_core::control::Fire;
use minimeater_core::model::Egg;
use minimeater_sim::{Fuel, uncertainty_band};

const H: f64 = 3600.0;

fn alarmed(f: Fire) -> bool {
    matches!(f, Fire::Low | Fire::Out)
}

/// Minutes after `from` of the first sample matching, if any.
fn first(tr: &Trace, from: f64, f: impl Fn(usize) -> bool) -> Option<f64> {
    (0..tr.t.len())
        .find(|&k| tr.t[k] >= from && f(k))
        .map(|k| (tr.t[k] - from) / 60.0)
}

fn variants() -> impl Iterator<Item = (f64, &'static str, Egg, bool)> {
    [150.0, 225.0, 325.0].into_iter().flat_map(|sp| {
        uncertainty_band(Egg::default())
            .into_iter()
            .flat_map(move |(band, egg)| [true, false].map(move |mpc| (sp, band, egg, mpc)))
    })
}

#[test]
fn a_fire_that_goes_out_is_reported_within_15_min() {
    let mut worst: f64 = 0.0;
    for (sp, band, egg, mpc) in variants() {
        for seed in [1, 2] {
            let tr = run(&Scenario {
                egg,
                mpc,
                sp,
                events: vec![Ev::Extinguish(H)],
                hours: 2.0,
                seed,
                ..Scenario::default()
            });
            let early = first(&tr, 0.0, |k| tr.t[k] < H && alarmed(tr.fire[k]));
            let out = first(&tr, H, |k| tr.fire[k] == Fire::Out).unwrap_or(f64::INFINITY);
            println!("{sp}F {band:9} mpc {mpc:5} seed {seed}: out after {out:4.1} min");
            assert!(early.is_none(), "{sp} {band} mpc {mpc}: alarm before the fire went out");
            assert!(out <= 15.0, "{sp} {band} mpc {mpc}: out after {out:.1} min");
            worst = worst.max(out);
        }
    }
    println!("worst {worst:.1} min");
}

#[test]
fn running_out_of_fuel_is_reported_within_15_min() {
    let mut worst: f64 = 0.0;
    for (sp, band, egg, mpc) in variants() {
        let tr = run(&Scenario {
            egg,
            mpc,
            sp,
            fuel: Fuel {
                kg: 0.5,
                ..Fuel::default()
            },
            hours: 14.0,
            ..Scenario::default()
        });
        // the fire is failing once its heat release halves
        let w0 = tr.fire_w[60];
        let k = (0..tr.t.len())
            .find(|&k| tr.fire_w[k] < 0.5 * w0)
            .expect("fuel ran out");
        let failing = tr.t[k];
        let early = first(&tr, 0.0, |k| tr.t[k] < failing && alarmed(tr.fire[k]));
        let (low, out) = (
            first(&tr, failing, |k| tr.fire[k] == Fire::Low),
            first(&tr, failing, |k| tr.fire[k] == Fire::Out),
        );
        let any = first(&tr, failing, |k| alarmed(tr.fire[k])).unwrap_or(f64::INFINITY);
        println!(
            "{sp}F {band:9} mpc {mpc:5}: failing at {:4.1} h, low {low:?}, out {out:?} min after",
            failing / H
        );
        assert!(early.is_none(), "{sp} {band} mpc {mpc}: alarm while the fire was fine");
        assert!(any <= 15.0, "{sp} {band} mpc {mpc}: reported after {any:.1} min");
        assert!(out.is_some(), "{sp} {band} mpc {mpc}: never reported out");
        worst = worst.max(any);
    }
    println!("worst {worst:.1} min");
}

#[test]
fn nothing_else_raises_a_fire_alarm() {
    let mut scenarios = common::everyday();
    for sp in [150.0, 225.0, 325.0] {
        for dur in [20.0, 300.0] {
            scenarios.push((
                "lid",
                Scenario {
                    sp,
                    events: vec![Ev::LidOpen(H, dur)],
                    hours: 2.5,
                    ..Scenario::default()
                },
            ));
        }
    }
    for (name, sc) in scenarios {
        for (band, egg) in uncertainty_band(Egg::default()) {
            for mpc in [true, false] {
                for (lid, seed) in [(Lid::Detect, 1), (Lid::Detect, 2), (Lid::Oracle, 1)] {
                    let tr = run(&Scenario {
                        egg,
                        mpc,
                        lid,
                        seed,
                        ..sc.clone()
                    });
                    let bad = first(&tr, 0.0, |k| alarmed(tr.fire[k]));
                    let lo = tr.pit.iter().copied().fold(f64::INFINITY, f64::min);
                    println!(
                        "{name:24} {:3}F {band:9} mpc {mpc:5} {lid:?} {seed}: low {lo:5.1}F",
                        sc.sp
                    );
                    assert!(
                        bad.is_none(),
                        "{name} {} {band} mpc {mpc} {lid:?}: false alarm at {bad:?} min",
                        sc.sp
                    );
                }
            }
        }
    }
}
