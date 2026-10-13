//! The twin's embassy time driver: a virtual clock that only the twin's main loop moves. Between
//! timer wakes the application is idle, so the main loop can step the plant up to the next wake
//! and jump the clock there, as fast as the host allows or paced to a multiple of real time.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use core::task::Waker;
use std::sync::Mutex;

use embassy_time_driver::Driver;
use embassy_time_queue_utils::Queue;

pub use embassy_time_driver::TICK_HZ;

pub struct Clock {
    now: AtomicU64,
    queue: Mutex<Queue>,
}

impl Driver for Clock {
    fn now(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }

    fn schedule_wake(&self, at: u64, waker: &Waker) {
        self.queue.lock().expect("timer queue").schedule_wake(at, waker);
    }
}

embassy_time_driver::time_driver_impl!(static CLOCK: Clock = Clock {
    now: AtomicU64::new(0),
    queue: Mutex::new(Queue::new()),
});

/// Set by the executor's pender: some task was woken and the executor should be polled.
static PENDING: AtomicBool = AtomicBool::new(false);

/// The thread that runs the twin's main loop: waking a task from another thread (a socket bridge)
/// also wakes it from its pacing sleep.
static MAIN: std::sync::OnceLock<std::thread::Thread> = std::sync::OnceLock::new();

pub fn register_main_thread() {
    let _ = MAIN.set(std::thread::current());
}

#[unsafe(export_name = "__pender")]
fn pender(_context: *mut ()) {
    PENDING.store(true, Ordering::SeqCst);
    if let Some(t) = MAIN.get() {
        t.unpark();
    }
}

/// True if some task has been woken since the last [`take_pending`].
pub fn pending() -> bool {
    PENDING.load(Ordering::SeqCst)
}

/// Clears and returns the pending flag.
pub fn take_pending() -> bool {
    PENDING.swap(false, Ordering::SeqCst)
}

pub fn now() -> u64 {
    CLOCK.now()
}

/// Wakes the timers that are due now; returns the next wake, `u64::MAX` if none.
pub fn expire() -> u64 {
    let now = CLOCK.now();
    CLOCK.queue.lock().expect("timer queue").next_expiration(now)
}

/// Moves the clock forward (never back).
pub fn set(t: u64) {
    CLOCK.now.fetch_max(t, Ordering::SeqCst);
}
