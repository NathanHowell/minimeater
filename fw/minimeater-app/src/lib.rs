//! minimeater application: everything above the peripheral boundary. Generic over a [`hal::Board`], so
//! the ESP32-C6 binary (`minimeater-fw`) and the host twin (`minimeater-sim`, `.4.16.2`) run the same
//! code; this crate never touches esp-hal and has no target `cfg`s.
#![cfg_attr(not(test), no_std)]

pub mod alarms;
pub mod ambient;
pub mod config;
pub mod drive;
pub mod drivers;
pub mod enrol;
pub mod food;
pub mod hal;
pub mod http;
pub mod improv;
pub mod mdns;
pub mod mqtt;
pub mod net;
pub mod otlp;
pub mod pit;
pub mod probes;
pub mod proto;
pub mod repl;
pub mod selftest;
pub mod serial;
pub mod sntp;
pub mod tasks;
pub mod tls;
pub mod ui;

/// The host tests' clock: every look at it is a millisecond later, and a timer that is set wakes at once, so a timeout
/// in code under test runs out in a few thousand polls instead of waiting.
#[cfg(test)]
mod test_clock {
    use core::sync::atomic::{AtomicU64, Ordering};

    struct Clock(AtomicU64);

    impl embassy_time_driver::Driver for Clock {
        fn now(&self) -> u64 {
            self.0.fetch_add(embassy_time_driver::TICK_HZ / 1000, Ordering::Relaxed)
        }

        fn schedule_wake(&self, _at: u64, waker: &core::task::Waker) {
            waker.wake_by_ref();
        }
    }

    embassy_time_driver::time_driver_impl!(static CLOCK: Clock = Clock(AtomicU64::new(0)));
}
