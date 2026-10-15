//! Why the chip started (the `boot` event, `.4.31`) and the one reset the firmware asks for itself.
//!
//! The reset reason register tells a power-on from a watchdog or a software reset. A software reset is `user` when the
//! reboot task left its mark in RTC memory first (which survives the reset, not a power cut) and `panic` when not.
//! The panic handler does not reset: it parks with interrupts off, and the hardware watchdog does the resetting, so a
//! panic reads as `watchdog`.

use core::ptr::{addr_of_mut, read_volatile, write_volatile};

use esp_hal::rtc_cntl::SocResetReason;
use esp_hal::system::reset_reason;
use minimeater_app::proto::BootReason;

const REQUESTED: u32 = 0x4D4D_5253;

#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut MARK: u32 = 0;

/// The reason for this boot; clears the mark so that it counts once.
pub fn boot_reason() -> BootReason {
    // SAFETY: single-threaded start-up, before the tasks run; the word is valid for any bit pattern
    let marked = unsafe { read_volatile(addr_of_mut!(MARK)) == REQUESTED };
    unsafe { write_volatile(addr_of_mut!(MARK), 0) };
    match reset_reason() {
        Some(SocResetReason::CoreSw | SocResetReason::Cpu0Sw) if marked => BootReason::User,
        Some(SocResetReason::CoreSw | SocResetReason::Cpu0Sw) => BootReason::Panic,
        Some(
            SocResetReason::CoreMwdt0
            | SocResetReason::CoreMwdt1
            | SocResetReason::CoreRtcWdt
            | SocResetReason::Cpu0Mwdt0
            | SocResetReason::Cpu0Mwdt1
            | SocResetReason::Cpu0RtcWdt
            | SocResetReason::SysRtcWdt
            | SocResetReason::SysSuperWdt,
        ) => BootReason::Watchdog,
        _ => BootReason::PowerOn,
    }
}

/// Resets the chip, marking it as asked for.
pub fn reboot() -> ! {
    // SAFETY: as above
    unsafe { write_volatile(addr_of_mut!(MARK), REQUESTED) };
    esp_hal::system::software_reset()
}
