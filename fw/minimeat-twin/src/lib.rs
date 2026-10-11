//! Host digital twin (`.4.16.2`): the firmware's application tasks ([`minimeat_app::tasks`]),
//! unchanged, on an embassy executor whose clock is virtual, against the simulated board
//! ([`minimeat_sim::hal`]) and the plant model. The main loop polls the executor until every task
//! waits on a timer, steps the plant up to the next timer in 1 s steps (applying the scenario's
//! events on the way), and moves the clock there: a 20 h cook takes seconds flat out, or runs at a
//! multiple of real time for watching.
//!
//! One run per process: the time driver, the executor and the task pools are statics.
//!
//! Not yet: the network stack, MQTT and OTLP against a local Mosquitto / collector / Home Assistant
//! (waiting on `.5.2` and `.4.11`), and a live plot.

pub mod clock;
pub mod metrics;
pub mod scenario;
pub mod suite;

use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use embassy_executor::raw;
use embassy_futures::block_on;
use minimeat_app::config::{ConfigStore, Settings};
use minimeat_app::food::Foods;
use minimeat_app::pit::{Event, Pit};
use minimeat_app::tasks::{self, Bus, Status};
use minimeat_core::model::airflow::AirPath;
use minimeat_sim::hal::{
    SimAmbient, SimBoard, SimConfig, SimFan, SimProbes, SimServo, SimServoPower, SimTach, World, io,
};

pub use scenario::{Action, Scenario};

type B = SimBoard<'static>;

#[embassy_executor::task]
async fn control(
    ambient: SimAmbient<'static>,
    store: ConfigStore<SimConfig<'static>>,
    settings: Settings,
    pit: &'static mut Pit,
    foods: &'static mut Foods,
    bus: &'static Bus,
) {
    tasks::control::<B>(ambient, store, settings, pit, foods, bus).await
}

#[embassy_executor::task]
async fn probes(adc: SimProbes<'static>, bus: &'static Bus) {
    tasks::probes::<B>(adc, bus).await
}

#[embassy_executor::task]
async fn actuators(
    fan: SimFan<'static>,
    tach: SimTach<'static>,
    servo: SimServo<'static>,
    power: SimServoPower<'static>,
    s_min: f64,
    rpm_max: f64,
    bus: &'static Bus,
) {
    tasks::actuators::<B>(fan, tach, servo, power, s_min, rpm_max, bus).await
}

#[embassy_executor::task]
async fn slow(bus: &'static Bus) {
    tasks::slow(bus).await
}

pub struct Opts {
    /// Multiple of real time; `None` runs flat out.
    pub speed: Option<f64>,
}

/// One control period as the twin saw it.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub t_s: f64,
    /// What the application reported.
    pub status: Status,
    /// Plant truth: the pit probe's temperature and each meat's core, F.
    pub pit_f: f64,
    pub cores_f: [Option<f64>; 3],
    /// The flap's physical angle from closed, degrees.
    pub flap_deg: f64,
}

/// Everything a run produced.
#[derive(Default)]
pub struct Report {
    pub samples: Vec<Sample>,
    pub events: Vec<(f64, Event)>,
}

static RAN: AtomicBool = AtomicBool::new(false);

/// Runs `sc` to its end, calling `watch` with each sample and the events raised since the last.
/// Panics if called twice in one process.
pub fn run(sc: Scenario, opts: &Opts, mut watch: impl FnMut(&Sample, &[(f64, Event)])) -> Report {
    assert!(!RAN.swap(true, Ordering::SeqCst), "one twin run per process");
    let world: &'static RefCell<World> = Box::leak(Box::new(RefCell::new(sc.world())));
    let bus: &'static Bus = Box::leak(Box::new(Bus::new()));
    // the board as the user left it: settings in flash, then a boot
    let (mut store, _) = block_on(ConfigStore::open(io(world).config));
    block_on(store.save(&sc.settings)).expect("settings saved");
    drop(store);
    let io = io(world);
    let (store, settings) = block_on(ConfigStore::open(io.config));
    let model = AirPath::default();
    let pit = Box::leak(Box::new(Pit::new(&settings, model, world.borrow().env.ambient)));
    let foods = Box::leak(Box::new(Foods::new(&settings.probes)));

    let exec: &'static raw::Executor = Box::leak(Box::new(raw::Executor::new(core::ptr::null_mut())));
    let spawner = exec.spawner();
    spawner.spawn(control(io.ambient, store, settings, pit, foods, bus).expect("control"));
    spawner.spawn(probes(io.probes, bus).expect("probes"));
    spawner.spawn(
        actuators(
            io.fan,
            io.tach,
            io.servo,
            io.servo_power,
            model.fan.s_min(),
            model.fan.rpm_max,
            bus,
        )
        .expect("actuators"),
    );
    spawner.spawn(slow(bus).expect("slow"));

    let hz = clock::TICK_HZ;
    let end = (sc.hours * 3600.0 * hz as f64) as u64;
    let mut actions = sc.actions.clone();
    actions.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut actions = actions.into_iter().peekable();
    let (started, mut report, mut new_events) = (Instant::now(), Report::default(), Vec::new());
    let mut phys = 0u64;
    loop {
        // run every task until all of them wait on time
        loop {
            // SAFETY: one executor, polled only from this thread
            unsafe { exec.poll() };
            while let Ok(e) = bus.events.try_receive() {
                new_events.push((phys as f64 / hz as f64, e));
            }
            if let Some(status) = bus.status.try_take() {
                let w = world.borrow();
                let s = Sample {
                    t_s: phys as f64 / hz as f64,
                    status,
                    pit_f: w.plant.pit_probe_f(),
                    cores_f: core::array::from_fn(|i| w.plant.meats[i].map(|m| m.core_f())),
                    flap_deg: w.flap_deg,
                };
                drop(w);
                watch(&s, &new_events);
                report.samples.push(s);
                report.events.append(&mut new_events);
            }
            if !clock::take_pending() {
                break;
            }
        }
        let next = clock::expire();
        if clock::take_pending() {
            // a timer was already due
            continue;
        }
        assert!(next != u64::MAX, "every task is blocked with no timer pending");
        let next = next.min(end);
        while phys < next {
            let step = (next - phys).min(hz);
            phys += step;
            world.borrow_mut().advance(step as f64 / hz as f64);
            while let Some((_, a)) = actions.next_if(|(t, _)| (*t * hz as f64) as u64 <= phys) {
                a.apply(&mut world.borrow_mut(), bus);
            }
        }
        if let Some(speed) = opts.speed {
            let want = Duration::from_secs_f64(phys as f64 / hz as f64 / speed);
            if let Some(ahead) = want.checked_sub(started.elapsed()) {
                std::thread::sleep(ahead);
            }
        }
        clock::set(next);
        if next >= end {
            return report;
        }
    }
}
