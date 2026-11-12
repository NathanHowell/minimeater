//! Replays closed-loop trajectories recorded from the Python reference controllers
//! (`just model::fixtures` -> tests/fixtures/control.json) and checks the Rust port commands the
//! same airflow from the same inputs.

use minimeater_core::control::{FfPi, Fopdt, Mpc, Obs, PitController, Schedule};
use serde_json::Value;

struct Table {
    t0: f64,
    step: f64,
    q: Vec<f64>,
}

impl Table {
    /// np.interp: linear, clamped at the ends.
    fn at(&self, t: f64) -> f64 {
        let x = ((t - self.t0) / self.step).clamp(0.0, (self.q.len() - 1) as f64);
        let i = (x.floor() as usize).min(self.q.len() - 2);
        self.q[i] + (x - i as f64) * (self.q[i + 1] - self.q[i])
    }
}

fn floats(v: &Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

struct Stats {
    max_err: f64,
    mean_err: f64,
    worst_step: usize,
}

fn replay(fixture: &Value, traj: &Value) -> Stats {
    let dt = fixture["dt"].as_f64().unwrap();
    let ff = &fixture["ff"];
    let table = Table {
        t0: ff["t0"].as_f64().unwrap(),
        step: ff["step"].as_f64().unwrap(),
        q: floats(&ff["q"]),
    };
    let models: Vec<Fopdt> = fixture["schedule"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| Fopdt {
            sp_f: m["sp"].as_f64().unwrap(),
            k: m["k"].as_f64().unwrap(),
            tau_s: m["tau"].as_f64().unwrap(),
            theta_s: m["theta"].as_f64().unwrap(),
        })
        .collect();
    let schedule = Schedule::new(&models);
    let mut ctrl = match traj["controller"].as_str().unwrap() {
        "Mpc" => PitController::Mpc(Mpc::new(schedule)),
        "FfPi" => PitController::FfPi(FfPi::new(schedule)),
        other => panic!("unknown controller {other}"),
    };
    // Python sees the true lid flag and has no detector.
    ctrl.lid_mut().detector = None;
    let (pit, sp, q_min, q_max, want) = (
        floats(&traj["pit"]),
        floats(&traj["sp"]),
        floats(&traj["q_min"]),
        floats(&traj["q_max"]),
        floats(&traj["q"]),
    );
    let lid = floats(&traj["lid"]);
    let ffn = |t: f64| table.at(t);
    let (mut max_err, mut sum, mut worst_step) = (0.0, 0.0, 0);
    for k in 0..pit.len() {
        let obs = Obs {
            pit_f: pit[k],
            sp_f: sp[k],
            lid: lid[k] != 0.0,
            q_min: q_min[k],
            q_max: q_max[k],
        };
        let got = ctrl.step(&obs, &ffn, dt);
        // Python actuates lid-recovery airflow beyond q_max at q_max; the Rust API clamps it explicitly.
        let err = (got - want[k].min(q_max[k])).abs();
        sum += err;
        if err > max_err {
            max_err = err;
            worst_step = k;
        }
    }
    Stats {
        max_err,
        mean_err: sum / pit.len() as f64,
        worst_step,
    }
}

#[test]
fn matches_python_reference_trajectories() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/control.json")).expect("fixture parses");
    let mut failures = Vec::new();
    for traj in fixture["trajectories"].as_array().unwrap() {
        let name = format!(
            "{}/{}",
            traj["scenario"].as_str().unwrap(),
            traj["controller"].as_str().unwrap()
        );
        let s = replay(&fixture, traj);
        println!(
            "{name}: max |dq| {:.2e} CFM at step {}, mean {:.2e}",
            s.max_err, s.worst_step, s.mean_err
        );
        // Inputs are rounded to 1e-4..1e-6 in the fixture; outputs agree to a few 1e-6 CFM (the MPC
        // optimum is unique - convex cost - so L-BFGS-B in Python and projected Newton here coincide).
        let tol = 1e-4;
        if s.max_err > tol {
            failures.push(name);
        }
    }
    assert!(failures.is_empty(), "diverged from Python reference: {failures:?}");
}
