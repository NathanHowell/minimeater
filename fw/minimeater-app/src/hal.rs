//! The firmware's peripheral boundary (`.4.16.1`). Application logic sees the board only through
//! these traits; `minimeater-fw` implements them with esp-hal and `minimeater-sim::hal` with the plant
//! model, so the same application code runs on the ESP32-C6 and in the host twin.
//!
//! Standard traits are used where they fit: `embedded-hal` PWM for the fan, GPIO for the LED, the servo
//! power gate and the button (`embedded-hal-async` `Wait` for presses), and `embedded-storage-async`
//! `NorFlash` for the configuration region. Project traits cover what has no standard shape: a
//! ratiometric probe ADC, an ambient sensor, a tach, a servo by pulse width and the USB-C source.
//!
//! Time is not a trait: application code uses `embassy-time`, whose driver is the esp-rtos timer on
//! the target and the twin's (possibly faster-than-real-time) driver on the host.

use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::pwm::SetDutyCycle;
use embedded_hal_async::digital::Wait;
use embedded_storage_async::nor_flash::{ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash};

/// Why a peripheral read failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HalError {
    /// The bus transaction failed (I2C error, NAK).
    Bus,
    /// The device did not answer in time (e.g. ADS122C04 DRDY never fell).
    Timeout,
    /// The device answered but has no valid data yet (e.g. TMP117 before its first conversion).
    NotReady,
    /// The device is missing or is not the expected part.
    Absent,
}

/// Number of thermistor jacks (ADR 0002).
pub const PROBE_JACKS: usize = 4;

/// The ratiometric probe front end (ADR 0002): each reading is the divider ratio
/// `x = R_t / (R_t + R_BIAS)`, which [`minimeater_core::thermistor::Jack::classify`] turns into a
/// resistance. One call is one conversion; averaging, filtering and the round robin are
/// application logic (`.4.7`).
#[allow(async_fn_in_trait, reason = "single-threaded embassy executor; no Send bound wanted")]
pub trait ProbeAdc {
    /// One conversion of jack `jack` (0..[`PROBE_JACKS`]).
    async fn ratio(&mut self, jack: usize) -> Result<f64, HalError>;
    /// Offset check with the inputs shorted at mid-supply, as a ratio (ideally 0).
    async fn offset(&mut self) -> Result<f64, HalError>;
    /// ADC die temperature, C (board health; the ADC must stay below 85C, ADR 0002).
    async fn die_temp_c(&mut self) -> Result<f64, HalError>;
}

/// Outside-air temperature at the box inlet (ADR 0002: TMP117).
#[allow(async_fn_in_trait, reason = "single-threaded embassy executor; no Send bound wanted")]
pub trait AmbientSensor {
    async fn read_c(&mut self) -> Result<f64, HalError>;
}

/// Fan tachometer.
pub trait Tach {
    /// Mean speed since the previous call, rpm; `None` if there was nothing to measure yet.
    fn rpm(&mut self) -> Option<f64>;
}

/// Hobby servo, commanded by pulse width. Angle calibration and slew limiting are application logic
/// (`.4.8`); the gate that powers the servo is a separate [`OutputPin`].
pub trait Servo {
    fn set_pulse_us(&mut self, us: u16);
}

/// What the USB-C source advertises on CC (ADR 0001: CC1/CC2 sensed on ADC1 pins).
pub trait UsbPower {
    /// Advertised current, mA (500/900 USB default, 1500, 3000); `None` if no source is detected.
    fn source_current_ma(&mut self) -> Option<u16>;
}

/// The hardware watchdog: armed once with a timeout, then fed. A missed feed resets the board, which boots into the
/// saved cook (`.4.24`). The control task feeds it.
pub trait Watchdog {
    /// Arms (and feeds) it: the board resets if `timeout_ms` pass without a [`feed`](Self::feed).
    fn start(&mut self, timeout_ms: u32);
    fn feed(&mut self);
}

/// The board's peripheral types. Embassy tasks cannot be generic, so each binary names its board's
/// concrete types once and wraps the generic application functions in thin `#[task]`s.
pub trait Board {
    type Probes: ProbeAdc;
    type Ambient: AmbientSensor;
    /// Fan PWM (25 kHz on the target); duty maps to speed through the fan curve (`.4.8`).
    type Fan: SetDutyCycle;
    type Tach: Tach;
    type Servo: Servo;
    /// High powers the servo; it is cut while stationary to stop hold current and jitter.
    type ServoPower: OutputPin;
    /// High powers the fan through its load switch, so 0 % really stops it.
    type FanPower: OutputPin;
    /// The load switches' shared open-drain fault line: low while one of them is limiting its current.
    type LoadFault: InputPin;
    type Led: OutputPin;
    /// The user button (GPIO9/BOOT on the target, ADR 0001): low while pressed.
    type Button: InputPin + Wait;
    /// The configuration region only, not the whole flash (`.4.9`).
    type Config: NorFlash;
    type Usb: UsbPower;
    type Watchdog: Watchdog;
}

/// Every peripheral of a board, handed to the application at start-up.
pub struct Io<B: Board> {
    pub probes: B::Probes,
    pub ambient: B::Ambient,
    pub fan: B::Fan,
    pub tach: B::Tach,
    pub servo: B::Servo,
    pub servo_power: B::ServoPower,
    pub fan_power: B::FanPower,
    pub load_fault: B::LoadFault,
    pub led: B::Led,
    pub button: B::Button,
    pub config: B::Config,
    pub usb: B::Usb,
    pub watchdog: B::Watchdog,
}

/// A [`Servo`] on a PWM channel running at `period_us` (20 000 for 50 Hz).
pub struct PwmServo<P> {
    pwm: P,
    period_us: u32,
}

impl<P: SetDutyCycle> PwmServo<P> {
    pub fn new(pwm: P, period_us: u32) -> Self {
        Self { pwm, period_us }
    }
}

impl<P: SetDutyCycle> Servo for PwmServo<P> {
    fn set_pulse_us(&mut self, us: u16) {
        let max = u32::from(self.pwm.max_duty_cycle());
        let duty = (u32::from(us) * max + self.period_us / 2) / self.period_us;
        // LEDC only fails on an unconfigured channel, which is a start-up bug, not a runtime fault.
        let _ = self.pwm.set_duty_cycle(duty.min(max) as u16);
    }
}

/// A window `[offset, offset + len)` of a larger flash, presented as its own flash starting at 0, so
/// the configuration store cannot touch the firmware image.
pub struct FlashRegion<F> {
    flash: F,
    offset: u32,
    len: u32,
}

impl<F: NorFlash> FlashRegion<F> {
    /// `OFFSET` and `LEN` must be multiples of the erase size, which the build checks.
    pub fn new<const OFFSET: u32, const LEN: u32>(flash: F) -> Self {
        const {
            assert!((OFFSET as usize).is_multiple_of(F::ERASE_SIZE) && (LEN as usize).is_multiple_of(F::ERASE_SIZE));
        }
        Self {
            flash,
            offset: OFFSET,
            len: LEN,
        }
    }

    fn check(&self, from: u32, n: usize) -> Result<u32, RegionError<F::Error>> {
        match from.checked_add(n as u32) {
            Some(end) if end <= self.len => Ok(self.offset + from),
            _ => Err(RegionError::OutOfBounds),
        }
    }
}

#[derive(Debug)]
pub enum RegionError<E> {
    OutOfBounds,
    Flash(E),
}

impl<E: NorFlashError> NorFlashError for RegionError<E> {
    fn kind(&self) -> NorFlashErrorKind {
        match self {
            RegionError::OutOfBounds => NorFlashErrorKind::OutOfBounds,
            RegionError::Flash(e) => e.kind(),
        }
    }
}

impl<F: NorFlash> ErrorType for FlashRegion<F> {
    type Error = RegionError<F::Error>;
}

impl<F: NorFlash> ReadNorFlash for FlashRegion<F> {
    const READ_SIZE: usize = F::READ_SIZE;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        let at = self.check(offset, bytes.len())?;
        self.flash.read(at, bytes).await.map_err(RegionError::Flash)
    }

    fn capacity(&self) -> usize {
        self.len as usize
    }
}

impl<F: NorFlash> NorFlash for FlashRegion<F> {
    const WRITE_SIZE: usize = F::WRITE_SIZE;
    const ERASE_SIZE: usize = F::ERASE_SIZE;

    async fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        if to < from {
            return Err(RegionError::OutOfBounds);
        }
        let start = self.check(from, (to - from) as usize)?;
        self.flash
            .erase(start, start + (to - from))
            .await
            .map_err(RegionError::Flash)
    }

    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        let at = self.check(offset, bytes.len())?;
        self.flash.write(at, bytes).await.map_err(RegionError::Flash)
    }
}
