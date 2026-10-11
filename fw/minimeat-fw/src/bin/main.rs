#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe with esp_hal types, especially those holding DMA buffers"
)]
#![deny(clippy::large_stack_frames)]

use embassy_executor::Spawner;
use embassy_time::{Duration, Instant, Ticker};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;
use log::info;
use minimeat_core::thermistor::{Preset, R_BIAS, r_from_ratio};
use minimeat_core::units::k_to_f;

// App descriptor required by the ESP-IDF second-stage bootloader.
esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    esp_println::logger::init_logger_from_env();
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);
    info!("minimeat-fw {} up", env!("CARGO_PKG_VERSION"));

    spawner.spawn(heartbeat().expect("heartbeat task"));

    // Placeholder control period until the control loop task (.4.14) exists: exercise the core's
    // probe conversion on a fixed ratio so a flashed board proves core links and runs on target.
    let mut tick = Ticker::every(Duration::from_secs(10));
    loop {
        let t0 = Instant::now();
        let t_f = k_to_f(Preset::ThermoworksPro.thermistor().t_k(r_from_ratio(0.0955546, R_BIAS)));
        info!(
            "control tick: x=0.0955546 -> {t_f:.2}F in {} us",
            t0.elapsed().as_micros()
        );
        tick.next().await;
    }
}

#[embassy_executor::task]
async fn heartbeat() {
    let mut tick = Ticker::every(Duration::from_secs(1));
    loop {
        info!("uptime {} s", Instant::now().as_secs());
        tick.next().await;
    }
}
