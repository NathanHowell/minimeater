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
use minimeat_app::selftest;
use minimeat_core::control::{Mpc, Obs, Schedule};
use minimeat_fw::board;
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

    // Until the control loop task (.4.14) exists, run the MPC against an in-memory FOPDT egg with an
    // unmodelled -15F disturbance, so a flashed board reports the on-target solve time (budget 50 ms).
    let ff = |_t: f64| 1.0;
    let m = Schedule::default().at(225.0);
    let a = libm::exp(-10.0 / m.tau_s);
    let delay = libm::round(m.theta_s / 10.0) as usize;
    // ~2 KB with its dead-time buffer: keep it off the async stack.
    static MPC: StaticCell<Mpc> = StaticCell::new();
    let mpc = MPC.init_with(Mpc::default);
    let (mut pit, mut hist, mut k) = (150.0, [0.0f64; 32], 0usize);
    let mut tick = Ticker::every(Duration::from_secs(10));
    loop {
        let obs = Obs {
            pit_f: pit,
            sp_f: 225.0,
            lid: false,
            q_min: 0.0,
            q_max: 4.0,
        };
        let t0 = Instant::now();
        let q = mpc.step(&obs, &ff, 10.0);
        let us = t0.elapsed().as_micros();
        info!(
            "mpc: pit {pit:.1}F q {q:.3} CFM d {:.1}F, {} iters in {us} us",
            mpc.disturbance_f(),
            mpc.last_iters
        );
        hist[k % 32] = q;
        let q_delayed = hist[(k + 32 - delay) % 32];
        pit = a * pit + (1.0 - a) * (225.0 + m.k * (q_delayed - 1.0) - 15.0);
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
