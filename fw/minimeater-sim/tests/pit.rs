//! The control loop (`.4.14`) on the plant model, wired as the firmware wires it: settings through
//! the config store on the simulated flash, autotune fits on a slow side task, the probe pipeline's
//! `None` for a pit probe that is not `ok`.
//!
//! - A power cut at any point of a cook, from seconds to an hour, resumes it from flash without
//!   overshoot and without a false lid or fire alarm.
//! - A pulled pit probe closes the flap and stops the fan within the grace period, and the cook picks
//!   up again without overshoot when it is plugged back in; a missed reading changes nothing.
//! - Manual -> run is bumpless.
//! - Autotune runs end to end through the loop with the fit off the control path, survives a power
//!   cut, and reports `autotune_done`.

use core::cell::RefCell;

use embassy_futures::block_on;
use minimeater_app::config::{ConfigStore, ControllerKind, Mode, Settings};
use minimeater_app::pit::{Command, Event, FaultCode, Output, PERIOD_S, Phase, Pit};
use minimeater_core::control::autotune::{Due, Identifier};
use minimeater_core::control::{Fire, Schedule};
use minimeater_core::model::airflow::{AirPath, AirflowMap};
use minimeater_core::model::{Ambient, Egg, top_vent};
use minimeater_sim::hal::{SimConfig, World, io};
use minimeater_sim::{Actuators, Dynamics, Environment, Plant, Rng, uncertainty_band};

const H: f64 = 3600.0;

fn env() -> Environment {
    Environment {
        ambient: Ambient::default(),
        top_area_m2: top_vent::SLIDER_QUARTER,
        lid_open: false,
        food_load_w: 0.0,
    }
}

/// The board's flash; its plant is not used.
fn flash() -> RefCell<World> {
    RefCell::new(World::new(
        Plant::cold(AirPath::default(), Dynamics::default(), &Ambient::default()),
        env(),
    ))
}

#[derive(Default)]
struct Trace {
    t: Vec<f64>,
    /// Pit probe as the plant has it (no noise), F.
    pit: Vec<f64>,
    out: Vec<Output>,
    events: Vec<(f64, Event)>,
}

impl Trace {
    fn max_pit(&self, from: f64, to: f64) -> f64 {
        self.window(from, to)
            .map(|k| self.pit[k])
            .fold(f64::NEG_INFINITY, f64::max)
    }

    fn window(&self, from: f64, to: f64) -> impl Iterator<Item = usize> + '_ {
        (0..self.t.len()).filter(move |&k| self.t[k] >= from && self.t[k] < to)
    }

    /// Minutes from `from` until the pit is within `band` of `sp`.
    fn back_within(&self, from: f64, sp: f64, band: f64) -> f64 {
        self.window(from, f64::INFINITY)
            .find(|&k| (self.pit[k] - sp).abs() < band)
            .map_or(f64::INFINITY, |k| (self.t[k] - from) / 60.0)
    }

    fn events_after(&self, from: f64) -> impl Iterator<Item = &Event> {
        self.events.iter().filter(move |(t, _)| *t >= from).map(|(_, e)| e)
    }
}

/// The board: the loop, its store, and a slow task that returns each fit `fit_lag` periods after
/// the window came due.
struct Rig<'w> {
    flash: &'w RefCell<World>,
    store: ConfigStore<SimConfig<'w>>,
    settings: Settings,
    pit: Pit,
    plant: Plant,
    env: Environment,
    model: AirPath,
    rng: Rng,
    t: f64,
    act: Actuators,
    fit_lag: usize,
    job: Option<(Due, Box<Identifier>, usize)>,
    tr: Trace,
}

impl<'w> Rig<'w> {
    fn new(flash: &'w RefCell<World>, plant: Plant, seed: u64) -> Self {
        let (store, settings) = block_on(ConfigStore::open(io(flash).config));
        let model = AirPath::default();
        let env = env();
        Self {
            flash,
            store,
            pit: Pit::new(&settings, model, env.ambient),
            settings,
            plant,
            env,
            model,
            rng: Rng::new(seed),
            t: 0.0,
            act: Actuators::default(),
            fit_lag: 3,
            job: None,
            tr: Trace::default(),
        }
    }

    fn command(&mut self, c: Command) {
        self.pit.command(c).expect("accepted");
        self.save();
    }

    fn save(&mut self) {
        self.pit.persist(&mut self.settings);
        block_on(self.store.save(&self.settings)).expect("saved");
    }

    /// One control period; `probe`: the pit probe reads `ok`.
    fn period(&mut self, probe: bool) {
        let y = probe.then(|| self.plant.pit_probe_f() + 0.3 * self.rng.normal());
        let (out, events) = self.pit.step(y, self.env.ambient, PERIOD_S);
        for e in events {
            self.tr.events.push((self.t, *e));
        }
        self.tr.t.push(self.t);
        self.tr.pit.push(self.plant.pit_probe_f());
        self.tr.out.push(out);
        self.act = Actuators {
            damper: out.act.damper,
            fan: out.act.fan,
        };
        // the slow task
        if let Some((due, id)) = self.pit.take_fit() {
            self.job = Some((due, Box::new(id.clone()), self.fit_lag));
        }
        if let Some((due, id, left)) = self.job.take() {
            if left == 0 {
                self.pit.fitted(due, id.fit());
            } else {
                self.job = Some((due, id, left - 1));
            }
        }
        if let Some((path, amb)) = self.pit.map_wanted() {
            self.pit.set_map(AirflowMap::build(&path, amb));
        }
        self.save();
        for _ in 0..5 {
            self.plant.step(2.0, self.act, &self.env);
        }
        self.t += PERIOD_S;
    }

    fn run(&mut self, s: f64) {
        self.run_with(s, |_| true);
    }

    fn run_with(&mut self, s: f64, probe: impl Fn(f64) -> bool) {
        let end = self.t + s;
        while self.t < end - 1e-9 {
            self.period(probe(self.t));
        }
    }

    /// The board loses power for `s`: the servo holds where it was (no power, no torque, but the
    /// linkage friction keeps it), the fan stops, a fit in progress is lost. Then it boots from flash.
    fn power_cut(&mut self, s: f64) {
        let held = Actuators {
            damper: self.act.damper,
            fan: 0.0,
        };
        let steps = (s / 2.0).round() as usize;
        for _ in 0..steps {
            self.plant.step(2.0, held, &self.env);
        }
        self.t += steps as f64 * 2.0;
        let (store, settings) = block_on(ConfigStore::open(io(self.flash).config));
        self.store = store;
        self.pit = Pit::new(&settings, self.model, self.env.ambient);
        self.settings = settings;
        self.job = None;
        self.act = Actuators::default();
    }
}

fn cooking(flash: &RefCell<World>, egg: Egg, sp: f64, kind: ControllerKind, seed: u64) -> Rig<'_> {
    let path = AirPath {
        egg,
        ..AirPath::default()
    };
    let plant = Plant::steady(path, Dynamics::default(), &env().ambient, sp, 0.0).expect("reachable");
    let mut rig = Rig::new(flash, plant, seed);
    rig.command(Command::Sp(sp));
    rig.command(Command::Controller(kind));
    rig.command(Command::Mode(Mode::Run));
    rig
}

/// Resume overshoot bound, F: the MPC (default) holds 5F. FF+PI is the fallback: an 80F climb back on
/// a low-loss egg at 325F peaks at 12F, so it gets 15F (its light-off bound is 40F).
fn bound(kind: ControllerKind) -> f64 {
    match kind {
        ControllerKind::Mpc => 5.0,
        ControllerKind::FfPi => 15.0,
    }
}

fn alarm(e: &Event) -> bool {
    matches!(
        e,
        Event::LidOpen { .. } | Event::FireLow { .. } | Event::FireOut { .. } | Event::Fault(_)
    )
}

#[test]
fn a_power_cut_resumes_the_cook_from_flash() {
    let mut worst = [(f64::NEG_INFINITY, 0.0f64); 2];
    for sp in [150.0, 225.0, 325.0] {
        for (band, egg) in uncertainty_band(Egg::default()) {
            for kind in [ControllerKind::Mpc, ControllerKind::FfPi] {
                for cut in [10.0, 120.0, 900.0, 3600.0] {
                    let flash = flash();
                    let mut rig = cooking(&flash, egg, sp, kind, 1);
                    rig.run(H);
                    let before = rig.tr.max_pit(0.5 * H, H);
                    rig.power_cut(cut);
                    let back = rig.t;
                    let low = rig.plant.pit_probe_f();
                    assert_eq!(rig.pit.cook().mode, Mode::Run);
                    assert_eq!(rig.pit.cook().sp_f, sp);
                    assert_eq!(rig.pit.cook().controller, kind);
                    rig.run(2.0 * H);
                    let over = rig.tr.max_pit(back, back + 2.0 * H) - sp;
                    let mins = rig.tr.back_within(back, sp, 5.0);
                    let alarms: Vec<_> = rig.tr.events_after(back).filter(|e| alarm(e)).collect();
                    println!(
                        "{sp}F {band:9} {kind:?} cut {cut:4.0}s: resumed at {low:5.1}F, peak {over:+5.1}F \
                         (before {:+4.1}), within 5F after {mins:4.1} min",
                        before - sp
                    );
                    assert!(over <= bound(kind), "{sp} {band} {kind:?} {cut}: overshoot {over:.1}F");
                    assert!(mins <= 60.0, "{sp} {band} {kind:?} {cut}: {mins:.1} min to recover");
                    assert!(alarms.is_empty(), "{sp} {band} {kind:?} {cut}: {alarms:?}");
                    let w = &mut worst[kind as usize];
                    w.0 = w.0.max(over);
                    w.1 = w.1.max(mins);
                }
            }
        }
    }
    println!("worst (mpc, ffpi): {worst:.1?} (peak F, min to recover)");
}

#[test]
fn a_pulled_pit_probe_fails_safe_and_the_cook_resumes() {
    for sp in [150.0, 225.0, 325.0] {
        for (band, egg) in uncertainty_band(Egg::default()) {
            for kind in [ControllerKind::Mpc, ControllerKind::FfPi] {
                let flash = flash();
                let mut rig = cooking(&flash, egg, sp, kind, 2);
                rig.run(H);
                // a single missed reading: nothing moves
                let held = *rig.pit.output();
                rig.period(false);
                assert_eq!(rig.tr.out.last().unwrap().act, held.act);
                assert_eq!(rig.tr.out.last().unwrap().phase, held.phase);
                rig.run(H);
                // pulled for 10 min
                let (from, to) = (rig.t, rig.t + 600.0);
                rig.run_with(1200.0, |t| !(from..to).contains(&t));
                let safe = rig
                    .tr
                    .window(from, to)
                    .find(|&k| rig.tr.out[k].phase == Phase::Fault)
                    .map(|k| rig.tr.t[k] - from);
                let closed = rig
                    .tr
                    .window(from + 30.0, to)
                    .all(|k| rig.tr.out[k].act.damper == 0.0 && rig.tr.out[k].act.fan == 0.0);
                rig.run(2.0 * H);
                let over = rig.tr.max_pit(to, rig.t) - sp;
                let mins = rig.tr.back_within(to, sp, 5.0);
                let faults: Vec<_> = rig
                    .tr
                    .events_after(from)
                    .filter(|e| matches!(e, Event::Fault(_) | Event::FaultClear(_)))
                    .collect();
                println!(
                    "{sp}F {band:9} {kind:?}: fault after {safe:?} s, dropped to {:5.1}F, peak {over:+5.1}F, \
                     within 5F {mins:4.1} min after",
                    rig.tr
                        .window(from, to + 1.0)
                        .map(|k| rig.tr.pit[k])
                        .fold(f64::INFINITY, f64::min)
                );
                assert!(safe.is_some_and(|s| s <= 30.0), "{sp} {band} {kind:?}: {safe:?}");
                assert!(closed, "{sp} {band} {kind:?}: air while faulted");
                assert_eq!(
                    faults,
                    [
                        &Event::Fault(FaultCode::PitProbe),
                        &Event::FaultClear(FaultCode::PitProbe)
                    ]
                );
                assert!(over <= bound(kind), "{sp} {band} {kind:?}: overshoot {over:.1}F");
                assert!(mins <= 60.0, "{sp} {band} {kind:?}: {mins:.1} min to recover");
            }
        }
    }
}

#[test]
fn manual_to_run_is_bumpless() {
    for (band, egg) in uncertainty_band(Egg::default()) {
        for kind in [ControllerKind::Mpc, ControllerKind::FfPi] {
            let flash = flash();
            let mut rig = cooking(&flash, egg, 225.0, kind, 3);
            rig.run(2.0 * H);
            // the user takes over at what the controller was doing
            let a = rig.pit.output().act;
            let flap = minimeater_core::model::actuator::flap_angle_deg(a.damper) / 90.0 * 100.0;
            rig.command(Command::ManualFlap(flap));
            rig.command(Command::ManualFan(a.fan * 100.0));
            rig.command(Command::Mode(Mode::Manual));
            rig.run(1800.0);
            assert_eq!(rig.pit.output().phase, Phase::Manual);
            let back = rig.t;
            let at = rig.plant.pit_probe_f();
            rig.command(Command::Mode(Mode::Run));
            rig.run(H);
            let dev = rig
                .tr
                .window(back, rig.t)
                .map(|k| (rig.tr.pit[k] - 225.0).abs())
                .fold(0.0, f64::max);
            let q0 = rig.tr.out[rig.tr.window(back, rig.t).next().unwrap()].q;
            println!(
                "{band:9} {kind:?}: back to run at {at:5.1}F, q {:.2} -> {q0:.2} CFM, worst {dev:4.1}F off",
                rig.tr.out[rig.tr.window(back - 20.0, back).next().unwrap()].q
            );
            assert!(dev <= (at - 225.0).abs() + 3.0, "{band} {kind:?}: {dev:.1}F");
        }
    }
}

#[test]
fn autotune_runs_through_the_loop_and_survives_a_power_cut() {
    for (band, egg) in uncertainty_band(Egg::default()) {
        let flash = flash();
        let mut rig = cooking(&flash, egg, 225.0, ControllerKind::Mpc, 4);
        assert_eq!(rig.pit.command(Command::Mode(Mode::Off)), Ok(()));
        assert!(rig.pit.command(Command::Autotune).is_err(), "autotune needs run");
        rig.command(Command::Mode(Mode::Run));
        rig.run(600.0);
        rig.command(Command::Autotune);
        rig.run(1.5 * H);
        assert!(rig.settings.cook.autotune, "the dither is saved");
        rig.power_cut(60.0);
        assert!(
            rig.pit.autotune().status().exciting,
            "{band}: the dither resumes after a power cut"
        );
        let resumed = rig.t;
        rig.run(8.0 * H);
        let done: Vec<_> = rig
            .tr
            .events_after(resumed)
            .filter_map(|e| match e {
                Event::AutotuneDone { model, accepted } => Some((*model, *accepted)),
                _ => None,
            })
            .collect();
        let dither = rig
            .tr
            .window(resumed, rig.t)
            .filter(|&k| rig.tr.out[k].ref_f != 225.0)
            .count();
        let worst = rig
            .tr
            .window(resumed + 0.75 * H, rig.t)
            .map(|k| (rig.tr.pit[k] - 225.0).abs())
            .fold(0.0, f64::max);
        let alarms: Vec<_> = rig.tr.events_after(0.0).filter(|e| alarm(e)).collect();
        println!(
            "{band:9}: done {done:?}, dithered {:.1} h after the cut, worst {worst:.1}F off, schedule {:?}",
            dither as f64 * PERIOD_S / H,
            rig.settings.schedule.models()
        );
        assert_eq!(done.len(), 1, "{band}: one autotune_done");
        assert!(done[0].1 > 0, "{band}: learned nothing");
        assert!(!rig.settings.cook.autotune, "{band}: the finished dither is saved");
        // learning carries on after the dither: what is saved is what the controller has now
        assert!(
            rig.settings.schedule == *rig.pit.schedule(),
            "{band}: the learned model is saved"
        );
        assert_ne!(rig.settings.schedule, Schedule::default(), "{band}");
        let learned = rig.settings.schedule.clone();
        assert!(worst <= 8.0, "{band}: {worst:.1}F off");
        assert!(alarms.is_empty(), "{band}: {alarms:?}");
        // and it is still there after the next reboot
        rig.power_cut(10.0);
        assert_eq!(*rig.pit.schedule(), learned);
    }
}

#[test]
fn off_closes_everything_and_commands_are_checked() {
    let flash = flash();
    let mut rig = cooking(&flash, Egg::default(), 225.0, ControllerKind::Mpc, 5);
    rig.run(600.0);
    rig.command(Command::Mode(Mode::Off));
    rig.run(60.0);
    let o = rig.pit.output();
    assert_eq!(
        (o.phase, o.q, o.act.damper, o.act.fan, o.fire),
        (Phase::Idle, 0.0, 0.0, 0.0, Fire::Unknown)
    );
    use minimeater_app::pit::Reject::*;
    for (c, r) in [
        (Command::Sp(149.0), OutOfRange),
        (Command::Sp(326.0), OutOfRange),
        (Command::LidBoost(0.5), OutOfRange),
        (Command::ManualFan(101.0), OutOfRange),
        (Command::Autotune, NotAllowed),
    ] {
        assert_eq!(rig.pit.command(c), Err(r), "{c:?}");
    }
    assert_eq!(rig.pit.cook().sp_f, 225.0);
    rig.command(Command::LidDetect(false));
    rig.power_cut(10.0);
    assert!(!rig.settings.lid.detect);
    assert_eq!(rig.pit.cook().mode, Mode::Off);
}
