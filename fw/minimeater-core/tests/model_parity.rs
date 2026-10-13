//! The firmware-side egg model against the Python reference at sample points
//! (`uv run minimeater-fixtures` -> tests/fixtures/model.json).

use minimeater_core::model::airflow::{AirPath, AirflowMap};
use minimeater_core::model::thermal::steady_state;
use minimeater_core::model::{Ambient, Egg};
use minimeater_core::units::{CFM, c_to_k, f_to_k};
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/model.json")).unwrap()
}

fn f(v: &Value, k: &str) -> f64 {
    v[k].as_f64().unwrap()
}

fn ambient(v: &Value) -> Ambient {
    Ambient {
        t_k: c_to_k(f(v, "amb_c")),
        wind_m_s: f(v, "wind"),
    }
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-3)
}

#[test]
fn air_flow_matches() {
    let path = AirPath::default();
    for p in fixture()["air_flow"].as_array().unwrap() {
        let q = path.air_flow(&ambient(p), f(p, "s"), f(p, "damper"), f_to_k(f(p, "pit_f"))) / CFM;
        assert!(rel(q, f(p, "cfm")) < 1e-6, "{p}: got {q}");
    }
}

#[test]
fn steady_state_matches() {
    for p in fixture()["steady_state"].as_array().unwrap() {
        let ss = steady_state(&Egg::default(), &ambient(p), f_to_k(f(p, "pit_f")), f(p, "food_w")).unwrap();
        assert!(rel(ss.m_air_kg_s, f(p, "m_air")) < 1e-6, "{p}: m_air {}", ss.m_air_kg_s);
        assert!(
            (ss.t_wall_out_k - f(p, "t_wo")).abs() < 1e-4,
            "{p}: t_wo {}",
            ss.t_wall_out_k
        );
        assert!(
            (ss.t_wall_in_k - f(p, "t_wi")).abs() < 1e-4,
            "{p}: t_wi {}",
            ss.t_wall_in_k
        );
    }
}

#[test]
fn airflow_map_matches() {
    let path = AirPath::default();
    let mut map: Option<(Ambient, AirflowMap)> = None;
    for p in fixture()["airflow_map"].as_array().unwrap() {
        let amb = ambient(p);
        if map.as_ref().is_none_or(|(a, _)| *a != amb) {
            map = Some((amb, AirflowMap::build(&path, amb)));
        }
        let m = &map.as_ref().unwrap().1;
        let pit = f(p, "pit_f");
        let (lo, hi) = m.limits(pit);
        // f32 table storage: ~1e-7 relative
        assert!(
            rel(lo, f(p, "q_min")) < 1e-5 && rel(hi, f(p, "q_max")) < 1e-5,
            "{p}: limits {lo} {hi}"
        );
        let a = m.actuate(pit, f(p, "q"));
        assert!((a.damper - f(p, "damper")).abs() < 1e-4, "{p}: damper {}", a.damper);
        assert!((a.fan - f(p, "fan")).abs() < 1e-4, "{p}: fan {}", a.fan);
    }
}
