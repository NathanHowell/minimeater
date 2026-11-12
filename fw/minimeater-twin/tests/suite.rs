//! The scenario suite (`.4.16.3`): every scenario in `Scenario::NAMES` runs on the twin, each in its
//! own process (the twin is one run per process), and its control metrics must meet the scenario's
//! bounds (`suite::Expect`) and not be worse than `baselines.json`.
//!
//! `just fw::twin::scenarios-update` rewrites the baselines
//! from this run, after a change that is meant to move them. The CSV of each run is left in
//! `target/tmp/scenarios/` for `scripts/plot_scenarios.py`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use minimeater_twin::Scenario;
use minimeater_twin::metrics::Metrics;
use minimeater_twin::suite::Expect;
use serde_json::{Map, Value, json};

/// How much worse than its baseline a metric may get: a relative part and an absolute slack.
const REL: f64 = 0.10;
const SLACK: [f64; 9] = [3.0, 1.5, 0.25, 0.75, 3.0, 1.5, 15.0, 3.0, 0.5];

struct Run {
    name: &'static str,
    values: [Option<f64>; 9],
    events: Vec<(String, usize)>,
}

fn run(name: &'static str, dir: &std::path::Path) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_minimeater-twin"))
        .args(["--scenario", name, "--metrics", "--csv"])
        .arg(dir.join(format!("{name}.csv")))
        .output()
        .expect("twin binary runs");
    assert!(out.status.success(), "{name}: {}", String::from_utf8_lossy(&out.stderr));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{name}: {e}"));
    let m = &v["metrics"];
    let values = Metrics::NAMES.map(|n| m[n].as_f64());
    let events = v["events"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, c)| (k.clone(), c.as_u64().unwrap() as usize))
        .collect();
    Run { name, values, events }
}

fn metrics(values: &[Option<f64>; 9]) -> Metrics {
    let v = values.map(|v| v.unwrap_or(f64::NAN));
    Metrics {
        reach_min: v[0],
        overshoot_f: v[1],
        hold_rms_f: v[2],
        hold_max_f: v[3],
        lid_recovery_min: v[4],
        lid_overshoot_f: v[5],
        servo_deg_per_h: v[6],
        fan_on_pct: v[7],
        fault_min: v[8],
    }
}

#[test]
fn every_scenario_meets_its_bounds_and_its_baseline() {
    assert!(Scenario::NAMES.len() >= 12, "the suite has at least 12 scenarios");
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("scenarios");
    std::fs::create_dir_all(&dir).unwrap();
    let runs: Vec<Run> = std::thread::scope(|s| {
        let hs: Vec<_> = Scenario::NAMES.iter().map(|n| s.spawn(|| run(n, &dir))).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("baselines.json");
    if std::env::var_os("UPDATE_BASELINES").is_some() {
        let all: BTreeMap<_, _> = runs
            .iter()
            .map(|r| {
                let m: Map<_, _> = Metrics::NAMES
                    .iter()
                    .zip(r.values)
                    .map(|(n, v)| {
                        (
                            n.to_string(),
                            v.map_or(Value::Null, |v| json!((v * 1000.0).round() / 1000.0)),
                        )
                    })
                    .collect();
                (r.name, Value::Object(m))
            })
            .collect();
        std::fs::write(&path, serde_json::to_string_pretty(&all).unwrap() + "\n").unwrap();
        println!("baselines written to {}", path.display());
    }
    let base: Value = serde_json::from_slice(&std::fs::read(&path).expect("baselines.json")).unwrap();

    let mut failures = Vec::new();
    for r in &runs {
        let m = metrics(&r.values);
        let mut bad = Expect::of(r.name)
            .unwrap_or_else(|| panic!("{}: no Expect", r.name))
            .check(&m, &r.events);
        match base.get(r.name) {
            None => bad.push("no baseline (UPDATE_BASELINES=1)".into()),
            Some(b) => {
                for (i, n) in Metrics::NAMES.iter().enumerate() {
                    match (b[*n].as_f64(), r.values[i]) {
                        (Some(b), Some(v)) if v > b * (1.0 + REL) + SLACK[i] => {
                            bad.push(format!("{n} {v:.2} against a baseline of {b:.2}"));
                        }
                        (Some(b), None) => bad.push(format!("{n} is gone, the baseline was {b:.2}")),
                        _ => {}
                    }
                }
            }
        }
        println!(
            "{:15} {}",
            r.name,
            if bad.is_empty() {
                "ok".to_string()
            } else {
                bad.join("; ")
            }
        );
        if !bad.is_empty() {
            failures.push(format!("{}: {}", r.name, bad.join("; ")));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
