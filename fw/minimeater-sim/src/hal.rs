//! Simulated peripherals (`.4.16.1`): the [`minimeater_app::hal`] traits implemented on top of the
//! plant model, so application code runs unchanged on the host. Everything the peripherals touch
//! lives in one [`World`]; the twin (`.4.16.2`) or a test advances it with [`World::advance`] and
//! changes weather, lid, wiring and faults on it directly.
//!
//! What the sim treats as ground truth for the mechanics (servo travel, fan curve) is deliberately
//! separate from the firmware's own calibration of them, so a miscalibrated firmware shows up.

use core::cell::RefCell;
use core::convert::Infallible;
use core::future::poll_fn;
use core::task::{Poll, Waker};

use embedded_hal::digital::{ErrorType as PinErrorType, InputPin, OutputPin};
use embedded_hal::pwm::{ErrorType as PwmErrorType, SetDutyCycle};
use embedded_hal_async::digital::Wait;
use embedded_storage_async::nor_flash::{ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash};
use minimeater_app::hal::{AmbientSensor, Board, HalError, Io, PROBE_JACKS, ProbeAdc, Servo, Tach, UsbPower, Watchdog};
use minimeater_core::thermistor::{R_BIAS, Thermistor};
use minimeater_core::units::f_to_k;

use crate::{Actuators, Environment, Plant, Rng};

/// What is plugged into a jack.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wiring {
    /// Nothing plugged in; reads open (the PJ-242 tip switch is unverified, ADR 0002).
    Empty,
    /// The pit probe, reading the plant's lagged pit temperature.
    Pit(Thermistor),
    /// A food probe in `plant.meats[i]`'s core.
    Food(usize, Thermistor),
    /// A failed probe or cable (fault injection, `.4.16.4`).
    Short,
}

/// The damper servo's mechanics: pulse width at the flap's closed and fully open (90 degree) stops.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ServoMech {
    pub closed_us: f64,
    pub open_us: f64,
    /// How far past a stop the pulse must be before the servo, stalled against it, draws more than the load switch's limit
    /// and the shared fault line goes low, us.
    pub trip_us: f64,
}

impl Default for ServoMech {
    fn default() -> Self {
        Self {
            closed_us: 1000.0,
            open_us: 2000.0,
            trip_us: 30.0,
        }
    }
}

/// Injected hardware faults (`.4.16.4`): a scenario sets the whole state at once, and the sim
/// peripherals misbehave accordingly.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Faults {
    /// Every ADC conversion times out (the chip is dead or unplugged).
    pub adc_down: bool,
    /// Additive noise on every probe conversion, rms in divider ratio (about 1.4e-3 per F at 225F).
    pub adc_noise: f64,
    /// Probability that a conversion returns garbage anywhere in 0..1 (an EMI spike).
    pub adc_glitch: f64,
    /// The fan rotor is jammed: it does not turn whatever the duty.
    pub fan_jam: bool,
    /// The servo ignores its pulse: the flap stays where it is.
    pub servo_stuck: bool,
    /// The ambient sensor NACKs.
    pub ambient_nack: bool,
    /// C added to the ambient sensor's reading (the board next to it is overheating).
    pub ambient_offset_c: f64,
    /// A load switch is in current limit: the shared fault line reads low.
    pub load_fault: bool,
    /// Flash erases and writes fail.
    pub flash_fail: bool,
}

/// Size of the configuration region (the ESP-IDF default `nvs` partition, 0x6000).
pub const CONFIG_BYTES: usize = 0x6000;

/// The simulated board and everything around it.
pub struct World {
    pub plant: Plant,
    pub env: Environment,
    pub jacks: [Wiring; PROBE_JACKS],
    pub servo: ServoMech,
    /// Flap angle from closed, degrees; it only moves while the servo is powered.
    pub flap_deg: f64,
    pub servo_powered: bool,
    /// The fan's load switch is on; without it the rotor does not turn whatever the duty.
    pub fan_powered: bool,
    servo_pulse_us: u16,
    /// Fan PWM duty, 0..1. The fan runs at speed = duty and stalls below its minimum speed. [est]
    pub fan_duty: f64,
    pub faults: Faults,
    rng: Rng,
    pub led: bool,
    /// How many times the LED has changed state.
    pub led_edges: u32,
    button_pressed: bool,
    button_waker: Option<Waker>,
    pub usb_ma: Option<u16>,
    /// ADS1220 die temperature, C.
    pub board_c: f64,
    /// Survives [`World`] being reused across a simulated reboot.
    pub config: [u8; CONFIG_BYTES],
    /// Fan revolutions since the tach was last read, and the time they took.
    revs: f64,
    revs_s: f64,
    /// The watchdog: the timeout once armed (s), the time since the last feed, and the longest gap between feeds.
    pub wdt: Wdt,
}

/// What the simulated watchdog saw. It does not reset the twin; a run that expires it says so in `barked`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wdt {
    pub timeout_s: Option<f64>,
    pub since_feed_s: f64,
    pub longest_gap_s: f64,
    pub barked: bool,
}

impl World {
    pub fn new(plant: Plant, env: Environment) -> Self {
        Self {
            plant,
            env,
            jacks: [Wiring::Empty; PROBE_JACKS],
            servo: ServoMech::default(),
            flap_deg: 0.0,
            servo_powered: false,
            fan_powered: false,
            servo_pulse_us: 0,
            fan_duty: 0.0,
            faults: Faults::default(),
            rng: Rng::new(0x5EED),
            led: false,
            led_edges: 0,
            button_pressed: false,
            button_waker: None,
            usb_ma: Some(3000),
            board_c: 35.0,
            config: [0xFF; CONFIG_BYTES],
            revs: 0.0,
            revs_s: 0.0,
            wdt: Wdt::default(),
        }
    }

    /// Flap open-area fraction (butterfly, 1 - cos).
    pub fn damper(&self) -> f64 {
        1.0 - libm::cos(self.flap_deg.clamp(0.0, 90.0).to_radians())
    }

    /// Fan speed fraction actually reached.
    pub fn fan_speed(&self) -> f64 {
        let s = if self.fan_powered {
            self.fan_duty.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if self.faults.fan_jam || s < self.plant.path.fan.s_min() {
            0.0
        } else {
            s
        }
    }

    /// Advance the physics by `dt` seconds with the actuators as the firmware left them.
    pub fn advance(&mut self, dt: f64) {
        let act = Actuators {
            damper: self.damper(),
            fan: self.fan_speed(),
        };
        self.plant.step(dt, act, &self.env);
        self.revs += act.fan * self.plant.path.fan.rpm_max / 60.0 * dt;
        self.revs_s += dt;
        if let Some(t) = self.wdt.timeout_s {
            self.wdt.since_feed_s += dt;
            self.wdt.longest_gap_s = self.wdt.longest_gap_s.max(self.wdt.since_feed_s);
            self.wdt.barked |= self.wdt.since_feed_s > t;
        }
    }

    fn set_led(&mut self, on: bool) {
        self.led_edges += u32::from(self.led != on);
        self.led = on;
    }

    pub fn set_button(&mut self, pressed: bool) {
        self.button_pressed = pressed;
        if let Some(w) = self.button_waker.take() {
            w.wake();
        }
    }

    fn jack_ratio(&self, jack: usize) -> f64 {
        let ratio = |th: &Thermistor, t_k: f64| {
            let r = th.r(t_k);
            r / (r + R_BIAS)
        };
        match self.jacks[jack] {
            Wiring::Empty => 1.0,
            Wiring::Short => 0.0,
            Wiring::Pit(th) => ratio(&th, f_to_k(self.plant.pit_probe_f())),
            Wiring::Food(i, th) => match &self.plant.meats[i] {
                Some(m) => ratio(&th, f_to_k(m.core_f())),
                None => 1.0,
            },
        }
    }
}

/// Each peripheral holds a shared reference to the world; borrows never cross an `await`.
pub type Shared<'a> = &'a RefCell<World>;

pub struct SimProbes<'a>(Shared<'a>);
pub struct SimAmbient<'a>(Shared<'a>);
pub struct SimFan<'a>(Shared<'a>);
pub struct SimTach<'a>(Shared<'a>);
pub struct SimServo<'a>(Shared<'a>);
pub struct SimServoPower<'a>(Shared<'a>);
pub struct SimFanPower<'a>(Shared<'a>);
pub struct SimLoadFault<'a>(Shared<'a>);
pub struct SimLed<'a>(Shared<'a>);
pub struct SimButton<'a>(Shared<'a>);
pub struct SimConfig<'a>(Shared<'a>);
pub struct SimUsb<'a>(Shared<'a>);
pub struct SimWatchdog<'a>(Shared<'a>);

pub struct SimBoard<'a>(core::marker::PhantomData<&'a ()>);

impl<'a> Board for SimBoard<'a> {
    type Probes = SimProbes<'a>;
    type Ambient = SimAmbient<'a>;
    type Fan = SimFan<'a>;
    type Tach = SimTach<'a>;
    type Servo = SimServo<'a>;
    type ServoPower = SimServoPower<'a>;
    type FanPower = SimFanPower<'a>;
    type LoadFault = SimLoadFault<'a>;
    type Led = SimLed<'a>;
    type Button = SimButton<'a>;
    type Config = SimConfig<'a>;
    type Usb = SimUsb<'a>;
    type Watchdog = SimWatchdog<'a>;
}

/// The application's view of `world`.
pub fn io(world: Shared<'_>) -> Io<SimBoard<'_>> {
    Io {
        probes: SimProbes(world),
        ambient: SimAmbient(world),
        fan: SimFan(world),
        tach: SimTach(world),
        servo: SimServo(world),
        servo_power: SimServoPower(world),
        fan_power: SimFanPower(world),
        load_fault: SimLoadFault(world),
        led: SimLed(world),
        button: SimButton(world),
        config: SimConfig(world),
        usb: SimUsb(world),
        watchdog: SimWatchdog(world),
    }
}

impl ProbeAdc for SimProbes<'_> {
    async fn ratio(&mut self, jack: usize) -> Result<f64, HalError> {
        let mut w = self.0.borrow_mut();
        let f = w.faults;
        if f.adc_down {
            return Err(HalError::Timeout);
        }
        let mut x = w.jack_ratio(jack);
        // noise rides on a probe's signal; an empty or shorted jack sits on its rail
        if f.adc_noise > 0.0 && (1e-3..0.99).contains(&x) {
            x += f.adc_noise * w.rng.normal();
        }
        if f.adc_glitch > 0.0 && w.rng.uniform() < f.adc_glitch {
            x = w.rng.uniform();
        }
        Ok(x)
    }

    async fn offset(&mut self) -> Result<f64, HalError> {
        if self.0.borrow().faults.adc_down {
            return Err(HalError::Timeout);
        }
        Ok(0.0)
    }

    async fn die_temp_c(&mut self) -> Result<f64, HalError> {
        let w = self.0.borrow();
        if w.faults.adc_down {
            return Err(HalError::Timeout);
        }
        Ok(w.board_c)
    }
}

/// What the simulated sensor picks up of the board's excess over the air.
const SENSOR_SELF_HEAT: f64 = 0.12;

impl AmbientSensor for SimAmbient<'_> {
    async fn read_c(&mut self) -> Result<f64, HalError> {
        let w = self.0.borrow();
        if w.faults.ambient_nack {
            return Err(HalError::Bus);
        }
        // the board's heat on the sensor, a little more than the firmware assumes (`minimeater_app::ambient::SELF_HEAT`)
        let air = w.env.ambient.t_k - 273.15;
        Ok(air + SENSOR_SELF_HEAT * (w.board_c - air) + w.faults.ambient_offset_c)
    }
}

impl PwmErrorType for SimFan<'_> {
    type Error = Infallible;
}

impl SetDutyCycle for SimFan<'_> {
    fn max_duty_cycle(&self) -> u16 {
        1023
    }

    fn set_duty_cycle(&mut self, duty: u16) -> Result<(), Infallible> {
        self.0.borrow_mut().fan_duty = f64::from(duty.min(1023)) / 1023.0;
        Ok(())
    }
}

impl Tach for SimTach<'_> {
    fn rpm(&mut self) -> Option<f64> {
        let mut w = self.0.borrow_mut();
        if w.revs_s <= 0.0 {
            return None;
        }
        let rpm = w.revs / w.revs_s * 60.0;
        (w.revs, w.revs_s) = (0.0, 0.0);
        Some(rpm)
    }
}

impl Servo for SimServo<'_> {
    fn set_pulse_us(&mut self, us: u16) {
        let mut w = self.0.borrow_mut();
        w.servo_pulse_us = us;
        if w.servo_powered {
            w.move_flap();
        }
    }
}

impl World {
    /// The shared fault line of the two load switches is low: an injected fault, or the powered servo pushed past a hard stop.
    fn load_fault_low(&self) -> bool {
        let m = self.servo;
        let (lo, hi) = (m.closed_us.min(m.open_us), m.closed_us.max(m.open_us));
        let p = f64::from(self.servo_pulse_us);
        let stalled = self.servo_powered && self.servo_pulse_us != 0 && (p < lo - m.trip_us || p > hi + m.trip_us);
        self.faults.load_fault || stalled
    }
}

impl World {
    /// A powered servo goes straight to its commanded angle (the 10 s control period dwarfs its
    /// ~0.2 s travel). A pulse outside the mechanical stops leaves the horn at the stop.
    fn move_flap(&mut self) {
        if self.servo_pulse_us == 0 || self.faults.servo_stuck {
            return;
        }
        let m = self.servo;
        let frac = (f64::from(self.servo_pulse_us) - m.closed_us) / (m.open_us - m.closed_us);
        self.flap_deg = 90.0 * frac.clamp(0.0, 1.0);
    }
}

impl PinErrorType for SimServoPower<'_> {
    type Error = Infallible;
}

impl OutputPin for SimServoPower<'_> {
    fn set_low(&mut self) -> Result<(), Infallible> {
        self.0.borrow_mut().servo_powered = false;
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Infallible> {
        let mut w = self.0.borrow_mut();
        w.servo_powered = true;
        w.move_flap();
        Ok(())
    }
}

impl PinErrorType for SimFanPower<'_> {
    type Error = Infallible;
}

impl OutputPin for SimFanPower<'_> {
    fn set_low(&mut self) -> Result<(), Infallible> {
        self.0.borrow_mut().fan_powered = false;
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Infallible> {
        self.0.borrow_mut().fan_powered = true;
        Ok(())
    }
}

impl PinErrorType for SimLoadFault<'_> {
    type Error = Infallible;
}

impl InputPin for SimLoadFault<'_> {
    fn is_high(&mut self) -> Result<bool, Infallible> {
        Ok(!self.0.borrow().load_fault_low())
    }

    fn is_low(&mut self) -> Result<bool, Infallible> {
        Ok(self.0.borrow().load_fault_low())
    }
}

impl PinErrorType for SimLed<'_> {
    type Error = Infallible;
}

impl OutputPin for SimLed<'_> {
    fn set_low(&mut self) -> Result<(), Infallible> {
        self.0.borrow_mut().set_led(false);
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Infallible> {
        self.0.borrow_mut().set_led(true);
        Ok(())
    }
}

impl PinErrorType for SimButton<'_> {
    type Error = Infallible;
}

impl InputPin for SimButton<'_> {
    fn is_high(&mut self) -> Result<bool, Infallible> {
        Ok(!self.0.borrow().button_pressed)
    }

    fn is_low(&mut self) -> Result<bool, Infallible> {
        Ok(self.0.borrow().button_pressed)
    }
}

impl SimButton<'_> {
    /// Resolves once the button is pressed (`true`) or released (`false`).
    async fn until(&mut self, pressed: bool) {
        poll_fn(|cx| {
            let mut w = self.0.borrow_mut();
            if w.button_pressed == pressed {
                Poll::Ready(())
            } else {
                w.button_waker = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
}

impl Wait for SimButton<'_> {
    async fn wait_for_high(&mut self) -> Result<(), Infallible> {
        self.until(false).await;
        Ok(())
    }

    async fn wait_for_low(&mut self) -> Result<(), Infallible> {
        self.until(true).await;
        Ok(())
    }

    async fn wait_for_rising_edge(&mut self) -> Result<(), Infallible> {
        self.until(true).await;
        self.until(false).await;
        Ok(())
    }

    async fn wait_for_falling_edge(&mut self) -> Result<(), Infallible> {
        self.until(false).await;
        self.until(true).await;
        Ok(())
    }

    async fn wait_for_any_edge(&mut self) -> Result<(), Infallible> {
        let now = self.0.borrow().button_pressed;
        self.until(!now).await;
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum FlashError {
    OutOfBounds,
    NotAligned,
    /// Injected: the erase or write did not happen (`Faults::flash_fail`).
    Failed,
}

impl NorFlashError for FlashError {
    fn kind(&self) -> NorFlashErrorKind {
        match self {
            FlashError::OutOfBounds => NorFlashErrorKind::OutOfBounds,
            FlashError::NotAligned => NorFlashErrorKind::NotAligned,
            FlashError::Failed => NorFlashErrorKind::Other,
        }
    }
}

impl ErrorType for SimConfig<'_> {
    type Error = FlashError;
}

fn span(offset: u32, len: usize, align: usize) -> Result<core::ops::Range<usize>, FlashError> {
    let start = offset as usize;
    if !start.is_multiple_of(align) || !len.is_multiple_of(align) {
        return Err(FlashError::NotAligned);
    }
    match start.checked_add(len) {
        Some(end) if end <= CONFIG_BYTES => Ok(start..end),
        _ => Err(FlashError::OutOfBounds),
    }
}

impl ReadNorFlash for SimConfig<'_> {
    const READ_SIZE: usize = 1;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), FlashError> {
        bytes.copy_from_slice(&self.0.borrow().config[span(offset, bytes.len(), 1)?]);
        Ok(())
    }

    fn capacity(&self) -> usize {
        CONFIG_BYTES
    }
}

/// NOR semantics like the ESP32-C6's flash: erase sets 4 KB sectors to 0xFF, a write can only clear
/// bits, so a missing erase corrupts data here as it would on the device.
impl NorFlash for SimConfig<'_> {
    const WRITE_SIZE: usize = 4;
    const ERASE_SIZE: usize = 4096;

    async fn erase(&mut self, from: u32, to: u32) -> Result<(), FlashError> {
        let r = span(from, to.saturating_sub(from) as usize, Self::ERASE_SIZE)?;
        if self.0.borrow().faults.flash_fail {
            return Err(FlashError::Failed);
        }
        self.0.borrow_mut().config[r].fill(0xFF);
        Ok(())
    }

    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), FlashError> {
        let r = span(offset, bytes.len(), Self::WRITE_SIZE)?;
        if self.0.borrow().faults.flash_fail {
            return Err(FlashError::Failed);
        }
        for (d, s) in self.0.borrow_mut().config[r].iter_mut().zip(bytes) {
            *d &= *s;
        }
        Ok(())
    }
}

impl Watchdog for SimWatchdog<'_> {
    fn start(&mut self, timeout_ms: u32) {
        let mut w = self.0.borrow_mut();
        w.wdt = Wdt {
            timeout_s: Some(f64::from(timeout_ms) * 1e-3),
            ..Wdt::default()
        };
    }

    fn feed(&mut self) {
        self.0.borrow_mut().wdt.since_feed_s = 0.0;
    }
}

impl UsbPower for SimUsb<'_> {
    fn source_current_ma(&mut self) -> Option<u16> {
        self.0.borrow().usb_ma
    }
}
