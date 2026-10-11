//! Closed-loop behaviour of the plant with the real core controller and actuator inversion: the
//! physics extensions (food stall, fuel, fire-out) must look like real cooks, and runs must be
//! deterministic given the seed.

use minimeat_core::control::{Mpc, Obs};
use minimeat_core::model::Ambient;
use minimeat_core::model::airflow::{AirPath, AirflowMap, ModelFeedforward};
use minimeat_core::model::top_vent;
use minimeat_sim::{Actuators, Dynamics, Environment, Fuel, Meat, Plant, Rng};

struct Run {
    pit_f: Vec<f64>,
    food_f: Vec<f64>,
    disturbance_f: Vec<f64>,
}

/// Hold `sp` with the MPC for `hours`, sampling every 10 s control period.
fn hold(mut plant: Plant, sp: f64, hours: f64, seed: u64) -> (Plant, Run) {
    let env = Environment {
        ambient: Ambient::default(),
        top_area_m2: top_vent::SLIDER_QUARTER,
        lid_open: false,
        food_load_w: 0.0,
    };
    let path = AirPath::default();
    let map = AirflowMap::build(&path, env.ambient);
    let ff = ModelFeedforward {
        path: &path,
        ambient: env.ambient,
    };
    let mut ctrl = Mpc::default();
    let mut rng = Rng::new(seed);
    let mut act = Actuators::default();
    let mut run = Run {
        pit_f: vec![],
        food_f: vec![],
        disturbance_f: vec![],
    };
    for i in 0..(hours * 1800.0) as usize {
        if i % 5 == 0 {
            let pit = plant.pit_probe_f() + 0.3 * rng.normal();
            let (q_min, q_max) = map.limits(pit);
            let q = ctrl.step(
                &Obs {
                    pit_f: pit,
                    sp_f: sp,
                    lid: false,
                    q_min,
                    q_max,
                },
                &ff,
                10.0,
            );
            let a = map.actuate(pit, q);
            act = Actuators {
                damper: a.damper,
                fan: a.fan,
            };
            run.pit_f.push(pit);
            run.food_f.push(plant.meats[0].map_or(f64::NAN, |m| m.core_f()));
            run.disturbance_f.push(ctrl.disturbance_f());
        }
        plant.step(2.0, act, &env);
    }
    (plant, run)
}

fn steady(sp: f64) -> Plant {
    Plant::steady(AirPath::default(), Dynamics::default(), &Ambient::default(), sp, 0.0).unwrap()
}

#[test]
fn brisket_stalls_and_finishes_under_mpc() {
    let mut plant = steady(250.0);
    plant.meats[0] = Some(Meat::brisket());
    let (_, run) = hold(plant, 250.0, 16.0, 1);
    // samples are 10 s apart: 360 per hour
    let hour = |h: usize| run.food_f[h * 360];
    let stall = (0..13).any(|h| (145.0..175.0).contains(&hour(h)) && hour(h + 3) - hour(h) < 12.0);
    let done_h = run.food_f.iter().position(|&t| t >= 203.0).map(|i| i as f64 / 360.0);
    let pit_dev = run.pit_f[360..].iter().map(|p| (p - 250.0).abs()).fold(0.0, f64::max);
    println!("brisket done at {done_h:.1?} h, pit max deviation {pit_dev:.1}F");
    assert!(stall, "no stall: {:?}", (0..16).map(hour).collect::<Vec<_>>());
    assert!(
        done_h.is_some_and(|h| (10.0..16.0).contains(&h)),
        "done at {done_h:?} h"
    );
    // the meat's cold heat load (~200 W at first) is a disturbance the MPC must absorb
    assert!(pit_dev < 8.0, "pit strayed {pit_dev:.1}F while the brisket loaded it");
}

#[test]
fn fuel_burnout_drops_pit_and_shows_in_disturbance() {
    let mut plant = steady(250.0);
    plant.fuel = Fuel {
        kg: 1.0,
        ..Fuel::default()
    };
    let (plant, run) = hold(plant, 250.0, 14.0, 2);
    assert!(plant.fuel.kg < 0.05, "fuel left {:.2} kg", plant.fuel.kg);
    let dropped = run
        .pit_f
        .iter()
        .position(|&p| p < 225.0)
        .expect("pit never dropped as fuel ran out");
    let hours = dropped as f64 / 360.0;
    // ~0.07-0.3 kg/h at 250F => 1 kg lasts roughly 3-14 h
    println!("1 kg of lump held 250F for {hours:.1} h");
    assert!((3.0..14.0).contains(&hours), "1 kg lasted {hours:.1} h");
    // the MPC sees "more air but no heat": the disturbance estimate goes strongly negative (.4.19)
    let d_at_drop = run.disturbance_f[dropped];
    assert!(d_at_drop < -20.0, "disturbance at drop {d_at_drop:.1}F");
}

#[test]
fn starved_fire_dies_back_only_when_enabled() {
    let env = Environment {
        ambient: Ambient::default(),
        top_area_m2: top_vent::SLIDER_QUARTER,
        lid_open: false,
        food_load_w: 0.0,
    };
    let shut = Actuators { damper: 0.0, fan: 0.0 };
    let mut on = steady(225.0);
    on.fuel.sustain_w = 600.0;
    let mut off = steady(225.0);
    for _ in 0..1800 {
        on.step(2.0, shut, &env);
        off.step(2.0, shut, &env);
    }
    assert!(on.lit() < 0.1, "lit {:.2}", on.lit());
    assert!((off.lit() - 1.0).abs() < 1e-9);
}

#[test]
fn deterministic_given_seed() {
    let a = hold(steady(225.0), 225.0, 1.0, 9).1.pit_f;
    let b = hold(steady(225.0), 225.0, 1.0, 9).1.pit_f;
    let c = hold(steady(225.0), 225.0, 1.0, 10).1.pit_f;
    assert_eq!(a, b);
    assert_ne!(a, c);
}
