#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe with esp_hal types, especially those holding DMA buffers"
)]
#![deny(clippy::large_stack_frames)]

use embassy_executor::Spawner;
use embassy_time::{Duration, Instant, Ticker, Timer};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;
use log::info;
use minimeater_app::config::{ConfigStore, Settings};
use minimeater_app::food::Foods;
use minimeater_app::hal::Board;
use minimeater_app::improv;
use minimeater_app::net::Identity;
use minimeater_app::pit::Pit;
use minimeater_app::selftest;
use minimeater_app::tasks::{self, Bus};
use minimeater_app::ui;
use minimeater_core::model::airflow::AirPath;
use minimeater_fw::board::{self, Esp};
use minimeater_fw::console::{self, Port};
use minimeater_fw::net::{self, Link, Tcp};
use static_cell::{ConstStaticCell, StaticCell};

// App descriptor required by the ESP-IDF second-stage bootloader.
esp_bootloader_esp_idf::esp_app_desc!();

type B = Esp;

#[allow(
    clippy::large_stack_frames,
    reason = "main holds the peripherals for the device's lifetime"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    // the radio driver's heap; the application itself does not allocate
    esp_alloc::heap_allocator!(size: 72 * 1024);
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    let (mut io, rest) = board::init(peripherals);
    // the logger and the one USB serial port, shared with Improv provisioning
    let (console_tx, port) = console::init(rest.usb);

    let timg0 = TimerGroup::new(rest.timg0);
    esp_rtos::start(timg0.timer0, rest.sw_interrupt);
    info!("minimeater-fw {} up", env!("CARGO_PKG_VERSION"));

    spawner.spawn(console::writer(console_tx).expect("console task"));
    spawner.spawn(heartbeat().expect("heartbeat task"));
    selftest::survey(&mut io).await;

    // Settings, the learned model and the cook in progress: a power cut resumes from here.
    let (store, settings) = ConfigStore::open(io.config).await;
    info!(
        "config: {:?} at {}F ({:?}), autotune {}, {} learned models, wifi {}",
        settings.cook.mode,
        settings.cook.sp_f,
        settings.cook.controller,
        settings.cook.autotune,
        settings.schedule.models().len(),
        settings.wifi.is_some()
    );

    // The application's tasks on the board, as the twin runs them on the simulated one: probes scan,
    // control runs the cook, actuators drive the fan and flap and watch the supply and the load switches
    // (a source under 1.5 A or a tripped switch is a fault with both loads off), slow does the fits.
    let model = AirPath::DEFAULT;
    // built at compile time: no copy through main's stack at boot
    static BUS: Bus = Bus::new();
    let bus: &'static Bus = &BUS;
    let reason = minimeater_fw::reset::boot_reason();
    info!("boot: {}", reason.name());
    bus.boot.lock(|c| c.set(Some(reason)));
    static PIT: ConstStaticCell<Pit> = ConstStaticCell::new(Pit::BLANK);
    let pit = PIT.take();
    pit.load(&settings, model, minimeater_core::model::Ambient::DEFAULT);
    pit.boot(&settings);
    static FOODS: StaticCell<Foods> = StaticCell::new();
    let foods = FOODS.init_with(|| Foods::new(&settings.probes));

    spawner.spawn(probes(io.probes, bus).expect("probes task"));
    spawner.spawn(
        actuators(
            io.fan,
            io.tach,
            io.servo,
            io.servo_power,
            io.fan_power,
            io.load_fault,
            io.servo_current,
            io.usb,
            model.fan.s_min(),
            model.fan.rpm_max,
            bus,
        )
        .expect("actuators task"),
    );
    spawner.spawn(slow(bus).expect("slow task"));
    spawner.spawn(status_led(io.led, bus).expect("led task"));
    spawner.spawn(user_button(io.button, bus).expect("button task"));
    spawner.spawn(reboot(bus).expect("reboot task"));
    static ID: StaticCell<heapless::String<12>> = StaticCell::new();
    let id: &'static str = ID.init(device_id());
    spawner.spawn(provisioning(port, id, bus).expect("provisioning task"));
    spawner.spawn(enrol(rest.secure_element, id, bus).expect("enrol task"));

    // WiFi and the clients that use it: the radio joins the saved network, MQTT and OTLP connect when their settings
    // exist and the link is up
    static HOST: StaticCell<heapless::String<24>> = StaticCell::new();
    let host: &'static str = HOST.init(hostname(id));
    let rng = esp_hal::rng::Rng::new();
    let seed = u64::from(rng.random()) << 32 | u64::from(rng.random());
    let (controller, stack, runner) = net::init(rest.wifi, host, seed);
    spawner.spawn(net::stack_runner(runner).expect("net task"));
    spawner.spawn(net::wifi(controller, stack, bus).expect("wifi task"));
    spawner.spawn(net::sntp_client(stack, seed).expect("sntp task"));
    spawner.spawn(net::mdns_responder(stack, host).expect("mdns task"));
    static MQTT_BUFS: ConstStaticCell<minimeater_app::mqtt::Buffers> =
        ConstStaticCell::new(minimeater_app::mqtt::Buffers::new());
    static OTLP_BUFS: ConstStaticCell<minimeater_app::otlp::Buffers> =
        ConstStaticCell::new(minimeater_app::otlp::Buffers::new());
    // TLS for MQTT when a CA is pinned (`mqtt ca`): the device key in the secure element, two record buffers
    static TLS_READ: ConstStaticCell<[u8; minimeater_app::tls::RECORD]> =
        ConstStaticCell::new([0; minimeater_app::tls::RECORD]);
    static TLS_WRITE: ConstStaticCell<[u8; minimeater_app::tls::WRITE_RECORD]> =
        ConstStaticCell::new([0; minimeater_app::tls::WRITE_RECORD]);
    let tls = minimeater_fw::tls::Client::new(
        minimeater_fw::tls::DeviceRng,
        rest.secure_element,
        TLS_READ.take(),
        TLS_WRITE.take(),
    );
    spawner.spawn(mqtt(tcp!(stack), tls, id, bus, MQTT_BUFS.take()).expect("mqtt task"));
    spawner.spawn(otlp(tcp!(stack), id, bus, OTLP_BUFS.take()).expect("otlp task"));
    static HTTP_BUFS: ConstStaticCell<minimeater_app::http::Buffers> =
        ConstStaticCell::new(minimeater_app::http::Buffers::new());
    spawner.spawn(http(tcp!(stack), id, bus, HTTP_BUFS.take()).expect("http task"));
    spawner.spawn(control(io.watchdog, io.ambient, store, settings, pit, foods, bus).expect("control task"));
    loop {
        core::future::pending::<()>().await;
    }
}

/// The application's tasks bound to the board. Their futures are large (the config store's 1.7 kB buffer, the
/// pit) and built once in `main`'s frame before they live in the task pool; the tasks take their peripherals as
/// arguments, as the twin's do.
mod run {
    #![allow(clippy::large_stack_frames, clippy::too_many_arguments)]

    use super::*;

    #[embassy_executor::task]
    pub async fn control(
        wdt: <B as Board>::Watchdog,
        ambient: <B as Board>::Ambient,
        store: ConfigStore<<B as Board>::Config>,
        settings: Settings,
        pit: &'static mut Pit,
        foods: &'static mut Foods,
        bus: &'static Bus,
    ) {
        tasks::control::<B>(wdt, ambient, store, settings, pit, foods, bus).await
    }

    #[embassy_executor::task]
    pub async fn probes(adc: <B as Board>::Probes, bus: &'static Bus) {
        tasks::probes::<B>(adc, bus).await
    }

    #[embassy_executor::task]
    pub async fn actuators(
        fan: <B as Board>::Fan,
        tach: <B as Board>::Tach,
        servo: <B as Board>::Servo,
        power: <B as Board>::ServoPower,
        fan_power: <B as Board>::FanPower,
        load_fault: <B as Board>::LoadFault,
        servo_current: <B as Board>::ServoCurrent,
        usb: <B as Board>::Usb,
        s_min: f64,
        rpm_max: f64,
        bus: &'static Bus,
    ) {
        tasks::actuators::<B>(
            fan,
            tach,
            servo,
            power,
            fan_power,
            load_fault,
            servo_current,
            usb,
            s_min,
            rpm_max,
            bus,
        )
        .await
    }

    // The fits (autotune, airflow map) take long: they belong on a low-priority executor once a target
    // run has measured them (budget: well under the 10 s control period); until then they share this one.
    #[embassy_executor::task]
    pub async fn slow(bus: &'static Bus) {
        tasks::slow(bus).await
    }
}
use run::{actuators, control, probes, slow};

/// A TCP connector with its own static buffers.
macro_rules! tcp {
    ($stack:expr) => {{
        static RX: ConstStaticCell<[u8; 2048]> = ConstStaticCell::new([0; 2048]);
        static TX: ConstStaticCell<[u8; 2048]> = ConstStaticCell::new([0; 2048]);
        Tcp::new($stack, RX.take(), TX.take())
    }};
}
use tcp;

/// `minimeater-` and the same three bytes: the DHCP and mDNS name (`minimeater-a1b2c3.local`).
fn hostname(id: &str) -> heapless::String<24> {
    let mut s = heapless::String::new();
    let _ = s.push_str("minimeater-");
    let _ = s.push_str(id.strip_prefix("mm-").unwrap_or(id));
    s
}

/// `mm-` and the low three bytes of the MAC, the id MQTT and Home Assistant know the device by.
fn device_id() -> heapless::String<12> {
    use core::fmt::Write as _;
    let mac = esp_hal::efuse::base_mac_address();
    let m = mac.as_bytes();
    let mut s = heapless::String::new();
    let _ = write!(s, "mm-{:02x}{:02x}{:02x}", m[3], m[4], m[5]);
    s
}

#[embassy_executor::task]
async fn provisioning(mut port: Port, id: &'static str, bus: &'static Bus) {
    let who = Identity {
        prefix: "minimeater",
        id,
        sw: env!("CARGO_PKG_VERSION"),
        hw: "A",
    };
    improv::run(&mut port, &mut Link, &who, bus).await
}

/// The secure element, for the REPL's `identity` commands.
#[embassy_executor::task]
async fn enrol(chip: &'static minimeater_fw::board::SecureElement, id: &'static str, bus: &'static Bus) {
    minimeater_app::enrol::serve(chip, bus, id).await
}

#[embassy_executor::task]
async fn mqtt(
    mut tcp: Tcp,
    mut tls: minimeater_fw::tls::Client,
    id: &'static str,
    bus: &'static Bus,
    bufs: &'static mut minimeater_app::mqtt::Buffers,
) {
    let who = Identity {
        prefix: "minimeater",
        id,
        sw: env!("CARGO_PKG_VERSION"),
        hw: "A",
    };
    minimeater_app::mqtt::run(&mut tcp, &mut tls, &who, bus, bufs, &net::rssi).await
}

#[embassy_executor::task]
async fn otlp(mut tcp: Tcp, id: &'static str, bus: &'static Bus, bufs: &'static mut minimeater_app::otlp::Buffers) {
    let who = Identity {
        prefix: "minimeater",
        id,
        sw: env!("CARGO_PKG_VERSION"),
        hw: "A",
    };
    let heap_free = || Some(esp_alloc::HEAP.free() as u32);
    minimeater_app::otlp::run(&mut tcp, &who, bus, bufs, &net::unix_ns, &net::rssi, &heap_free).await
}

#[embassy_executor::task]
async fn http(mut tcp: Tcp, id: &'static str, bus: &'static Bus, bufs: &'static mut minimeater_app::http::Buffers) {
    let who = Identity {
        prefix: "minimeater",
        id,
        sw: env!("CARGO_PKG_VERSION"),
        hw: "A",
    };
    minimeater_app::http::run(&mut tcp, &who, bus, &net::rssi, bufs).await
}

/// The `reboot` request (MQTT, Home Assistant's button, the serial REPL): the settings are saved as they change, so
/// the board comes back into the cook it was in.
#[embassy_executor::task]
async fn status_led(led: <B as Board>::Led, bus: &'static Bus) {
    ui::led(led, bus).await
}

#[embassy_executor::task]
async fn user_button(pin: <B as Board>::Button, bus: &'static Bus) {
    ui::button(pin, bus).await
}

#[embassy_executor::task]
async fn reboot(bus: &'static Bus) {
    bus.reboot.wait().await;
    info!("reboot requested");
    // the reply and the last status go out first
    Timer::after_millis(500).await;
    minimeater_fw::reset::reboot()
}

#[embassy_executor::task]
async fn heartbeat() {
    let mut tick = Ticker::every(Duration::from_secs(1));
    loop {
        info!("uptime {} s", Instant::now().as_secs());
        tick.next().await;
    }
}
