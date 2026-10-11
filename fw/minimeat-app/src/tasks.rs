//! The application's tasks, generic over the [`Board`]: the ESP32-C6 binary and the host twin
//! (`.4.16.2`) wrap each in a thin `#[embassy_executor::task]` and run the same code.
//!
//! - [`control`]: every [`PERIOD_S`], probes in, [`Pit::step`] and [`Foods::step`], the actuation
//!   out to [`actuators`], status and events onto the [`Bus`], settings to flash.
//! - [`actuators`]: every second, the fan (duty-cycled below its minimum speed, [`FanCycler`]) and
//!   the damper servo (powered only while it moves).
//! - [`slow`]: what is too slow for the control period, an autotune fit or an airflow-map rebuild,
//!   handed over through the [`Bus`]. On the C6 it belongs on a lower-priority executor (`.4.24`).
//!
//! The probe reading and actuator output here are the minimum that works end to end: one
//! conversion per jack per period, a linear servo calibration and fan duty = speed. The probe
//! pipeline (`.4.7`: filtering, round robin, self-check) and the actuator drivers (`.4.8`: servo
//! calibration and slew, fan curve, tach check) replace them behind the same task boundaries.

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Ticker};
use embedded_hal::digital::OutputPin;
use embedded_hal::pwm::SetDutyCycle;
use log::warn;
use minimeat_core::control::autotune::{Due, Fit, Identifier};
use minimeat_core::food::Food;
use minimeat_core::model::Ambient;
use minimeat_core::model::actuator::{FanCycler, flap_angle_deg};
use minimeat_core::model::airflow::{Actuation, AirPath, AirflowMap};
use minimeat_core::thermistor::Jack;
use minimeat_core::units::{c_to_k, k_to_f};

use crate::config::{ConfigStore, Cook, Settings};
use crate::food::Foods;
use crate::hal::{AmbientSensor, Board, PROBE_JACKS, ProbeAdc, Servo};
use crate::pit::{Command, Event, Output, PERIOD_S, Pit};

type M = CriticalSectionRawMutex;

/// Servo pulse widths at the flap's closed and fully open stops, us. [est until `.4.8` calibrates]
pub const SERVO_CLOSED_US: f64 = 1000.0;
pub const SERVO_OPEN_US: f64 = 2000.0;
/// The servo stays powered this long after its last move, s.
const SERVO_HOLD_S: u32 = 2;
/// Probe readings outside this range are not `ok` (docs/protocol.md), F.
const PROBE_RANGE_F: (f64, f64) = (-20.0, 700.0);

/// The latest status, for `~/state` (`.5.2`) and the twin's display.
#[derive(Clone, Copy, Debug)]
pub struct Status {
    pub up_s: u64,
    pub cook: Cook,
    pub out: Output,
    /// Each jack, F with offset, `None` unless `ok`.
    pub temps: [Option<f64>; PROBE_JACKS],
    pub foods: [Option<Food>; PROBE_JACKS],
    pub ambient_c: Option<f64>,
}

#[allow(clippy::large_enum_variant, reason = "one static slot on the bus, no alloc")]
enum Job {
    Fit(Due, Identifier),
    Map(AirPath, Ambient),
}

#[allow(clippy::large_enum_variant, reason = "one static slot on the bus, no alloc")]
enum Done {
    Fit(Due, Option<Fit>),
    Map(AirflowMap),
}

/// Everything the tasks exchange. One static instance per board.
pub struct Bus {
    /// In: `~/set`, `~/cmd` (`.5.2`), or a twin scenario.
    pub commands: Channel<M, Command, 8>,
    /// Out: `~/event`. A full channel drops the event (control never waits on the network).
    pub events: Channel<M, Event, 16>,
    pub status: Signal<M, Status>,
    act: Signal<M, Actuation>,
    job: Signal<M, Job>,
    done: Signal<M, Done>,
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

impl Bus {
    pub const fn new() -> Self {
        Self {
            commands: Channel::new(),
            events: Channel::new(),
            status: Signal::new(),
            act: Signal::new(),
            job: Signal::new(),
            done: Signal::new(),
        }
    }

    fn emit(&self, e: Event) {
        if self.events.try_send(e).is_err() {
            warn!("events: queue full, dropped {e:?}");
        }
    }
}

/// One conversion per jack: F with the probe's offset, `None` unless the probe reads `ok`.
async fn read_jacks<P: ProbeAdc>(probes: &mut P, settings: &Settings) -> [Option<f64>; PROBE_JACKS] {
    let mut temps = [None; PROBE_JACKS];
    for (j, t) in temps.iter_mut().enumerate() {
        let p = &settings.probes[j];
        *t = match probes.ratio(j).await.map(Jack::classify) {
            Ok(Jack::Probe(r)) => Some(k_to_f(p.kind.thermistor().t_k(r)) + p.offset_f),
            _ => None,
        }
        .filter(|f| (PROBE_RANGE_F.0..=PROBE_RANGE_F.1).contains(f));
    }
    temps
}

/// The control loop. `pit` and `foods` are large: the caller gives them static storage.
pub async fn control<B: Board>(
    mut probes: B::Probes,
    mut ambient: B::Ambient,
    mut store: ConfigStore<B::Config>,
    mut settings: Settings,
    pit: &mut Pit,
    foods: &mut Foods,
    bus: &Bus,
) -> ! {
    let mut tick = Ticker::every(Duration::from_millis((PERIOD_S * 1000.0) as u64));
    let mut busy = false;
    loop {
        while let Ok(c) = bus.commands.try_receive() {
            if let Err(r) = pit.command(c) {
                warn!("command {c:?} rejected: {r:?}");
            }
        }
        let temps = read_jacks(&mut probes, &settings).await;
        let ambient_c = ambient.read_c().await.ok();
        let amb = Ambient {
            t_k: c_to_k(ambient_c.unwrap_or(20.0)),
            ..Ambient::default()
        };
        let jack = settings.cook.pit_probe as usize;
        let (out, events) = pit.step(temps[jack], amb, PERIOD_S);
        bus.act.signal(out.act);
        for e in events {
            bus.emit(*e);
        }
        for e in foods.step(&temps, jack, temps[jack], out.phase, &settings.probes, PERIOD_S) {
            bus.emit(*e);
        }

        // slow work, one job at a time
        if let Some(d) = bus.done.try_take() {
            match d {
                Done::Fit(due, fit) => {
                    pit.fitted(due, fit);
                }
                Done::Map(map) => pit.set_map(map),
            }
            busy = false;
        }
        if !busy {
            if let Some((due, id)) = pit.take_fit() {
                bus.job.signal(Job::Fit(due, id.clone()));
                busy = true;
            } else if let Some((path, amb)) = pit.map_wanted() {
                bus.job.signal(Job::Map(path, amb));
                busy = true;
            }
        }

        // best effort: control never depends on the flash
        pit.persist(&mut settings);
        let _ = store.save(&settings).await;

        bus.status.signal(Status {
            up_s: Instant::now().as_secs(),
            cook: *pit.cook(),
            out,
            temps,
            foods: core::array::from_fn(|j| foods.food(j, &temps, jack)),
            ambient_c,
        });
        tick.next().await;
    }
}

/// The actuators, every second: `s_min` is the fan's minimum continuous speed fraction.
pub async fn actuators<B: Board>(
    mut fan: B::Fan,
    mut servo: B::Servo,
    mut power: B::ServoPower,
    s_min: f64,
    bus: &Bus,
) -> ! {
    let mut tick = Ticker::every(Duration::from_secs(1));
    let mut cycler = FanCycler::default();
    let mut act = Actuation { damper: 0.0, fan: 0.0 };
    let (mut pulse_us, mut still_s) = (0u16, 0u32);
    loop {
        if let Some(a) = bus.act.try_take() {
            act = a;
        }
        let s = cycler.tick(act.fan, s_min, 1.0);
        let max = f64::from(fan.max_duty_cycle());
        let _ = fan.set_duty_cycle((s.clamp(0.0, 1.0) * max) as u16);

        let frac = flap_angle_deg(act.damper) / 90.0;
        let us = (SERVO_CLOSED_US + frac * (SERVO_OPEN_US - SERVO_CLOSED_US)) as u16;
        if us != pulse_us {
            let _ = power.set_high();
            servo.set_pulse_us(us);
            (pulse_us, still_s) = (us, 0);
        } else if still_s < SERVO_HOLD_S {
            still_s += 1;
            if still_s == SERVO_HOLD_S {
                let _ = power.set_low();
            }
        }
        tick.next().await;
    }
}

/// Runs the slow jobs the control task hands over.
pub async fn slow(bus: &Bus) -> ! {
    loop {
        let done = match bus.job.wait().await {
            Job::Fit(due, id) => Done::Fit(due, id.fit()),
            Job::Map(path, amb) => Done::Map(AirflowMap::build(&path, amb)),
        };
        bus.done.signal(done);
    }
}
