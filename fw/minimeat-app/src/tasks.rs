//! The application's tasks, generic over the [`Board`]: the ESP32-C6 binary and the host twin
//! (`.4.16.2`) wrap each in a thin `#[embassy_executor::task]` and run the same code.
//!
//! - [`control`]: every [`PERIOD_S`], probes in, [`Pit::step`] and [`Foods::step`], the actuation
//!   out to [`actuators`], status and events onto the [`Bus`], settings to flash.
//! - [`actuators`]: every second, the fan (duty-cycled below its minimum speed, [`FanCycler`]; duty
//!   curve and stall watch from the tach) and the damper servo (calibration, slew limit, powered
//!   only while it moves), all in [`crate::drive`].
//! - [`slow`]: what is too slow for the control period, an autotune fit or an airflow-map rebuild,
//!   handed over through the [`Bus`]. On the C6 it belongs on a lower-priority executor (`.4.24`).
//!
//! - [`probes`]: the ADC round robin, averaging and filtering ([`crate::probes`]), publishing the
//!   latest [`Snapshot`] on the [`Bus`] for [`control`] to read each period.
//!
//! The servo calibration and fan curve are defaults until the parts are chosen (`.2.9`, `.2.2`).

use core::cell::{Cell, RefCell};

use embassy_futures::select::{Either, select};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::pubsub::PubSubChannel;
use embassy_sync::signal::Signal;
use embassy_sync::watch::Watch;
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

use crate::alarms::{Alarms, Limit};
use crate::config::{ConfigStore, Cook, Mode, ProbeKind, Settings};
use crate::drive::{FanCurve, FanDrive, ServoCal, ServoDrive, TachWatch};
use crate::food::Foods;
use crate::hal::{AmbientSensor, Board, PROBE_JACKS, Servo, Tach};
use crate::pit::{Command, Event, FaultCode, Output, PERIOD_S, Phase, Pit, Reject};
use crate::probes::{self, ProbeSt, Reading, Scanner, Sense, Snapshot};
use crate::proto::request::{Error, ErrorCode, ProbeChange, Request};

pub(crate) type M = CriticalSectionRawMutex;

/// How many tasks can read the status, events and replies: the MQTT client, OTLP, and one more
/// (the twin's harness).
pub const READERS: usize = 3;

/// ADC die temperature that raises `board_hot`, and the temperature it must fall to to clear, C.
const BOARD_HOT_C: (f64, f64) = (80.0, 75.0);
/// The scanner starts a pass at least this often, ms (ADR 0002: four jacks at 0.4 s each). A pass
/// on the real ADC takes longer, so it runs back to back there.
const PASS_MS: u64 = 1600;

/// A request and the id its reply will carry.
#[derive(Clone, Debug)]
pub struct Inbound {
    pub id: u16,
    pub req: Request,
}

#[derive(Clone, Debug)]
pub struct Reply {
    pub id: u16,
    pub result: Result<(), Error>,
}

/// `autotune` in the state (docs/protocol.md).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AutotuneInfo {
    /// The setpoint dither is running.
    pub exciting: bool,
    /// Fits taken into the model since boot.
    pub accepted: u32,
    /// How full the data window is, 0..1.
    pub window: f64,
}

/// A set of [`FaultCode`]s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Faults(u8);

impl Faults {
    fn bit(code: FaultCode) -> u8 {
        1 << FaultCode::ALL.iter().position(|c| *c == code).unwrap_or(0)
    }

    pub fn contains(self, code: FaultCode) -> bool {
        self.0 & Self::bit(code) != 0
    }

    /// Sets or clears `code`; true if that changed it.
    pub fn set(&mut self, code: FaultCode, on: bool) -> bool {
        let was = self.contains(code);
        if on {
            self.0 |= Self::bit(code);
        } else {
            self.0 &= !Self::bit(code);
        }
        was != on
    }

    pub fn iter(self) -> impl Iterator<Item = FaultCode> {
        FaultCode::ALL.into_iter().filter(move |c| self.contains(*c))
    }
}

/// The latest status, for `~/state` (`.5.2`) and the twin's display.
#[derive(Clone, Copy, Debug)]
pub struct Status {
    pub up_s: u64,
    pub cook: Cook,
    pub out: Output,
    /// Each jack, F with offset: `None` unless the probe is `ok` or `disagree`.
    pub temps: [Option<f64>; PROBE_JACKS],
    pub probe_st: [ProbeSt; PROBE_JACKS],
    /// Each probe's alarm (`probes[n].alarm`), latched.
    pub alarms: [Option<Limit>; PROBE_JACKS],
    /// An alarm is on that `alarm_ack` has not silenced: the LED shows it.
    pub alarm_led: bool,
    /// ADC die temperature, C.
    pub adc_die_c: Option<f64>,
    /// Fan tachometer, rpm.
    pub fan_rpm: Option<f64>,
    /// Active faults (`faults`, docs/protocol.md): a bit per [`FaultCode`].
    pub faults: Faults,
    pub autotune: AutotuneInfo,
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
    /// In: `~/set` and `~/cmd` once parsed ([`crate::proto::parse`]), or a twin scenario. Each is
    /// answered on `replies` with its id.
    pub commands: Channel<M, Inbound, 8>,
    /// The outcome of each request taken from `commands`. Every subscriber sees every reply and
    /// picks out the ids it sent; a subscriber that falls behind loses the oldest.
    pub replies: PubSubChannel<M, Reply, 8, READERS, 1>,
    /// Out: `~/event`. Control never waits on a subscriber: one that falls behind loses the oldest.
    pub events: PubSubChannel<M, Event, 16, READERS, 1>,
    /// The latest status, every control period: `~/state`, OTLP and the twin each take their own
    /// receiver (`bus.status.receiver()`).
    pub status: Watch<M, Status, READERS>,
    /// The scanner's latest pass, read (not consumed) by the control task.
    probes: Mutex<M, Cell<Snapshot>>,
    /// The settings as they are now, for `~/config`; `config_changed` fires when they move.
    config: Mutex<M, RefCell<Option<Settings>>>,
    /// A counter that moves whenever `~/config` should be published again.
    pub config_rev: Watch<M, u32, READERS>,
    /// `identify` and `reboot` requests, for the LED and the board.
    pub identify: Signal<M, ()>,
    pub reboot: Signal<M, ()>,
    /// Hardware faults raised by any task, and the fan's last tach reading.
    faults: Mutex<M, Cell<Faults>>,
    fan_rpm: Mutex<M, Cell<Option<f64>>>,
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
            replies: PubSubChannel::new(),
            events: PubSubChannel::new(),
            status: Watch::new(),
            config: Mutex::new(RefCell::new(None)),
            config_rev: Watch::new(),
            identify: Signal::new(),
            reboot: Signal::new(),
            probes: Mutex::new(Cell::new(Snapshot {
                passes: 0,
                sense: [Sense::Pending; PROBE_JACKS],
                die_c: None,
                offset: None,
            })),
            faults: Mutex::new(Cell::new(Faults(0))),
            fan_rpm: Mutex::new(Cell::new(None)),
            act: Signal::new(),
            job: Signal::new(),
            done: Signal::new(),
        }
    }

    /// Queues a request for the control task; false if the queue is full.
    pub fn send(&self, id: u16, req: Request) -> bool {
        self.commands.try_send(Inbound { id, req }).is_ok()
    }

    /// The current settings (`None` before the control task has started).
    pub fn config(&self) -> Option<Settings> {
        self.config.lock(|c| c.borrow().clone())
    }

    /// Publishes the settings for `~/config` and moves the revision readers wait on.
    pub fn publish_config(&self, settings: &Settings) {
        self.config.lock(|c| *c.borrow_mut() = Some(settings.clone()));
        let rev = self.config_rev.try_get().unwrap_or(0).wrapping_add(1);
        self.config_rev.sender().send(rev);
    }

    /// Raises or clears a fault; the `fault` and `fault_clear` events fire on the change only.
    fn fault(&self, code: FaultCode, on: bool) {
        let changed = self.faults.lock(|c| {
            let mut f = c.get();
            let changed = f.set(code, on);
            c.set(f);
            changed
        });
        if changed {
            self.emit(if on {
                Event::Fault(code)
            } else {
                Event::FaultClear(code)
            });
        }
    }

    fn snapshot(&self) -> Snapshot {
        self.probes.lock(Cell::get)
    }

    fn emit(&self, e: Event) {
        self.events.immediate_publisher().publish_immediate(e);
    }
}

/// Applies one request; true if it changed what `~/config` reports.
fn apply(req: Request, pit: &mut Pit, settings: &mut Settings, bus: &Bus) -> Result<bool, Error> {
    match req {
        Request::Pit(c) => {
            pit.command(c).map_err(|r| match r {
                Reject::OutOfRange => Error::new(ErrorCode::OutOfRange, "out of range"),
                Reject::NotAllowed => Error::new(ErrorCode::NotAllowed, "not allowed in this mode"),
            })?;
            Ok(!matches!(c, Command::Autotune | Command::AutotuneStop))
        }
        Request::Probe { n, change } => {
            let p = settings
                .probes
                .get_mut(n)
                .ok_or_else(|| Error::new(ErrorCode::Unknown, "no such probe"))?;
            match change {
                ProbeChange::Name(name) => p.name = name,
                ProbeChange::Kind(kind) => p.kind = kind,
                ProbeChange::KeepCustom => {
                    if !matches!(p.kind, ProbeKind::Custom(_)) {
                        return Err(Error::new(ErrorCode::NotAllowed, "send probe/<n>/coeffs first"));
                    }
                }
                ProbeChange::Offset(o) => p.offset_f = o,
                ProbeChange::Lo(l) => p.lo_f = l,
                ProbeChange::Hi(h) => p.hi_f = h,
                ProbeChange::Cut(c) => p.cut = c,
            }
            Ok(true)
        }
        Request::Identify => {
            bus.identify.signal(());
            Ok(false)
        }
        Request::Reboot => {
            bus.reboot.signal(());
            Ok(false)
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
    let mut alarms = Alarms::default();
    let mut last_pass = 0;
    // `~/config` goes out once at boot and again after every accepted change
    let mut republish = true;
    loop {
        let ambient_c = ambient.read_c().await.ok();
        bus.fault(FaultCode::Ambient, ambient_c.is_none());
        let ambient_f = ambient_c.map(|c| k_to_f(c_to_k(c)));
        // a scanner that has stopped is an ADC that is not answering
        let snap = bus.snapshot();
        let sense = if snap.passes == last_pass {
            [Sense::Fault; PROBE_JACKS]
        } else {
            snap.sense
        };
        last_pass = snap.passes;
        bus.fault(FaultCode::Adc, sense.iter().all(|s| matches!(s, Sense::Fault)));
        if let Some(c) = snap.die_c {
            let hot = bus.faults.lock(Cell::get).contains(FaultCode::BoardHot);
            bus.fault(FaultCode::BoardHot, c >= BOARD_HOT_C.0 || (hot && c > BOARD_HOT_C.1));
        }
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
        for e in alarms.step(&temps, &settings.probes) {
            bus.emit(e);
        }
        let amb = Ambient {
            t_k: c_to_k(ambient_c.unwrap_or(20.0)),
            ..Ambient::default()
        };
        let jack = settings.cook.pit_probe as usize;
        let (out, events) = pit.step(temps[jack], amb, PERIOD_S);
        bus.act.signal(out.act);
        for e in events {
            // a learned model is part of `~/config`
            republish |= matches!(e, Event::AutotuneDone { .. });
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
        if republish {
            republish = false;
            bus.publish_config(&settings);
        }
        let _ = store.save(&settings).await;

        bus.status.sender().send(Status {
            up_s: Instant::now().as_secs(),
            cook: *pit.cook(),
            out,
            temps,
            probe_st: readings.map(|r| r.st),
            alarms: alarms.active(),
            alarm_led: alarms.unacked(),
            adc_die_c: snap.die_c,
            fan_rpm: bus.fan_rpm.lock(Cell::get),
            autotune: AutotuneInfo {
                exciting: pit.autotune().status().exciting,
                accepted: pit.autotune().status().accepted,
                window: pit.autotune().window_fill(),
            },
            faults: {
                let mut f = bus.faults.lock(Cell::get);
                f.set(FaultCode::PitProbe, out.phase == Phase::Fault);
                f
            },
            foods: core::array::from_fn(|j| foods.food(j, &temps, jack)),
            ambient_c,
        });
        // until the next period, requests are answered as they arrive
        loop {
            match select(tick.next(), bus.commands.receive()).await {
                Either::First(()) => break,
                Either::Second(r) => {
                    handle::<B>(r, pit, &mut settings, &mut store, &mut alarms, &mut check, bus).await;
                }
            }
        }
    }
}

/// One request: applied now, answered, and if it changed a setting then saved and republished.
async fn handle<B: Board>(
    Inbound { id, req }: Inbound,
    pit: &mut Pit,
    settings: &mut Settings,
    store: &mut ConfigStore<B::Config>,
    alarms: &mut Alarms,
    check: &mut bool,
    bus: &Bus,
) {
    let result = match req {
        Request::Pit(Command::SelfCheck) => {
            *check = true;
            Ok(false)
        }
        Request::Pit(Command::AlarmAck) => {
            alarms.ack();
            Ok(false)
        }
        req => apply(req, pit, settings, bus),
    };
    let result = match result {
        Ok(changed) => {
            if changed {
                // a setpoint set before a power cut must survive it: do not wait for the next period
                pit.persist(settings);
                let _ = store.save(settings).await;
                bus.publish_config(settings);
            }
            Ok(())
        }
        Err(e) => {
            warn!("request {id} rejected: {:?} {}", e.code, e.detail);
            Err(e)
        }
    };
    bus.replies
        .immediate_publisher()
        .publish_immediate(Reply { id, result });
}

/// The actuators, every second: `s_min` is the fan's minimum continuous speed fraction and `rpm_max`
/// its speed at full duty.
pub async fn actuators<B: Board>(
    mut fan: B::Fan,
    mut tach: B::Tach,
    mut servo: B::Servo,
    mut power: B::ServoPower,
    s_min: f64,
    rpm_max: f64,
    bus: &Bus,
) -> ! {
    const DT: f64 = 1.0;
    let mut tick = Ticker::every(Duration::from_secs(1));
    let mut cycler = FanCycler::default();
    let mut fan_drive = FanDrive::new(FanCurve::LINEAR, None);
    let mut watch = TachWatch::default();
    let mut flap = ServoDrive::new(ServoCal::DEFAULT);
    let mut act = Actuation { damper: 0.0, fan: 0.0 };
    loop {
        if let Some(a) = bus.act.try_take() {
            act = a;
        }
        let speed = cycler.tick(act.fan, s_min, DT);
        let duty = fan_drive.duty(speed);
        let max = f64::from(fan.max_duty_cycle());
        let _ = fan.set_duty_cycle((duty * max) as u16);

        // pulse first, then power: a servo waking up goes straight to the new position
        let s = flap.tick(flap_angle_deg(act.damper), DT);
        servo.set_pulse_us(s.pulse_us);
        let _ = if s.power { power.set_high() } else { power.set_low() };

        tick.next().await;
        // the tach's mean over the second just run
        let rpm = tach.rpm();
        bus.fan_rpm.lock(|c| c.set(rpm));
        if let Some(stalled) = watch.tick(duty, rpm, rpm_max, DT) {
            bus.fault(FaultCode::FanStall, stalled);
        }
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
