//! Host digital twin (`.4.16.2`): the firmware's application tasks ([`minimeater_app::tasks`]),
//! unchanged, on an embassy executor whose clock is virtual, against the simulated board
//! ([`minimeater_sim::hal`]) and the plant model. The main loop polls the executor until every task
//! waits on a timer, steps the plant up to the next timer in 1 s steps (applying the scenario's
//! events on the way), and moves the clock there: a 20 h cook takes seconds flat out, or runs at a
//! multiple of real time for watching.
//!
//! One run per process: the time driver, the executor and the task pools are statics.
//!
//! The application's network clients run too: with an `mqtt` setting the MQTT client talks to an
//! in-process broker ([`net`]: deterministic, any speed, with outages on demand), or with
//! [`NetMode::Real`] (`--mqtt`/`--otlp`, a paced run) to a real Mosquitto and OpenTelemetry
//! collector in containers (`just twin-stack`). Not yet: a live plot.
#![allow(clippy::too_many_arguments)] // the executor tasks take their peripherals as arguments

pub mod clock;
pub mod metrics;
pub mod net;
pub mod scenario;
pub mod suite;

use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use embassy_executor::raw;
use embassy_futures::block_on;
use embassy_sync::pubsub::WaitResult;
use minimeater_app::config::{ConfigStore, Settings};
use minimeater_app::food::Foods;
use minimeater_app::pit::{Event, Pit};
use minimeater_app::proto::BootReason;
use minimeater_app::tasks::{self, Bus, Status};
use minimeater_app::ui;
use minimeater_core::model::airflow::AirPath;
use minimeater_sim::hal::{
    SimAmbient, SimBoard, SimButton, SimConfig, SimFan, SimFanPower, SimLed, SimLoadFault, SimProbes, SimServo,
    SimServoPower, SimTach, SimUsb, SimWatchdog, Wdt, World, io,
};

pub use scenario::{Action, Scenario};

type B = SimBoard<'static>;

#[embassy_executor::task]
async fn control(
    wdt: SimWatchdog<'static>,
    ambient: SimAmbient<'static>,
    store: ConfigStore<SimConfig<'static>>,
    settings: Settings,
    pit: &'static mut Pit,
    foods: &'static mut Foods,
    bus: &'static Bus,
) {
    tasks::control::<B>(wdt, ambient, store, settings, pit, foods, bus).await
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
    fan_power: SimFanPower<'static>,
    load_fault: SimLoadFault<'static>,
    usb: SimUsb<'static>,
    s_min: f64,
    rpm_max: f64,
    bus: &'static Bus,
) {
    tasks::actuators::<B>(fan, tach, servo, power, fan_power, load_fault, usb, s_min, rpm_max, bus).await
}

#[embassy_executor::task]
async fn otlp(
    mut conn: net::RealConn,
    bus: &'static Bus,
    bufs: &'static mut minimeater_app::otlp::Buffers,
    wall0_ns: u64,
) {
    let who = minimeater_app::net::Identity {
        prefix: "minimeater",
        id: "mm-twin01",
        sw: "0.1.0",
        hw: "A",
    };
    // wall-clock time as the cook would see it: the start, plus the twin's own clock
    let unix = move || Some(wall0_ns + (clock::now() as u128 * 1_000_000_000 / clock::TICK_HZ as u128) as u64);
    minimeater_app::otlp::run(&mut conn, &who, bus, bufs, &unix, &|| Some(-60), &|| Some(200_000)).await
}

#[embassy_executor::task]
async fn mqtt(mut conn: net::AnyConn, bus: &'static Bus, bufs: &'static mut minimeater_app::mqtt::Buffers) {
    let who = minimeater_app::mqtt::Identity {
        prefix: "minimeater",
        id: "mm-twin01",
        sw: "0.1.0",
        hw: "A",
    };
    minimeater_app::mqtt::run(&mut conn, &mut minimeater_app::tls::Plain, &who, bus, bufs, &|| {
        Some(-60)
    })
    .await
}

#[embassy_executor::task]
async fn status_http(link: &'static net::Link, bufs: &'static mut minimeater_app::http::Buffers, bus: &'static Bus) {
    let who = minimeater_app::net::Identity {
        prefix: "minimeater",
        id: "mm-twin01",
        sw: "0.1.0",
        hw: "A",
    };
    minimeater_app::http::run(&mut net::TwinListener(link), &who, bus, &|| Some(-60), bufs).await
}

#[embassy_executor::task]
async fn broker(link: &'static net::Link) {
    net::broker(link).await
}

#[embassy_executor::task]
async fn status_led(led: SimLed<'static>, bus: &'static Bus) {
    ui::led(led, bus).await
}

#[embassy_executor::task]
async fn user_button(pin: SimButton<'static>, bus: &'static Bus) {
    ui::button(pin, bus).await
}

#[embassy_executor::task]
async fn slow(bus: &'static Bus) {
    tasks::slow(bus).await
}

pub struct Opts {
    /// Multiple of real time; `None` runs flat out.
    pub speed: Option<f64>,
}

/// Where the application's network clients connect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetMode {
    /// The in-process broker (and no OTLP): deterministic, any speed.
    Memory,
    /// The servers in the settings, over real sockets (Mosquitto, a collector). Use a paced run.
    Real,
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
    /// How many times the status LED has changed state since the start.
    pub led_edges: u32,
}

/// Everything a run produced.
#[derive(Default)]
pub struct Report {
    pub samples: Vec<Sample>,
    pub events: Vec<(f64, Event)>,
    /// The outcome of each request sent with [`Action::Mqtt`] or [`Action::Command`]: time, id, result.
    pub replies: Vec<(f64, u16, Result<(), minimeater_app::proto::Error>)>,
    /// Every time the settings changed enough for `~/config` to go out again, and what they were.
    pub configs: Vec<(f64, Settings)>,
    /// What the broker saw the device do (empty without an `mqtt` setting).
    pub mqtt: Vec<(f64, net::Seen)>,
    /// What the status endpoint was asked and answered ([`Action::Http`]): time, request, the whole response.
    pub http: Vec<(f64, String, Vec<u8>)>,
    /// What the hardware watchdog saw: armed by the control task, and the longest time it went unfed.
    pub wdt: Wdt,
}

static RAN: AtomicBool = AtomicBool::new(false);

/// Runs `sc` to its end, calling `watch` with each sample and the events raised since the last.
/// Panics if called twice in one process.
pub fn run(sc: Scenario, opts: &Opts, watch: impl FnMut(&Sample, &[(f64, Event)])) -> Report {
    run_with(sc, opts, NetMode::Memory, watch)
}

/// Like [`run`], with the network clients' servers chosen.
pub fn run_with(
    sc: Scenario,
    opts: &Opts,
    net_mode: NetMode,
    mut watch: impl FnMut(&Sample, &[(f64, Event)]),
) -> Report {
    clock::register_main_thread();
    assert!(!RAN.swap(true, Ordering::SeqCst), "one twin run per process");
    let world: &'static RefCell<World> = Box::leak(Box::new(RefCell::new(sc.world())));
    let bus: &'static Bus = Box::leak(Box::new(Bus::new()));
    bus.boot.lock(|c| c.set(Some(BootReason::PowerOn)));
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
    spawner.spawn(control(io.watchdog, io.ambient, store, settings, pit, foods, bus).expect("control"));
    spawner.spawn(probes(io.probes, bus).expect("probes"));
    spawner.spawn(
        actuators(
            io.fan,
            io.tach,
            io.servo,
            io.servo_power,
            io.fan_power,
            io.load_fault,
            io.usb,
            model.fan.s_min(),
            model.fan.rpm_max,
            bus,
        )
        .expect("actuators"),
    );
    spawner.spawn(slow(bus).expect("slow"));
    spawner.spawn(status_led(io.led, bus).expect("led"));
    spawner.spawn(user_button(io.button, bus).expect("button"));
    let link: &'static net::Link = Box::leak(Box::new(net::Link::new()));
    if sc.settings.mqtt.is_some() {
        let bufs = Box::leak(Box::new(minimeater_app::mqtt::Buffers::new()));
        let conn = match net_mode {
            NetMode::Memory => {
                spawner.spawn(broker(link).expect("broker"));
                net::AnyConn::Mem(net::TwinConn(link))
            }
            NetMode::Real => net::AnyConn::Real(net::RealConn::new()),
        };
        spawner.spawn(mqtt(conn, bus, bufs).expect("mqtt"));
    }
    if net_mode == NetMode::Memory {
        let bufs = Box::leak(Box::new(minimeater_app::http::Buffers::new()));
        spawner.spawn(status_http(link, bufs, bus).expect("http"));
    }
    if sc.settings.otlp.is_some() && net_mode == NetMode::Real {
        let bufs = Box::leak(Box::new(minimeater_app::otlp::Buffers::new()));
        let wall0 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        spawner.spawn(otlp(net::RealConn::new(), bus, bufs, wall0).expect("otlp"));
    }

    let hz = clock::TICK_HZ;
    let end = (sc.hours * 3600.0 * hz as f64) as u64;
    let mut actions = sc.actions.clone();
    actions.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut actions = actions.into_iter().peekable();
    let (started, mut report, mut new_events) = (Instant::now(), Report::default(), Vec::new());
    let mut phys = 0u64;
    // the harness is one of the bus's readers
    let mut events = bus.events.subscriber().expect("a reader slot");
    let mut replies = bus.replies.subscriber().expect("a reader slot");
    let mut config_rev = bus.config_rev.receiver().expect("a reader slot");
    let mut status_rx = bus.status.receiver().expect("a reader slot");
    loop {
        // run every task until all of them wait on time
        loop {
            // SAFETY: one executor, polled only from this thread
            unsafe { exec.poll() };
            let now = phys as f64 / hz as f64;
            while let Some(m) = events.try_next_message() {
                if let WaitResult::Message(e) = m {
                    new_events.push((now, e));
                }
            }
            while let Some(m) = replies.try_next_message() {
                if let WaitResult::Message(r) = m {
                    report.replies.push((now, r.id, r.result));
                }
            }
            if config_rev.try_changed().is_some()
                && let Some(c) = bus.config()
            {
                report.configs.push((now, c));
            }
            if let Some(status) = status_rx.try_changed() {
                let w = world.borrow();
                let s = Sample {
                    t_s: phys as f64 / hz as f64,
                    status,
                    pit_f: w.plant.pit_probe_f(),
                    cores_f: core::array::from_fn(|i| w.plant.meats[i].map(|m| m.core_f())),
                    flap_deg: w.flap_deg,
                    led_edges: w.led_edges,
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
                a.apply(&mut world.borrow_mut(), bus, link);
            }
        }
        if let Some(speed) = opts.speed {
            let want = Duration::from_secs_f64(phys as f64 / hz as f64 / speed);
            let mut woken = false;
            while let Some(ahead) = want.checked_sub(started.elapsed()) {
                std::thread::park_timeout(ahead);
                // a socket bridge woke a task: let it run now, and finish the wait afterwards
                if clock::pending() {
                    woken = true;
                    break;
                }
            }
            if woken {
                continue;
            }
        }
        clock::set(next);
        if next >= end {
            report.mqtt = link.take_log();
            report.http = link.take_http();
            report.wdt = world.borrow().wdt;
            return report;
        }
    }
}
