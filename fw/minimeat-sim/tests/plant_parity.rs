//! Open-loop scripted run against the Python plant (`uv run minimeat-fixtures` -> fixtures/plant.json):
//! same actuation script and events, compared every 60 s.

use minimeat_core::model::airflow::AirPath;
use minimeat_core::model::{Ambient, top_vent};
use minimeat_core::units::c_to_k;
use minimeat_sim::{Actuators, Dynamics, Environment, Plant};
use serde_json::Value;

#[test]
fn matches_python_plant() {
    let fx: Value = serde_json::from_str(include_str!("fixtures/plant.json")).unwrap();
    let script: Vec<(f64, f64, f64)> = fx["script"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["t"].as_f64().unwrap(),
                s["damper"].as_f64().unwrap(),
                s["s"].as_f64().unwrap(),
            )
        })
        .collect();
    let col = |k: &str| -> Vec<f64> { fx[k].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect() };
    let (pit, air, fire) = (col("pit_f"), col("air_cfm"), col("fire_w"));
    let dt = fx["dt"].as_f64().unwrap();
    let every = fx["every"].as_u64().unwrap() as usize;

    let mut env = Environment {
        ambient: Ambient::default(),
        top_area_m2: top_vent::SLIDER_QUARTER,
        lid_open: false,
        food_load_w: 0.0,
    };
    let mut plant = Plant::cold(AirPath::default(), Dynamics::default(), &env.ambient);
    let mut act = Actuators::default();
    let (mut worst_pit, mut worst_air, mut worst_fire) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..pit.len() * every {
        let t = i as f64 * dt;
        // events (sim.py applies them before the step at that time)
        env.lid_open = (5400.0..5520.0).contains(&t);
        if t >= 6000.0 {
            env.food_load_w = 150.0;
        }
        if t >= 9600.0 {
            env.ambient = Ambient {
                t_k: c_to_k(5.0),
                wind_m_s: 4.0,
            };
        }
        if t >= 11000.0 {
            env.top_area_m2 = top_vent::SLIDER_HALF;
        }
        if i % 5 == 0 {
            // 10 s control period
            let (_, d, s) = *script.iter().rev().find(|(t0, _, _)| t >= *t0).unwrap();
            act = Actuators { damper: d, fan: s };
        }
        plant.step(dt, act, &env);
        if i % every == 0 {
            let k = i / every;
            worst_pit = worst_pit.max((plant.pit_probe_f() - pit[k]).abs());
            worst_air = worst_air.max((plant.air_cfm() - air[k]).abs());
            worst_fire = worst_fire.max((plant.fire_w() - fire[k]).abs());
        }
    }
    println!("max |d pit| {worst_pit:.2e} F, |d air| {worst_air:.2e} CFM, |d fire| {worst_fire:.2e} W");
    assert!(worst_pit < 1e-6 && worst_air < 1e-8 && worst_fire < 1e-5);
}
