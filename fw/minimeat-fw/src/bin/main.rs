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
use minimeat_app::config::{ConfigStore, Mode};
use minimeat_app::hal::Board;
use minimeat_app::pit::{PERIOD_S, Pit};
use minimeat_app::selftest;
use minimeat_core::control::Schedule;
use minimeat_core::model::Ambient;
use minimeat_core::model::airflow::AirPath;
use minimeat_fw::board::{self, Esp};
use static_cell::StaticCell;

// App descriptor required by the ESP-IDF second-stage bootloader.
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "main holds the peripherals and demo state for the device's lifetime"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    esp_println::logger::init_logger_from_env();
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    let (mut io, rest) = board::init(peripherals);

    let timg0 = TimerGroup::new(rest.timg0);
    esp_rtos::start(timg0.timer0, rest.sw_interrupt);
    info!("minimeat-fw {} up", env!("CARGO_PKG_VERSION"));

    spawner.spawn(heartbeat().expect("heartbeat task"));
    selftest::survey(&mut io).await;

    // Settings, the learned model and the cook in progress: a power cut resumes from here.
    static STORE: StaticCell<ConfigStore<<Esp as Board>::Config>> = StaticCell::new();
    let (store, settings) = ConfigStore::open(io.config).await;
    let _store = STORE.init(store);
    info!(
        "config: {:?} at {}F ({:?}), autotune {}, {} learned models, wifi {}",
        settings.cook.mode,
        settings.cook.sp_f,
        settings.cook.controller,
        settings.cook.autotune,
        settings.schedule.models().len(),
        settings.wifi.is_some()
    );

    // Until the probe pipeline (.4.7) and actuator drivers (.4.8) exist, run the control loop
    // (.4.14) against an in-memory FOPDT egg that runs 15F colder than the model, so a flashed board
    // reports what one period and one autotune fit cost on target (budget: well under the 10 s
    // period; the fit belongs on a low-priority executor once it is measured).
    let ambient = Ambient::default();
    let mut settings = settings;
    settings.cook.mode = Mode::Run;
    static PIT: StaticCell<Pit> = StaticCell::new();
    let pit = PIT.init_with(|| Pit::new(&settings, AirPath::default(), ambient));
    let m = Schedule::default().at(settings.cook.sp_f);
    let a = libm::exp(-PERIOD_S / m.tau_s);
    let delay = libm::round(m.theta_s / PERIOD_S) as usize;
    let q0 = AirPath::default().feedforward_cfm(&ambient, settings.cook.sp_f);
    let (mut y, mut hist, mut k) = (150.0, [0.0f64; 32], 0usize);
    let mut tick = Ticker::every(Duration::from_secs(10));
    loop {
        let t0 = Instant::now();
        let (out, events) = pit.step(Some(y), ambient, PERIOD_S);
        let us = t0.elapsed().as_micros();
        info!(
            "pit: {y:.1}F {:?} ref {:.1} q {:.3} CFM flap {:.0}% fan {:.0}% in {us} us, events {events:?}",
            out.phase,
            out.ref_f,
            out.q,
            out.flap_pct,
            out.act.fan * 100.0
        );
        if let Some((due, id)) = pit.take_fit() {
            let t0 = Instant::now();
            let fit = id.fit();
            info!("autotune fit {fit:?} in {} ms", t0.elapsed().as_millis());
            pit.fitted(due, fit);
        }
        hist[k % 32] = out.q;
        let q_delayed = hist[(k + 32 - delay) % 32];
        y = a * y + (1.0 - a) * (settings.cook.sp_f + m.k * (q_delayed - q0) - 15.0);
        k += 1;
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
