//! Lid-open detection and recovery (`.4.6`) in closed loop on the plant model: the detector, seeing
//! only the noisy pit probe, catches every lid opening of 20 s or more within 30 s, lets go once the
//! lid is shut, and recovers about as well as if the controller had been told about the lid. Nothing
//! else an egg does (light-off, setpoint moves, weather, food, a dying fire) opens it.

use minimeat_core::control::{FfPi, Mpc, Obs, PitController, Schedule};
use minimeat_core::model::airflow::{AirPath, AirflowMap, ModelFeedforward};
use minimeat_core::model::{Ambient, Egg, top_vent};
use minimeat_core::units::c_to_k;
use minimeat_sim::{Actuators, Dynamics, Environment, Fuel, Plant, Rng, uncertainty_band};

const DT: f64 = 10.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Lid {
    /// The controller is told exactly when the lid is up (the Python reference's view).
    Oracle,
    /// The controller has to work it out from the pit probe.
    Detect,
    /// No lid handling at all.
    Ignore,
}

#[derive(Clone, Copy)]
enum Start {
    Cold,
    Steady,
}

/// Scenario events at a time, s.
#[derive(Clone, Copy)]
enum Ev {
    LidOpen(f64, f64),
    Setpoint(f64, f64),
    Ambient(f64, Ambient),
    FoodLoad(f64, f64),
}

struct Trace {
    t: Vec<f64>,
    pit: Vec<f64>,
    open: Vec<bool>,
    sp: Vec<f64>,
    /// The lid really is up.
    up: Vec<bool>,
}

#[allow(clippy::too_many_arguments)]
fn run(
    egg: Egg,
    mpc: bool,
    lid: Lid,
    sp0: f64,
    start: Start,
    events: &[Ev],
    fuel: Fuel,
    hours: f64,
    seed: u64,
) -> Trace {
    let mut env = Environment {
        ambient: Ambient::default(),
        top_area_m2: top_vent::SLIDER_QUARTER,
        lid_open: false,
        food_load_w: 0.0,
    };
    let model = AirPath::default();
    let map = AirflowMap::build(&model, env.ambient);
    let ff = ModelFeedforward {
        path: &model,
        ambient: env.ambient,
    };
    let path = AirPath { egg, ..model };
    let mut plant = match start {
        Start::Cold => Plant::cold(path, Dynamics::default(), &env.ambient),
        Start::Steady => Plant::steady(path, Dynamics::default(), &env.ambient, sp0, 0.0).expect("reachable"),
    };
    plant.fuel = fuel;
    let mut ctrl = if mpc {
        PitController::Mpc(Mpc::new(Schedule::default()))
    } else {
        PitController::FfPi(FfPi::new(Schedule::default()))
    };
    if lid != Lid::Detect {
        ctrl.lid_mut().detector = None;
    }
    let mut rng = Rng::new(seed);
    let mut act = Actuators::default();
    let mut sp = sp0;
    let mut tr = Trace {
        t: vec![],
        pit: vec![],
        open: vec![],
        sp: vec![],
        up: vec![],
    };
    let steps = (hours * 3600.0 / 2.0) as usize;
    for i in 0..steps {
        let t = i as f64 * 2.0;
        env.lid_open = false;
        for e in events {
            match *e {
                Ev::LidOpen(at, dur) => env.lid_open |= t >= at && t < at + dur,
                Ev::Setpoint(at, v) if t >= at => sp = v,
                Ev::Ambient(at, a) if t >= at => env.ambient = a,
                Ev::FoodLoad(at, w) if t >= at => env.food_load_w = w,
                _ => {}
            }
        }
        if i % 5 == 0 {
            let pit = plant.pit_probe_f() + 0.3 * rng.normal();
            let (q_min, q_max) = map.limits(pit);
            let obs = Obs {
                pit_f: pit,
                sp_f: sp,
                lid: lid == Lid::Oracle && env.lid_open,
                q_min,
                q_max,
            };
            let a = map.actuate(pit, ctrl.step(&obs, &ff, DT));
            act = Actuators {
                damper: a.damper,
                fan: a.fan,
            };
            tr.t.push(t);
            tr.pit.push(pit);
            tr.open.push(ctrl.lid().open());
            tr.sp.push(sp);
            tr.up.push(env.lid_open);
        }
        plant.step(2.0, act, &env);
    }
    tr
}

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
                    let ev = [Ev::LidOpen(LID_AT, dur)];
                    let go = |lid| {
                        let tr = run(egg, mpc, lid, sp, Start::Steady, &ev, Fuel::default(), 2.5, 3);
                        recovery(&tr, LID_AT, dur)
                    };
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
                    let early = early_release(&run(
                        egg,
                        mpc,
                        Lid::Detect,
                        sp,
                        Start::Steady,
                        &ev,
                        Fuel::default(),
                        2.5,
                        3,
                    ));
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

/// Name, setpoint, start, events, fuel, hours.
type Scenario<'a> = (&'a str, f64, Start, &'a [Ev], Fuel, f64);

/// Everything else an egg does, with the detector on and the lid shut throughout.
#[test]
fn nothing_but_the_lid_opens_it() {
    let h = 3600.0;
    let cold_front = Ambient {
        t_k: c_to_k(0.0),
        wind_m_s: 6.0,
    };
    #[rustfmt::skip]
    let scenarios: [Scenario; 8] = [
        ("light-off 150F", 150.0, Start::Cold, &[], Fuel::default(), 3.0),
        ("light-off 325F", 325.0, Start::Cold, &[], Fuel::default(), 3.0),
        (
            "setpoint 325->150F",
            325.0,
            Start::Steady,
            &[Ev::Setpoint(0.5 * h, 150.0)],
            Fuel::default(),
            4.0,
        ),
        (
            "setpoint 225->325->225F",
            225.0,
            Start::Steady,
            &[Ev::Setpoint(0.5 * h, 325.0), Ev::Setpoint(2.0 * h, 225.0)],
            Fuel::default(),
            4.0,
        ),
        (
            "cold front 0C 6 m/s",
            225.0,
            Start::Steady,
            &[Ev::Ambient(0.5 * h, cold_front)],
            Fuel::default(),
            3.0,
        ),
        (
            "cold front at 150F",
            150.0,
            Start::Steady,
            &[Ev::Ambient(0.5 * h, cold_front)],
            Fuel::default(),
            3.0,
        ),
        (
            "food load 400 W",
            225.0,
            Start::Steady,
            &[Ev::FoodLoad(0.5 * h, 400.0)],
            Fuel::default(),
            3.0,
        ),
        (
            "fuel runs out (fire dies)",
            250.0,
            Start::Steady,
            &[],
            Fuel {
                kg: 0.5,
                ..Fuel::default()
            },
            8.0,
        ),
    ];
    for (name, sp, start, ev, fuel, hours) in scenarios {
        for (band, egg) in uncertainty_band(Egg::default()) {
            for mpc in [true, false] {
                for seed in [1, 2] {
                    let tr = run(egg, mpc, Lid::Detect, sp, start, ev, fuel, hours, seed);
                    let opened = tr.open.iter().position(|&o| o);
                    let lo = tr.pit.iter().copied().fold(f64::INFINITY, f64::min);
                    println!("{name:24} {band:9} mpc {mpc:5} seed {seed}: low {lo:5.1}F, opened {opened:?}");
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
