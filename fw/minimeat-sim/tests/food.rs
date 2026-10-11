//! Food estimator (`.4.22`) on the two-node evaporative meat model: from a noisy food probe and pit
//! probe alone, a brisket's or shoulder's stall is reported within 30 min of its onset and end, the
//! ETA at the end of the stall is within 1 h of the real finish (and stays there), and ribs, which
//! slow down but do not plateau, show no stall. Onset and end are where the true core rate crosses
//! 5F/h.

mod common;

use common::{Ev, Scenario, run};
use minimeat_core::food::{Cut, Food, FoodEstimator, Stall};
use minimeat_core::model::Ambient;
use minimeat_core::units::f_to_k;
use minimeat_sim::{Meat, Rng};

const H: f64 = 3600.0;

/// A cook, sampled every 10 s: time, pit probe reading, core (truth), lid handling active.
struct Cook {
    t: Vec<f64>,
    pit: Vec<f64>,
    core: Vec<f64>,
    lid: Vec<bool>,
}

/// The meat alone in a steady pit (pit probe noise 0.3F).
fn steady(meat: Meat, pit_f: f64, hours: f64, seed: u64) -> Cook {
    let (mut m, amb, mut rng) = (meat, Ambient::default(), Rng::new(seed));
    let mut ck = Cook {
        t: vec![],
        pit: vec![],
        core: vec![],
        lid: vec![],
    };
    for k in 0..(hours * H / 10.0) as usize {
        ck.t.push(k as f64 * 10.0);
        ck.pit.push(pit_f + 0.3 * rng.normal());
        ck.core.push(m.core_f());
        ck.lid.push(false);
        for _ in 0..5 {
            m.step(2.0, f_to_k(pit_f), &amb);
        }
    }
    ck
}

struct Truth {
    onset: Option<f64>,
    end: Option<f64>,
    finish: f64,
}

/// Onset/end where the 10 min core rate crosses 5F/h (above 120F); finish at `target`.
fn truth(ck: &Cook, target: f64) -> Truth {
    let rate = |k: usize| (ck.core[k + 30] - ck.core[k - 30]) * 6.0;
    let mut tr = Truth {
        onset: None,
        end: None,
        finish: f64::INFINITY,
    };
    for k in 30..ck.t.len() - 30 {
        if ck.core[k] >= target {
            break;
        }
        match (tr.onset, tr.end) {
            (None, _) if ck.core[k] > 120.0 && rate(k) < 5.0 => tr.onset = Some(ck.t[k]),
            (Some(_), None) if rate(k) > 5.0 => tr.end = Some(ck.t[k]),
            _ => {}
        }
    }
    if let Some(k) = ck.core.iter().position(|&c| c >= target) {
        tr.finish = ck.t[k];
    }
    tr
}

/// Runs the estimator over the cook with food-probe noise; its output every 10 s.
fn estimate(ck: &Cook, cut: Cut, target: f64, seed: u64) -> Vec<Food> {
    let mut e = FoodEstimator::new(cut);
    let mut rng = Rng::new(seed + 100);
    (0..ck.t.len())
        .map(|k| {
            let core = Some(ck.core[k] + 0.1 * rng.normal());
            e.update(core, Some(ck.pit[k]), Some(target), ck.lid[k], 10.0)
        })
        .collect()
}

struct Score {
    onset_late_min: f64,
    end_late_min: f64,
    /// Worst |ETA error| from the detected end of the stall to 30 min before the finish, h.
    eta_err_h: f64,
    /// ETA error at the detected end of the stall, h.
    eta_at_end_h: f64,
}

fn score(ck: &Cook, est: &[Food], tr: &Truth) -> Score {
    let detected = |want: fn(Stall) -> bool| {
        (0..est.len())
            .find(|&k| want(est[k].stall))
            .map_or(f64::INFINITY, |k| ck.t[k])
    };
    let on = detected(|s| matches!(s, Stall::In { .. }));
    let off = detected(|s| s == Stall::Done);
    let err = |k: usize| est[k].eta_s.map_or(f64::INFINITY, |e| (ck.t[k] + e - tr.finish) / H);
    let k_off = (0..est.len()).find(|&k| ck.t[k] >= off);
    let eta_err_h = (0..est.len())
        .filter(|&k| ck.t[k] >= off && ck.t[k] <= tr.finish - 0.5 * H)
        .map(err)
        .fold(0.0f64, |w, e| if e.abs() > w.abs() { e } else { w });
    Score {
        onset_late_min: (on - tr.onset.unwrap_or(f64::NAN)) / 60.0,
        end_late_min: (off - tr.end.unwrap_or(f64::NAN)) / 60.0,
        eta_err_h,
        eta_at_end_h: k_off.map_or(f64::INFINITY, err),
    }
}

fn check_stalling(name: &str, ck: &Cook, cut: Cut) {
    for seed in [1, 2, 3] {
        let est = estimate(ck, cut, 203.0, seed);
        let tr = truth(ck, 203.0);
        let s = score(ck, &est, &tr);
        println!(
            "{name} seed {seed}: stall {:.1}-{:.1} h, finish {:.1} h | detected {:+.0} / {:+.0} min, ETA at the \
             end {:+.2} h, worst after {:+.2} h",
            tr.onset.unwrap() / H,
            tr.end.unwrap() / H,
            tr.finish / H,
            s.onset_late_min,
            s.end_late_min,
            s.eta_at_end_h,
            s.eta_err_h
        );
        assert!(
            (0.0..=30.0).contains(&s.onset_late_min),
            "{name}: onset {:.0} min",
            s.onset_late_min
        );
        assert!(
            (0.0..=30.0).contains(&s.end_late_min),
            "{name}: end {:.0} min",
            s.end_late_min
        );
        assert!(s.eta_at_end_h.abs() <= 1.0, "{name}: ETA {:.2} h off", s.eta_at_end_h);
        assert!(s.eta_err_h.abs() <= 1.0, "{name}: ETA {:.2} h off later", s.eta_err_h);
    }
}

#[test]
fn brisket_and_shoulder_stalls_and_eta_in_a_steady_pit() {
    for pit in [225.0, 250.0, 275.0] {
        check_stalling(
            &format!("brisket {pit}F"),
            &steady(Meat::brisket(), pit, 22.0, 1),
            Cut::Brisket,
        );
    }
    check_stalling(
        "shoulder 250F",
        &steady(Meat::pork_shoulder(), 250.0, 20.0, 1),
        Cut::PorkShoulder,
    );
    // without the cut's priors
    check_stalling(
        "brisket 250F, cut unknown",
        &steady(Meat::brisket(), 250.0, 20.0, 1),
        Cut::Other,
    );
}

#[test]
fn brisket_in_the_egg_with_the_lid_opened_every_two_hours() {
    let events = (1..8).map(|i| Ev::LidOpen(i as f64 * 2.0 * H, 60.0)).collect();
    let tr = run(&Scenario {
        sp: 250.0,
        hours: 18.0,
        meat: Some(Meat::brisket()),
        events,
        ..Scenario::default()
    });
    let ck = Cook {
        t: tr.t,
        pit: tr.pit,
        core: tr.core,
        lid: tr.lid_active,
    };
    check_stalling("brisket in the egg, lid every 2 h", &ck, Cut::Brisket);
}

#[test]
fn ribs_slow_down_but_do_not_stall() {
    for pit in [225.0, 250.0, 275.0] {
        let ck = steady(Meat::baby_back_ribs(), pit, 9.0, 1);
        let tr = truth(&ck, 195.0);
        for cut in [Cut::Ribs, Cut::Other] {
            for seed in [1, 2, 3] {
                let est = estimate(&ck, cut, 195.0, seed);
                let stalled = est.iter().any(|f| f.stall != Stall::None);
                // ETA from 1.5 h in, until 30 min before the finish
                let worst = (0..est.len())
                    .filter(|&k| ck.t[k] >= 1.5 * H && ck.t[k] <= tr.finish - 0.5 * H)
                    .map(|k| est[k].eta_s.map_or(f64::INFINITY, |e| (ck.t[k] + e - tr.finish) / H))
                    .fold(0.0f64, |w, e| if e.abs() > w.abs() { e } else { w });
                println!(
                    "ribs {pit}F {cut:?} seed {seed}: finish {:.1} h, stalled {stalled}, worst ETA {worst:+.2} h",
                    tr.finish / H
                );
                assert!(tr.onset.is_none(), "the model's ribs plateau");
                assert!(!stalled, "ribs {pit} {cut:?}: false stall");
                assert!(worst.abs() <= 1.0, "ribs {pit} {cut:?}: ETA {worst:.2} h off");
            }
        }
    }
}

#[test]
fn the_app_raises_one_stall_start_and_end_per_food_probe() {
    use minimeat_app::config::Settings;
    use minimeat_app::food::Foods;
    use minimeat_app::pit::{Event, Phase};

    let ck = steady(Meat::brisket(), 250.0, 15.0, 1);
    let mut probes = Settings::default().probes;
    probes[2].cut = Cut::Brisket;
    let mut foods = Foods::new(&probes);
    let mut events = vec![];
    for k in 0..ck.t.len() {
        // jack 0 is the pit, 2 the brisket, 1 and 3 empty
        let temps = [Some(ck.pit[k]), None, Some(ck.core[k]), None];
        for e in foods.step(&temps, 0, Some(ck.pit[k]), Phase::Hold, &probes, 10.0) {
            events.push(*e);
        }
        if k == ck.t.len() - 1 {
            assert!(foods.food(0, &temps, 0).is_none() && foods.food(1, &temps, 0).is_none());
            assert_eq!(
                foods.food(2, &temps, 0).unwrap().eta_s,
                Some(0.0),
                "the brisket is done"
            );
        }
    }
    let kinds: Vec<_> = events
        .iter()
        .map(|e| match e {
            Event::StallStart { probe, .. } => ("start", *probe),
            Event::StallEnd { probe, .. } => ("end", *probe),
            _ => ("other", 99),
        })
        .collect();
    assert_eq!(kinds, [("start", 2), ("end", 2)]);
}
