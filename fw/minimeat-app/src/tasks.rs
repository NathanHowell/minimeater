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
//! - [`probes`]: the ADC round robin, averaging and filtering ([`crate::probes`]), publishing the
//!   latest [`Snapshot`] on the [`Bus`] for [`control`] to read each period.
//!
//! The actuator output here is the minimum that works end to end: a linear servo calibration and fan
//! duty = speed. The actuator drivers (`.4.8`: servo calibration and slew, fan curve, tach check)
//! replace it behind the same task boundary.

use core::cell::Cell;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Ticker, Timer};
use embedded_hal::digital::OutputPin;
use embedded_hal::pwm::SetDutyCycle;
use log::warn;
use minimeat_core::control::autotune::{Due, Fit, Identifier};
use minimeat_core::food::Food;
use minimeat_core::model::Ambient;
use minimeat_core::model::actuator::{FanCycler, flap_angle_deg};
use minimeat_core::model::airflow::{Actuation, AirPath, AirflowMap};
use minimeat_core::units::{c_to_k, k_to_f};

use crate::config::{ConfigStore, Cook, Mode, Settings};
use crate::food::Foods;
use crate::hal::{AmbientSensor, Board, PROBE_JACKS, Servo};
use crate::pit::{Command, Event, Output, PERIOD_S, Pit};
use crate::probes::{self, ProbeSt, Reading, Scanner, Sense, Snapshot};

type M = CriticalSectionRawMutex;

/// Servo pulse widths at the flap's closed and fully open stops, us. [est until `.4.8` calibrates]
pub const SERVO_CLOSED_US: f64 = 1000.0;
pub const SERVO_OPEN_US: f64 = 2000.0;
/// The servo stays powered this long after its last move, s.
const SERVO_HOLD_S: u32 = 2;
/// The scanner starts a pass at least this often, ms (ADR 0002: four jacks at 0.4 s each). A pass
/// on the real ADC takes longer, so it runs back to back there.
const PASS_MS: u64 = 1600;

/// The latest status, for `~/state` (`.5.2`) and the twin's display.
#[derive(Clone, Copy, Debug)]
pub struct Status {
    pub up_s: u64,
    pub cook: Cook,
    pub out: Output,
    /// Each jack, F with offset: `None` unless the probe is `ok` or `disagree`.
    pub temps: [Option<f64>; PROBE_JACKS],
    pub probe_st: [ProbeSt; PROBE_JACKS],
    /// ADC die temperature, C.
    pub adc_die_c: Option<f64>,
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
    /// The scanner's latest pass, read (not consumed) by the control task.
    probes: Mutex<M, Cell<Snapshot>>,
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
            probes: Mutex::new(Cell::new(Snapshot {
                passes: 0,
                sense: [Sense::Pending; PROBE_JACKS],
                die_c: None,
                offset: None,
            })),
            act: Signal::new(),
            job: Signal::new(),
            done: Signal::new(),
        }
    }

    fn snapshot(&self) -> Snapshot {
        self.probes.lock(Cell::get)
    }

    fn emit(&self, e: Event) {
        if self.events.try_send(e).is_err() {
            warn!("events: queue full, dropped {e:?}");
        }
    }
}

/// The probe scanner: passes back to back, at least [`PASS_MS`] apart.
pub async fn probes<B: Board>(adc: B::Probes, bus: &Bus) -> ! {
    let mut scanner = Scanner::new(adc);
    let mut tick = Ticker::every(Duration::from_millis(PASS_MS));
    loop {
        let snap = scanner.pass().await;
        bus.probes.lock(|c| c.set(snap));
        tick.next().await;
    }
}

/// The control loop. `pit` and `foods` are large: the caller gives them static storage.
pub async fn control<B: Board>(
    mut ambient: B::Ambient,
    mut store: ConfigStore<B::Config>,
    mut settings: Settings,
    pit: &mut Pit,
    foods: &mut Foods,
    bus: &Bus,
) -> ! {
    // the first full scan before the first period
    while bus.snapshot().passes == 0 {
        Timer::after_millis(100).await;
    }
    let mut tick = Ticker::every(Duration::from_millis((PERIOD_S * 1000.0) as u64));
    let mut busy = false;
    // a cook in progress (power came back mid-cook) is not the time to question the probes
    let mut check = settings.cook.mode == Mode::Off;
    let mut disagree = [false; PROBE_JACKS];
    let mut last_pass = 0;
    loop {
        while let Ok(c) = bus.commands.try_receive() {
            if c == Command::SelfCheck {
                check = true;
            } else if let Err(r) = pit.command(c) {
                warn!("command {c:?} rejected: {r:?}");
            }
        }
        let ambient_c = ambient.read_c().await.ok();
        let ambient_f = ambient_c.map(|c| k_to_f(c_to_k(c)));
        // a scanner that has stopped is an ADC that is not answering
        let snap = bus.snapshot();
        let sense = if snap.passes == last_pass {
            [Sense::Fault; PROBE_JACKS]
        } else {
            snap.sense
        };
        last_pass = snap.passes;
        let read = |disagree: &[bool; PROBE_JACKS]| -> [Reading; PROBE_JACKS] {
            core::array::from_fn(|j| probes::reading(sense[j], &settings.probes[j], disagree[j]))
        };
        let mut readings = read(&disagree);
        if check {
            check = false;
            let results = probes::selfcheck(&readings, ambient_f);
            disagree = probes::disagreeing(&results);
            readings = read(&disagree);
            bus.emit(Event::SelfCheck { results });
        }
        for (d, s) in disagree.iter_mut().zip(sense) {
            // unplugged: the verdict goes with the probe
            *d &= matches!(s, Sense::Probe(_));
        }
        let temps = readings.map(|r| r.temp_f);
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
            probe_st: readings.map(|r| r.st),
            adc_die_c: snap.die_c,
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
