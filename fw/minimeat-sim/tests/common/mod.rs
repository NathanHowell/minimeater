//! Closed-loop scenario harness shared by the supervision tests (lid, fire): the plant on a noisy
//! pit probe, the nominal firmware-side model, either controller, and the supervisors as the
//! firmware wires them (`.4.14`).
#![allow(dead_code, reason = "each test binary uses part of the harness")]

use minimeat_core::control::{Feedforward, FfPi, Fire, FireObs, FireWatch, Mpc, Obs, PitController, Schedule};
use minimeat_core::model::airflow::{AirPath, AirflowMap, ModelFeedforward};
use minimeat_core::model::{Ambient, Egg, top_vent};
use minimeat_core::units::c_to_k;
use minimeat_sim::{Actuators, Dynamics, Environment, Fuel, Plant, Rng};

pub const DT: f64 = 10.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Lid {
    /// The controller is told exactly when the lid is up (the Python reference's view).
    Oracle,
    /// The controller has to work it out from the pit probe.
    Detect,
    /// No lid handling at all.
    Ignore,
}

#[derive(Clone, Copy, Debug)]
pub enum Start {
    Cold,
    Steady,
}

/// Scenario events at a time, s.
#[derive(Clone, Copy, Debug)]
pub enum Ev {
    /// Lid up at a time for a duration.
    LidOpen(f64, f64),
    Setpoint(f64, f64),
    Ambient(f64, Ambient),
    FoodLoad(f64, f64),
    /// The fire goes out.
    Extinguish(f64),
}

/// One run, sampled every control period.
#[derive(Default)]
pub struct Trace {
    pub t: Vec<f64>,
    pub pit: Vec<f64>,
    pub sp: Vec<f64>,
    /// Commanded and reachable airflow, CFM.
    pub q: Vec<f64>,
    pub q_max: Vec<f64>,
    /// The controller thinks the lid is open.
    pub open: Vec<bool>,
    /// The lid really is up.
    pub up: Vec<bool>,
    pub fire: Vec<Fire>,
    /// Heat release, W (plant truth).
    pub fire_w: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct Scenario {
    pub egg: Egg,
    pub mpc: bool,
    pub lid: Lid,
    pub sp: f64,
    pub start: Start,
    pub events: Vec<Ev>,
    pub fuel: Fuel,
    pub hours: f64,
    pub seed: u64,
}

impl Default for Scenario {
    fn default() -> Self {
        Self {
            egg: Egg::default(),
            mpc: true,
            lid: Lid::Detect,
            sp: 225.0,
            start: Start::Steady,
            events: vec![],
            fuel: Fuel::default(),
            hours: 3.0,
            seed: 1,
        }
    }
}

pub fn run(sc: &Scenario) -> Trace {
    let mut env = Environment {
        ambient: Ambient::default(),
        top_area_m2: top_vent::SLIDER_QUARTER,
        lid_open: false,
        food_load_w: 0.0,
    };
    let model = AirPath::default();
    // the firmware's ambient sensor tracks temperature but not wind
    let sensed = |a: &Ambient| Ambient {
        t_k: a.t_k,
        ..Ambient::default()
    };
    let mut map = AirflowMap::build(&model, sensed(&env.ambient));
    let path = AirPath { egg: sc.egg, ..model };
    let mut plant = match sc.start {
        Start::Cold => Plant::cold(path, Dynamics::default(), &env.ambient),
        Start::Steady => Plant::steady(path, Dynamics::default(), &env.ambient, sc.sp, 0.0).expect("reachable"),
    };
    plant.fuel = sc.fuel;
    let mut ctrl = if sc.mpc {
        PitController::Mpc(Mpc::new(Schedule::default()))
    } else {
        PitController::FfPi(FfPi::new(Schedule::default()))
    };
    if sc.lid != Lid::Detect {
        ctrl.lid_mut().detector = None;
    }
    let mut watch = FireWatch::default();
    let mut rng = Rng::new(sc.seed);
    let mut act = Actuators::default();
    let mut sp = sc.sp;
    let mut out = false;
    let mut tr = Trace::default();
    let steps = (sc.hours * 3600.0 / 2.0) as usize;
    for i in 0..steps {
        let t = i as f64 * 2.0;
        env.lid_open = false;
        for e in &sc.events {
            match *e {
                Ev::LidOpen(at, dur) => env.lid_open |= t >= at && t < at + dur,
                Ev::Setpoint(at, v) if t >= at => sp = v,
                Ev::Ambient(at, a) if t >= at => env.ambient = a,
                Ev::FoodLoad(at, w) if t >= at => env.food_load_w = w,
                Ev::Extinguish(at) if t >= at && !out => {
                    plant.extinguish();
                    out = true;
                }
                _ => {}
            }
        }
        if i % 5 == 0 {
            if map.stale(&sensed(&env.ambient)) {
                map = AirflowMap::build(&model, sensed(&env.ambient));
            }
            let ff = ModelFeedforward {
                path: &model,
                ambient: sensed(&env.ambient),
            };
            let pit = plant.pit_probe_f() + 0.3 * rng.normal();
            let (q_min, q_max) = map.limits(pit);
            let obs = Obs {
                pit_f: pit,
                sp_f: sp,
                lid: sc.lid == Lid::Oracle && env.lid_open,
                q_min,
                q_max,
            };
            let q = ctrl.step(&obs, &ff, DT);
            let a = map.actuate(pit, q);
            act = Actuators {
                damper: a.damper,
                fan: a.fan,
            };
            let expected_low = ctrl.starting() || ctrl.lid().active();
            let fo = FireObs {
                pit_f: pit,
                sp_f: sp,
                q,
                q_ff: ff.q_cfm(sp),
                q_max,
                expected_low,
            };
            tr.fire.push(watch.update(&fo, DT));
            tr.t.push(t);
            tr.pit.push(pit);
            tr.sp.push(sp);
            tr.q.push(q);
            tr.q_max.push(q_max);
            tr.open.push(ctrl.lid().open());
            tr.up.push(env.lid_open);
            tr.fire_w.push(plant.fire_w());
        }
        plant.step(2.0, act, &env);
    }
    tr
}

const H: f64 = 3600.0;

/// What an egg does on a normal day, besides the lid and the fire failing: none of it may raise a
/// supervisor (lid, fire). Egg, controller and seed are left at their defaults.
pub fn everyday() -> Vec<(&'static str, Scenario)> {
    let cold_front = Ambient {
        t_k: c_to_k(0.0),
        wind_m_s: 6.0,
    };
    let at = |sp: f64, start: Start, events: Vec<Ev>, hours: f64| Scenario {
        sp,
        start,
        events,
        hours,
        ..Scenario::default()
    };
    vec![
        ("light-off 150F", at(150.0, Start::Cold, vec![], 3.0)),
        ("light-off 325F", at(325.0, Start::Cold, vec![], 3.0)),
        (
            "setpoint 325->150F",
            at(325.0, Start::Steady, vec![Ev::Setpoint(0.5 * H, 150.0)], 4.0),
        ),
        (
            "setpoint 225->325->225F",
            at(
                225.0,
                Start::Steady,
                vec![Ev::Setpoint(0.5 * H, 325.0), Ev::Setpoint(2.0 * H, 225.0)],
                4.0,
            ),
        ),
        (
            "cold front 0C 6 m/s",
            at(225.0, Start::Steady, vec![Ev::Ambient(0.5 * H, cold_front)], 3.0),
        ),
        (
            "cold front at 150F",
            at(150.0, Start::Steady, vec![Ev::Ambient(0.5 * H, cold_front)], 3.0),
        ),
        (
            "food load 400 W",
            at(225.0, Start::Steady, vec![Ev::FoodLoad(0.5 * H, 400.0)], 3.0),
        ),
    ]
}
