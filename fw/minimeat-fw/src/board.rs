//! The ESP32-C6 board: `minimeat-app`'s peripheral traits bound to esp-hal (`.4.16.1`). This module is
//! the only place that knows pin numbers and peripheral instances.
//!
//! Pin map: the schematic's contract (`.3.3`; hw/minimeat.kicad_sch). Constraints honoured (ADR 0001): CC sense
//! on ADC1 pins (GPIO0-6), button on GPIO9 (BOOT), nothing on GPIO8 (strapping) or GPIO12/13 (USB).
//!
//! | Signal | GPIO | | Signal | GPIO |
//! |---|---|---|---|---|
//! | USB-C CC1 / CC2 (ADC1) | 0 / 1 | | I2C SDA / SCL (ADS122C04, TMP117, ATECC608B) | 22 / 23 |
//! | ADS122C04 DRDY | 2 | | fan PWM / tach | 18 / 19 |
//! | button (BOOT) | 9 | | servo PWM / power enable | 20 / 21 |
//! | status LED | 10 | | fan power enable (TPS2553) | 11 |
//! | load-switch fault (open drain, shared) | 3 | | spare (no-connect) | 4 / 5 / 6 / 7 / 15 |
//! | UART0 TX / RX (test pads) | 16 / 17 | | USB D- / D+ | 12 / 13 |

use embassy_embedded_hal::adapter::BlockingAsync;
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Delay, Instant};
use esp_hal::Async;
use esp_hal::analog::adc::{Adc, AdcCalCurve, AdcConfig, AdcPin, Attenuation};
use esp_hal::gpio::{DriveMode, Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::i2c::master::I2c;
use esp_hal::ledc::channel::{self, ChannelIFace};
use esp_hal::ledc::timer::{self, TimerIFace};
use esp_hal::ledc::{LSGlobalClkSource, Ledc, LowSpeed};
use esp_hal::pcnt::{Pcnt, channel::EdgeMode, unit::Unit};
use esp_hal::peripherals::{ADC1, FROM_CPU_INTR0, GPIO0, GPIO1, Peripherals, TIMG0, USB_DEVICE};
use esp_hal::time::Rate;
use esp_storage::FlashStorage;
use minimeat_app::drivers::{ads122c04, ads122c04::Ads122c04, tmp117, tmp117::Tmp117, usb_cc};
use minimeat_app::hal::{Board, FlashRegion, Io, PwmServo, Tach, UsbPower};
use static_cell::StaticCell;

/// Configuration region: the `nvs` partition of espflash's default partition table. `.4.13` (OTA)
/// replaces the table; keep this in step with it.
const CONFIG_OFFSET: u32 = 0x9000;
const CONFIG_LEN: u32 = 0x6000;

/// Fan PWM: 25 kHz (Intel 4-wire fan spec), 10-bit at the 80 MHz APB clock.
const FAN_PWM_HZ: u32 = 25_000;
/// Servo frame: 50 Hz, 14-bit, so 1 us is about 0.8 LSB.
const SERVO_PERIOD_US: u32 = 20_000;

/// The fan's tach gives two pulses per revolution (4-wire PC fan convention).
const TACH_PULSES_PER_REV: f64 = 2.0;

pub struct Esp;

/// The one I2C bus, shared by the probe ADC and the ambient sensor.
type Bus = I2cDevice<'static, CriticalSectionRawMutex, I2c<'static, Async>>;
type Probes = Ads122c04<Bus, Input<'static>, Delay>;
type Pwm = channel::Channel<'static, LowSpeed>;

impl Board for Esp {
    type Probes = Probes;
    type Ambient = Tmp117<Bus>;
    type Fan = Pwm;
    type Tach = PcntTach;
    type Servo = PwmServo<Pwm>;
    type ServoPower = Output<'static>;
    type FanPower = Output<'static>;
    type LoadFault = Input<'static>;
    type Led = Output<'static>;
    type Button = Input<'static>;
    type Config = FlashRegion<BlockingAsync<FlashStorage<'static>>>;
    type Usb = CcSense;
}

/// Peripherals the board does not wrap, for the runtime and (later) the radio.
pub struct Rest {
    pub timg0: TIMG0<'static>,
    pub sw_interrupt: FROM_CPU_INTR0<'static>,
    /// The USB Serial/JTAG port: console logs and Improv provisioning share it (`console.rs`).
    pub usb: USB_DEVICE<'static>,
}

/// Brings up every peripheral. Panics only on start-up bugs (bad static configuration); a missing
/// or broken chip shows up later as a read error, not here.
pub fn init(p: Peripherals) -> (Io<Esp>, Rest) {
    let out = OutputConfig::default();

    let i2c = I2c::new(
        p.I2C0,
        // 100 kHz: the ATECC608B (ADR 0005) wakes on SDA held low 60 us, which a write to address 0 gives
        // only at this speed; the ADC and the TMP117 need no more.
        esp_hal::i2c::master::Config::default().with_frequency(Rate::from_khz(100)),
    )
    .expect("i2c config")
    .with_sda(p.GPIO22)
    .with_scl(p.GPIO23)
    .into_async();
    static I2C_BUS: StaticCell<Mutex<CriticalSectionRawMutex, I2c<'static, Async>>> = StaticCell::new();
    let i2c = I2C_BUS.init(Mutex::new(i2c));
    let drdy = Input::new(p.GPIO2, InputConfig::default().with_pull(Pull::Up));
    let probes = Ads122c04::new(I2cDevice::new(i2c), ads122c04::ADDRESS, drdy, Delay);
    let ambient = Tmp117::new(I2cDevice::new(i2c), tmp117::ADDRESS);

    static LEDC: StaticCell<Ledc<'static>> = StaticCell::new();
    static FAN_TIMER: StaticCell<timer::Timer<'static, LowSpeed>> = StaticCell::new();
    static SERVO_TIMER: StaticCell<timer::Timer<'static, LowSpeed>> = StaticCell::new();
    let ledc = LEDC.init(Ledc::new(p.LEDC));
    ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);
    let fan_timer = FAN_TIMER.init(ledc.timer(timer::Number::Timer0));
    fan_timer
        .configure(timer::config::Config {
            duty: timer::config::Duty::Duty10Bit,
            clock_source: timer::LSClockSource::APBClk,
            frequency: Rate::from_hz(FAN_PWM_HZ),
        })
        .expect("fan timer");
    let servo_timer = SERVO_TIMER.init(ledc.timer(timer::Number::Timer1));
    servo_timer
        .configure(timer::config::Config {
            duty: timer::config::Duty::Duty14Bit,
            clock_source: timer::LSClockSource::APBClk,
            frequency: Rate::from_hz(1_000_000 / SERVO_PERIOD_US),
        })
        .expect("servo timer");
    let mut fan = ledc.channel(channel::Number::Channel0, p.GPIO18);
    fan.configure(channel::config::Config {
        timer: fan_timer,
        duty_pct: 0,
        drive_mode: DriveMode::PushPull,
    })
    .expect("fan channel");
    let mut servo = ledc.channel(channel::Number::Channel1, p.GPIO20);
    servo
        .configure(channel::config::Config {
            timer: servo_timer,
            duty_pct: 0,
            drive_mode: DriveMode::PushPull,
        })
        .expect("servo channel");

    static PCNT: StaticCell<Pcnt<'static>> = StaticCell::new();
    let pcnt = PCNT.init(Pcnt::new(p.PCNT));
    let tach_pin = Input::new(p.GPIO19, InputConfig::default().with_pull(Pull::Up));
    let unit = &pcnt.unit0;
    unit.set_filter(Some(1023)).expect("tach filter");
    unit.channel0.set_edge_signal(tach_pin.peripheral_input());
    unit.channel0.set_input_mode(EdgeMode::Hold, EdgeMode::Increment);
    unit.clear();
    unit.resume();

    let mut adc_cfg = AdcConfig::new();
    let cc1 = adc_cfg.enable_pin_with_cal::<_, AdcCalCurve<ADC1<'static>>>(p.GPIO0, Attenuation::_11dB);
    let cc2 = adc_cfg.enable_pin_with_cal::<_, AdcCalCurve<ADC1<'static>>>(p.GPIO1, Attenuation::_11dB);

    let io = Io {
        probes,
        ambient,
        fan,
        tach: PcntTach { unit, since: None },
        servo: PwmServo::new(servo, SERVO_PERIOD_US),
        servo_power: Output::new(p.GPIO21, Level::Low, out),
        fan_power: Output::new(p.GPIO11, Level::Low, out),
        // the shared open-drain fault line has its own pull-up on the board
        load_fault: Input::new(p.GPIO3, InputConfig::default().with_pull(Pull::None)),
        led: Output::new(p.GPIO10, Level::Low, out),
        button: Input::new(p.GPIO9, InputConfig::default().with_pull(Pull::Up)),
        config: FlashRegion::new(
            BlockingAsync::new(FlashStorage::new(p.FLASH)),
            CONFIG_OFFSET,
            CONFIG_LEN,
        ),
        usb: CcSense {
            adc: Adc::new(p.ADC1, adc_cfg),
            cc1,
            cc2,
        },
    };
    let rest = Rest {
        timg0: p.TIMG0,
        sw_interrupt: p.FROM_CPU_INTR0,
        usb: p.USB_DEVICE,
    };
    (io, rest)
}

/// Fan tach on a pulse-counter unit: rising edges, glitch-filtered, read and cleared each call.
pub struct PcntTach {
    unit: &'static Unit<'static, 0>,
    /// Start of the current count; `None` until the first call.
    since: Option<Instant>,
}

impl Tach for PcntTach {
    fn rpm(&mut self) -> Option<f64> {
        let now = Instant::now();
        let pulses = f64::from(self.unit.value());
        self.unit.clear();
        let s = (now - self.since.replace(now)?).as_micros() as f64 * 1e-6;
        (s > 0.0).then(|| pulses / TACH_PULSES_PER_REV / s * 60.0)
    }
}

type CcPin<P> = AdcPin<P, ADC1<'static>, AdcCalCurve<ADC1<'static>>>;

/// CC1/CC2 voltages across the 5.1k Rd pull-downs.
pub struct CcSense {
    adc: Adc<'static, ADC1<'static>, esp_hal::Blocking>,
    cc1: CcPin<GPIO0<'static>>,
    cc2: CcPin<GPIO1<'static>>,
}

impl UsbPower for CcSense {
    fn source_current_ma(&mut self) -> Option<u16> {
        let cc1 = nb::block!(self.adc.read_oneshot(&mut self.cc1)).unwrap_or(0);
        let cc2 = nb::block!(self.adc.read_oneshot(&mut self.cc2)).unwrap_or(0);
        usb_cc::source_current_ma(cc1, cc2)
    }
}
