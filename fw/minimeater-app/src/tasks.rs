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
//! The control task also arms the hardware watchdog (30 s, [`WATCHDOG_MS`]) and feeds it each period and after each
//! request, while the actuator task is still running: a hang resets the board into the saved cook.
//!
//! The servo calibration and fan curve are defaults until the parts are chosen (`.2.9`, `.2.2`).

use core::cell::{Cell, RefCell};

use embassy_futures::select::{Either, select};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex as AsyncMutex;
use embassy_sync::pubsub::PubSubChannel;
use embassy_sync::signal::Signal;
use embassy_sync::watch::Watch;
use embassy_time::{Duration, Instant, Ticker, Timer};
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::pwm::SetDutyCycle;
use log::warn;
use minimeater_core::control::autotune::{Due, Fit, Identifier};
use minimeater_core::food::Food;
use minimeater_core::model::Ambient;
use minimeater_core::model::actuator::{FanCycler, flap_angle_deg};
use minimeater_core::model::airflow::{Actuation, AirPath, AirflowMap};
use minimeater_core::units::{c_to_k, k_to_f};

use crate::alarms::{Alarms, Limit};
use crate::ambient::{PLAUSIBLE_C, compensate};
use crate::config::{ConfigStore, Cook, Mode, ProbeKind, Settings};
use crate::drive::{FanCurve, FanDrive, LoadWatch, ServoCal, ServoDrive, SupplyWatch, TachWatch};
use crate::food::{Foods, StallResponder};
use crate::hal::{AmbientSensor, Board, PROBE_JACKS, Servo, Tach, UsbPower, Watchdog};
use crate::pit::{Command, Event, FaultCode, Output, PERIOD_S, Phase, Pit, Reject};
use crate::probes::{self, ProbeSt, Reading, Scanner, Sense, Snapshot};
use crate::proto::BootReason;
use crate::proto::request::{Error, ErrorCode, ProbeChange, Request};

pub(crate) type M = CriticalSectionRawMutex;

/// How many tasks can read the status, events and replies: the MQTT client, OTLP, and one more
/// (the twin's harness).
pub const READERS: usize = 3;

/// Board temperature (the ADC's die, or the ambient sensor on the underside) that raises `board_hot`, and the temperature it must fall to to clear, C.
const BOARD_HOT_C: (f64, f64) = (80.0, 75.0);
/// The scanner starts a pass at least this often, ms (ADR 0002: four jacks at 0.4 s each). A pass
/// on the real ADC takes longer, so it runs back to back there.
const PASS_MS: u64 = 1600;
/// The hardware watchdog's timeout, ms: three control periods. The control task feeds it once a period and after each
/// request, and only while the actuator task is still running; a flash write or one solve of a fit is well inside.
const WATCHDOG_MS: u32 = 30_000;

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
/// The slow task's work: the window to fit and the map to rebuild sit on the [`Bench`], so a job and its
/// result are tags, not 3-7 KB values copied through both tasks' stacks.
enum Job {
    Fit(Due),
    Map(AirPath, Ambient),
}

enum Done {
    Fit(Due, Option<Fit>),
    Map,
}

/// What the slow task works on. The control task fills or swaps it only while no job is out, so its lock
/// is always free when control takes it.
struct Bench {
    /// A copy of the autotune window being fitted.
    id: Identifier,
    /// The next airflow map, swapped with the pit's when built.
    map: AirflowMap,
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
    /// Why the device started, until the MQTT client has announced it (`boot` event). Set by the binary at start-up.
    pub boot: Mutex<M, Cell<Option<BootReason>>>,
    /// Hardware faults raised by any task, and the fan's last tach reading.
    faults: Mutex<M, Cell<Faults>>,
    fan_rpm: Mutex<M, Cell<Option<f64>>>,
    /// Counts the actuator task's passes, so the control task can tell it from a hung one.
    beat: Mutex<M, Cell<u32>>,
    act: Signal<M, Actuation>,
    job: Signal<M, Job>,
    done: Signal<M, Done>,
    bench: AsyncMutex<M, Bench>,
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
            boot: Mutex::new(Cell::new(None)),
            probes: Mutex::new(Cell::new(Snapshot {
                passes: 0,
                sense: [Sense::Pending; PROBE_JACKS],
                die_c: None,
                offset: None,
            })),
            faults: Mutex::new(Cell::new(Faults(0))),
            fan_rpm: Mutex::new(Cell::new(None)),
            beat: Mutex::new(Cell::new(0)),
            act: Signal::new(),
            job: Signal::new(),
            done: Signal::new(),
            bench: AsyncMutex::new(Bench {
                id: Identifier::DEFAULT,
                map: AirflowMap::EMPTY,
            }),
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

    pub(crate) fn emit(&self, e: Event) {
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
                ProbeChange::Stall(s) => p.stall = s,
                ProbeChange::Bump(b) => p.bump_f = b,
            }
            Ok(true)
        }
        Request::Wifi(w) => {
            let changed = settings.wifi != w;
            settings.wifi = w;
            Ok(changed)
        }
        Request::Mqtt(m) => {
            let changed = settings.mqtt != m;
            settings.mqtt = m;
            Ok(changed)
        }
        Request::Otlp(o) => {
            let changed = settings.otlp != o;
            settings.otlp = o;
            Ok(changed)
        }
        Request::Identify => {
            bus.identify.signal(());
            Ok(false)
        }
        Request::Reboot => {
            bus.reboot.signal(());
            Ok(false)
        }
        // `handle` does this one: it needs the store
        Request::Defaults => Ok(false),
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
    mut wdt: B::Watchdog,
    mut ambient: B::Ambient,
    mut store: ConfigStore<B::Config>,
    mut settings: Settings,
    pit: &mut Pit,
    foods: &mut Foods,
    bus: &Bus,
) -> ! {
    // armed here, after the boot's flash reads and the self-survey; a hang from now on resets the board into the saved cook
    wdt.start(WATCHDOG_MS);
    let mut beat = 0;
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
    let mut responder = StallResponder::default();
    let mut last_pass = 0;
    // `~/config` goes out once at boot and again after every accepted change
    let mut republish = true;
    loop {
        // fed while the actuators (the fan and the flap) are still being driven; if they hang the board resets and
        // boots with both off
        let b = bus.beat.lock(Cell::get);
        if b != beat {
            beat = b;
            wdt.feed();
        } else {
            warn!("actuator task silent: watchdog not fed");
        }
        let raw_c = ambient.read_c().await.ok();
        // an implausible reading is no reading for the airflow plan; `board_hot` below still sees it
        let usable = raw_c.filter(|c| PLAUSIBLE_C.contains(c));
        bus.fault(FaultCode::Ambient, usable.is_none());
        // a scanner that has stopped is an ADC that is not answering
        let snap = bus.snapshot();
        // the sensor is on the board and reads some of its heat
        let ambient_c = usable.map(|c| compensate(c, snap.die_c));
        let ambient_f = ambient_c.map(|c| k_to_f(c_to_k(c)));
        let sense = if snap.passes == last_pass {
            [Sense::Fault; PROBE_JACKS]
        } else {
            snap.sense
        };
        last_pass = snap.passes;
        bus.fault(FaultCode::Adc, sense.iter().all(|s| matches!(s, Sense::Fault)));
        // the board's temperature is the hotter of the ADC's die and the ambient sensor on the underside
        if let Some(c) = snap.die_c.into_iter().chain(raw_c).reduce(f64::max) {
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
        if let Some(e) = responder.step(foods, &temps, jack, &settings.probes, pit, PERIOD_S) {
            bus.emit(e);
        }

        // slow work, one job at a time
        if let Some(d) = bus.done.try_take() {
            match d {
                Done::Fit(due, fit) => {
                    pit.fitted(due, fit);
                }
                Done::Map => pit.swap_map(&mut bus.bench.lock().await.map),
            }
            busy = false;
        }
        if !busy {
            if let Some((due, id)) = pit.take_fit() {
                bus.bench.lock().await.id.copy_from(id);
                bus.job.signal(Job::Fit(due));
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
                    // the flash write in a request can be slow; the period's feed is not enough after it
                    wdt.feed();
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
        Request::Defaults => {
            *settings = Settings::default();
            pit.reset(settings);
            // saved here, not left to the next period: the reboot is half a second away
            let saved = store.save(settings).await;
            bus.publish_config(settings);
            // without the save a reboot would bring the old settings back: stay up on the defaults and say so
            if saved.is_ok() {
                bus.reboot.signal(());
            }
            saved
                .map(|()| false)
                .map_err(|_| Error::new(ErrorCode::NotAllowed, "could not save"))
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
#[allow(clippy::too_many_arguments)]
pub async fn actuators<B: Board>(
    mut fan: B::Fan,
    mut tach: B::Tach,
    mut servo: B::Servo,
    mut power: B::ServoPower,
    mut fan_power: B::FanPower,
    mut load_fault: B::LoadFault,
    mut usb: B::Usb,
    s_min: f64,
    rpm_max: f64,
    bus: &Bus,
) -> ! {
    const DT: f64 = 1.0;
    let mut supply = SupplyWatch::default();
    let mut load = LoadWatch::default();
    let mut tick = Ticker::every(Duration::from_secs(1));
    let mut cycler = FanCycler::default();
    let mut fan_drive = FanDrive::new(FanCurve::LINEAR, None);
    let mut watch = TachWatch::default();
    let mut flap = ServoDrive::new(ServoCal::DEFAULT);
    let mut act = Actuation { damper: 0.0, fan: 0.0 };
    loop {
        bus.beat.lock(|c| c.set(c.get().wrapping_add(1)));
        if let Some(a) = bus.act.try_take() {
            act = a;
        }
        // the fan and the servo run on a 1.5 A source or not at all
        if let Some(raised) = supply.tick(usb.source_current_ma()) {
            bus.fault(FaultCode::Supply, raised);
        }
        let speed = cycler.tick(act.fan, s_min, DT);
        if load.tick(load_fault.is_low().unwrap_or(false)) {
            bus.fault(FaultCode::Load, true);
        }
        // both loads run only on a good source and until a load switch has tripped
        let live = supply.ok() && !load.tripped();
        let duty = if live { fan_drive.duty(speed) } else { 0.0 };
        // the duty first, then the power; power off first when the duty is zero: 0 % really stops the fan
        let _ = if duty > 0.0 {
            fan_power.set_high()
        } else {
            fan_power.set_low()
        };
        let max = f64::from(fan.max_duty_cycle());
        let _ = fan.set_duty_cycle((duty * max) as u16);

        // pulse first, then power: a servo waking up goes straight to the new position
        let s = flap.tick(flap_angle_deg(act.damper), DT);
        servo.set_pulse_us(s.pulse_us);
        let _ = if s.power && live {
            power.set_high()
        } else {
            power.set_low()
        };

        tick.next().await;
        // the tach's mean over the second just run
        let rpm = tach.rpm();
        bus.fan_rpm.lock(|c| c.set(rpm));
        if let Some(stalled) = watch.tick(duty, rpm, rpm_max, DT) {
            bus.fault(FaultCode::FanStall, stalled);
        }
    }
}

/// Runs the slow jobs the control task hands over. They are stepwise (`fit_async`, `rebuild` yield after every
/// solve), so on the one cooperative executor they delay the other tasks by one solve at most.
pub async fn slow(bus: &Bus) -> ! {
    loop {
        // the bench's lock is let go before the result is signalled
        let done = match bus.job.wait().await {
            Job::Fit(due) => Done::Fit(due, bus.bench.lock().await.id.fit_async().await),
            Job::Map(path, amb) => {
                bus.bench.lock().await.map.rebuild(&path, amb).await;
                Done::Map
            }
        };
        bus.done.signal(done);
    }
}
