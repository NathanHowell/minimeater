//! Closed-loop autotune on the plant (`.4.4`): with the MPC in charge throughout and only a +/-3F
//! setpoint dither for excitation, the learner identifies every egg in the uncertainty band within
//! 15% of its small-signal model (dead time: 15% or 20 s, two control periods, whichever is larger;
//! the probe and fire lags smear its onset), the pit never strays, and disturbances (lid, fire dying) never
//! push the schedule anywhere implausible.
//!
//! "Truth" is the small-signal FOPDT: a noise-free +5% open-loop airflow step from exact steady
//! state, fitted by the same identifier. The Python report's models (`docs/modeling/control.md`) come
//! from a +30% step, which on a lean, lossy egg drives the fire into a regime with much lower
//! average gain (high-loss 325F: 48 vs 72 F/CFM small-signal); they are printed for comparison
//! only. The MPC regulates around the setpoint, so the small-signal model is the one it needs.

use minimeat_core::control::Fopdt;
use minimeat_core::control::autotune::{Autotune, Identifier, MAX_DELAY, WINDOW};
use minimeat_core::control::{Mpc, Obs, PitController, Schedule};
use minimeat_core::model::airflow::{AirPath, AirflowMap, ModelFeedforward};
use minimeat_core::model::thermal::steady_state;
use minimeat_core::model::{Ambient, Egg, top_vent};
use minimeat_core::units::{CFM, air_density, f_to_k};
use minimeat_sim::{Actuators, Dynamics, Environment, Fuel, Plant, Rng, uncertainty_band};

/// Python reference (`uv run minimeat-control`): (plant, sp F, K F/CFM, tau min, theta s).
const REFERENCE: [(&str, f64, f64, f64, f64); 12] = [
    ("low loss", 150.0, 157.0, 9.1, 96.0),
    ("low loss", 225.0, 146.0, 8.8, 103.0),
    ("low loss", 275.0, 138.0, 8.5, 108.0),
    ("low loss", 325.0, 132.0, 8.4, 111.0),
    ("nominal", 150.0, 128.0, 8.9, 97.0),
    ("nominal", 225.0, 116.0, 8.4, 107.0),
    ("nominal", 275.0, 110.0, 8.2, 111.0),
    ("nominal", 325.0, 104.0, 8.0, 115.0),
    ("high loss", 150.0, 68.0, 8.5, 103.0),
    ("high loss", 225.0, 58.0, 8.0, 115.0),
    ("high loss", 275.0, 53.0, 7.7, 123.0),
    ("high loss", 325.0, 48.0, 7.4, 132.0),
];

/// What happens to the egg during a run.
#[derive(Clone, Copy, Default)]
struct Events {
    /// Lid open for 2 min at this hour.
    lid_at_h: Option<f64>,
    /// Fuel load (kg) and die-back threshold: the fire runs out mid-run.
    fuel: Option<(f64, f64)>,
}

struct Run {
    schedule: Schedule,
    /// Largest |pit - user setpoint| after the first 45 min, outside lid events, F.
    max_err_f: f64,
    accepted: u32,
}

/// `hours` at `sp` from a pit 20F low, MPC with the nominal schedule, autotune button pressed at
/// the start.
fn run(egg: Egg, sp: f64, hours: f64, seed: u64, ev: Events) -> Run {
    let env0 = Environment {
        ambient: Ambient::default(),
        top_area_m2: top_vent::SLIDER_QUARTER,
        lid_open: false,
        food_load_w: 0.0,
    };
    let model = AirPath::default();
    let map = AirflowMap::build(&model, env0.ambient);
    let ff = ModelFeedforward {
        path: &model,
        ambient: env0.ambient,
    };
    let path = AirPath { egg, ..model };
    let mut plant = Plant::steady(path, Dynamics::default(), &env0.ambient, sp - 20.0, 0.0).unwrap();
    if let Some((kg, sustain)) = ev.fuel {
        plant.fuel = Fuel {
            kg,
            sustain_w: sustain,
            ..Fuel::default()
        };
    }
    let mut rng = Rng::new(seed);
    let mut ctrl = PitController::Mpc(Mpc::new(Schedule::default()));
    let mut learn = Autotune::default();
    learn.start();
    let mut max_err: f64 = 0.0;
    let mut lid_until = f64::NEG_INFINITY;
    for k in 0..(hours * 360.0) as usize {
        let t_h = k as f64 / 360.0;
        let lid = ev.lid_at_h.is_some_and(|h| (h..h + 2.0 / 60.0).contains(&t_h));
        if lid {
            lid_until = t_h + 0.75;
        }
        let env = Environment { lid_open: lid, ..env0 };
        let pit = plant.pit_probe_f() + 0.3 * rng.normal();
        let (q_min, q_max) = map.limits(pit);
        let reference = learn.reference(sp, 10.0);
        let obs = Obs {
            pit_f: pit,
            sp_f: reference,
            lid,
            q_min,
            q_max,
        };
        let q = ctrl.step(&obs, &ff, 10.0);
        let recovering = match &ctrl {
            PitController::Mpc(m) => m.lid.active(),
            PitController::FfPi(f) => f.lid.active(),
        };
        let mut sched = ctrl.schedule_mut().clone();
        if learn.observe(&obs, sp, q, !recovering, 10.0, &mut sched).is_some() {
            *ctrl.schedule_mut() = sched;
        }
        if t_h > 0.75 && t_h > lid_until {
            max_err = max_err.max((plant.pit_probe_f() - sp).abs());
        }
        let a = map.actuate(pit, q);
        let act = Actuators {
            damper: a.damper,
            fan: a.fan,
        };
        for _ in 0..5 {
            plant.step(2.0, act, &env);
        }
    }
    Run {
        schedule: ctrl.schedule_mut().clone(),
        max_err_f: max_err,
        accepted: learn.status().accepted,
    }
}

/// Small-signal FOPDT of `egg` at `sp` in commanded CFM (through the firmware's nominal map).
fn truth(egg: Egg, sp: f64) -> Fopdt {
    let amb = Ambient::default();
    let model = AirPath::default();
    let map = AirflowMap::build(&model, amb);
    let path = AirPath { egg, ..model };
    let ss = steady_state(&egg, &amb, f_to_k(sp), 0.0).unwrap();
    let q_ss = ss.m_air_kg_s / air_density(amb.t_k) / CFM;
    let actual = |q: f64| {
        let a = map.actuate(sp, q);
        path.air_flow(&amb, a.fan, a.damper, f_to_k(sp)) / CFM
    };
    // commanded airflow that delivers the steady airflow
    let (mut lo, mut hi) = map.limits(sp);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if actual(mid) < q_ss { lo = mid } else { hi = mid }
    }
    let q0 = 0.5 * (lo + hi);
    let mut plant = Plant::steady(path, Dynamics::default(), &amb, sp, 0.0).unwrap();
    let env = Environment {
        ambient: amb,
        top_area_m2: top_vent::SLIDER_QUARTER,
        lid_open: false,
        food_load_w: 0.0,
    };
    let mut id = Identifier::default();
    for k in 0..(WINDOW + MAX_DELAY) {
        let q = if k < 120 { q0 } else { q0 * 1.05 };
        let pit = plant.pit_probe_f();
        id.push(pit, q, sp, 10.0, usize::MAX);
        let a = map.actuate(pit, q);
        for _ in 0..5 {
            plant.step(
                2.0,
                Actuators {
                    damper: a.damper,
                    fan: a.fan,
                },
                &env,
            );
        }
    }
    let f = id.fit().unwrap();
    Fopdt {
        sp_f: sp,
        k: f.k,
        tau_s: f.tau_s,
        theta_s: f.theta_s,
    }
}

fn egg(name: &str) -> Egg {
    uncertainty_band(Egg::default())
        .into_iter()
        .find(|b| b.0 == name)
        .unwrap()
        .1
}

#[test]
fn identifies_band_within_15pct_of_small_signal_model() {
    let mut worst: f64 = 0.0;
    for seed in [0u64, 100] {
        for (i, (name, sp, k_py, ..)) in REFERENCE.into_iter().enumerate() {
            let t = truth(egg(name), sp);
            let r = run(egg(name), sp, 5.0, seed + i as u64, Events::default());
            let m = r.schedule.at(sp);
            let errs = [m.k / t.k - 1.0, m.tau_s / t.tau_s - 1.0, m.theta_s / t.theta_s - 1.0];
            let theta_ok = (m.theta_s - t.theta_s).abs() <= 20.0_f64.max(0.15 * t.theta_s);
            println!(
                "seed {seed:3} {name:9} {sp}F: truth K {:5.1} tau {:4.1} min theta {:5.1} s (python K {k_py:5.1}); learned K {:6.1} ({:+5.1}%) tau {:4.1} min ({:+5.1}%) theta {:5.1} s ({:+5.1}%), {} fits, pit within {:.1}F",
                t.k,
                t.tau_s / 60.0,
                t.theta_s,
                m.k,
                100.0 * errs[0],
                m.tau_s / 60.0,
                100.0 * errs[1],
                m.theta_s,
                100.0 * errs[2],
                r.accepted,
                r.max_err_f
            );
            worst = errs[..2].iter().fold(worst, |w, e| w.max(e.abs()));
            assert!(
                theta_ok,
                "{name} {sp}F: dead time {:.0} s vs {:.0} s",
                m.theta_s, t.theta_s
            );
            assert!(r.max_err_f < 8.0, "{name} {sp}F: pit strayed {:.1}F", r.max_err_f);
        }
    }
    assert!(worst < 0.15, "worst parameter error {:.1}%", 100.0 * worst);
}

/// The light-off case `.4.17` left at 11.7F with the nominal schedule: the high-loss egg at 325F,
/// now with the schedule learned at each scheduling point.
#[test]
fn learned_schedule_tightens_light_off() {
    let egg = egg("high loss");
    let mut sched = Schedule::default();
    for (i, sp) in [150.0, 225.0, 275.0, 325.0].into_iter().enumerate() {
        sched.insert(run(egg, sp, 5.0, 40 + i as u64, Events::default()).schedule.at(sp));
    }
    let amb = Ambient::default();
    let model = AirPath::default();
    let map = AirflowMap::build(&model, amb);
    let ff = ModelFeedforward {
        path: &model,
        ambient: amb,
    };
    let env = Environment {
        ambient: amb,
        top_area_m2: top_vent::SLIDER_QUARTER,
        lid_open: false,
        food_load_w: 0.0,
    };
    let mut plant = Plant::cold(AirPath { egg, ..model }, Dynamics::default(), &amb);
    let mut ctrl = Mpc::new(sched);
    let mut rng = Rng::new(7);
    let mut peak = f64::NEG_INFINITY;
    for _ in 0..(3 * 360) {
        let pit = plant.pit_probe_f() + 0.3 * rng.normal();
        let (q_min, q_max) = map.limits(pit);
        let obs = Obs {
            pit_f: pit,
            sp_f: 325.0,
            lid: false,
            q_min,
            q_max,
        };
        let a = map.actuate(pit, ctrl.step(&obs, &ff, 10.0));
        for _ in 0..5 {
            plant.step(
                2.0,
                Actuators {
                    damper: a.damper,
                    fan: a.fan,
                },
                &env,
            );
        }
        peak = peak.max(plant.pit_probe_f());
    }
    println!(
        "high-loss 325F light-off, learned schedule: overshoot {:.1}F",
        peak - 325.0
    );
    assert!(peak - 325.0 < 10.0, "overshoot {:.1}F", peak - 325.0);
}

/// Lid events and a dying fire are not the plant's dynamics: whatever the learner makes of them,
/// the schedule stays plausible and control carries on.
#[test]
fn disturbances_never_corrupt_the_schedule() {
    let nominal = Schedule::default();
    for (what, ev) in [
        (
            "lid",
            Events {
                lid_at_h: Some(1.5),
                ..Events::default()
            },
        ),
        (
            "fuel out",
            Events {
                fuel: Some((0.6, 300.0)),
                ..Events::default()
            },
        ),
    ] {
        let r = run(egg("nominal"), 225.0, 6.0, 3, ev);
        for m in r.schedule.models() {
            let n = nominal.at(m.sp_f);
            for (got, base) in [(m.k, n.k), (m.tau_s, n.tau_s), (m.theta_s, n.theta_s)] {
                assert!(got > base / 3.0 && got < base * 3.0, "{what}: {m:?}");
            }
        }
        println!(
            "{what}: {} fits accepted, schedule {:?}",
            r.accepted,
            r.schedule.at(225.0)
        );
    }
}
