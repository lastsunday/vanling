//! Crash counting across resets, and the safe mode it leads to.
//!
//! The count lives in an RTC-domain scratch register (`store0`), which a core reset does
//! not clear — that is the kind of reset both the panic path and the watchdog use. A
//! power-on clears it, and that is also how a person gets out of safe mode.
//!
//! The rule for what a record means is in [`iot_core::boot`]; this module only supplies
//! the register, the reset reason, and the loop that keeps a parked part observable.

use esp_hal::delay::Delay;
use esp_hal::rtc_cntl::{SocResetReason, reset_reason};
use esp_hal::system::Cpu;
use iot_core::boot::{BootMode, ResetRecord, record_boot};

/// Set once this boot has decided to run safe mode. From then on a panic or an explicit
/// `crashed_reset` parks instead of resetting: the safe-mode page is the evidence, and a
/// reboot loop would destroy it.
static SAFE_MODE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

// The RTC-domain block that owns `store0`: the S3 calls it `LPWR`, the C6 `LP_AON`, and
// both spell the register the same way.
#[cfg(feature = "esp32c6")]
use esp_hal::peripherals::LP_AON as Aon;
#[cfg(feature = "esp32s3")]
use esp_hal::peripherals::LPWR as Aon;

/// What this boot is, for the caller that has to choose between the product and the park.
#[derive(Clone, Copy, Debug)]
pub struct BootInfo {
    pub mode: BootMode,
    pub record: ResetRecord,
    pub reason: Option<SocResetReason>,
}

impl BootInfo {
    pub const fn is_safe(&self) -> bool {
        matches!(self.mode, BootMode::Safe)
    }
}

/// Folds this boot into the record the reset left and writes it back.
///
/// Call after the chip is up and logging works, before anything that can hang or panic.
pub fn note_boot() -> BootInfo {
    let reason = reset_reason(Cpu::ProCpu);
    let previous = read();
    let (record, mode) = record_boot(previous, crashed(reason, previous));
    write(record);
    log::info!(
        "[BOOT] reset reason {reason:?}; {} crashed boot(s) in a row; {:?} start",
        record.crashed_boots(),
        mode,
    );
    BootInfo {
        mode,
        record,
        reason,
    }
}

/// Leaves the panic flag for the next boot. Called by the halt that resets from a panic.
pub fn mark_crashed() {
    write(read().panicked_after_crash());
}

/// Clears the record once this boot has proved itself: a frame was shown and the network
/// path settled (or is deliberately absent).
pub fn mark_healthy() {
    write(ResetRecord::EMPTY);
}

/// Latches safe mode for the rest of this boot. Call before anything that could panic.
pub fn enter_safe_mode() {
    SAFE_MODE.store(true, core::sync::atomic::Ordering::Release);
}

/// Whether this boot is safe mode. A panic after this point must not reset the part.
pub fn is_safe_mode() -> bool {
    SAFE_MODE.load(core::sync::atomic::Ordering::Acquire)
}

/// Stops the reset loop instead of feeding it: the part stays powered with the console up so
/// the cause can be read and the image replaced. Nothing here arms the watchdog.
pub fn park_forever(reason: &str) -> ! {
    log::error!("[BOOT] {reason}; no watchdog is armed and resets are suppressed");
    let delay = Delay::new();
    loop {
        delay.delay_millis(10_000);
        log::error!("[BOOT] still parked; press reset, reflash over USB, or power-cycle");
    }
}

/// Stops the product path in safe mode and keeps the serial console observable.
pub fn park_safe_mode(info: &BootInfo) -> ! {
    enter_safe_mode();
    log::error!(
        "[BOOT] safe mode after {} crashed boot(s) in a row (last reset: {:?}); the product \
         is not started",
        info.record.crashed_boots(),
        info.reason,
    );
    log::error!(
        "[BOOT] press reset, power-cycle, or reflash over USB. Safe mode suppresses resets"
    );
    park_forever("safe mode parked")
}

fn crashed(reason: Option<SocResetReason>, previous: ResetRecord) -> bool {
    previous.panicked() || is_watchdog(reason)
}

#[cfg(feature = "esp32s3")]
fn is_watchdog(reason: Option<SocResetReason>) -> bool {
    matches!(
        reason,
        Some(
            SocResetReason::CoreRtcWdt
                | SocResetReason::CpuRtcWdt
                | SocResetReason::SysRtcWdt
                | SocResetReason::CoreMwdt0
                | SocResetReason::CoreMwdt1
                | SocResetReason::CpuMwdt0
                | SocResetReason::CpuMwdt1
        )
    )
}

#[cfg(feature = "esp32c6")]
fn is_watchdog(reason: Option<SocResetReason>) -> bool {
    matches!(
        reason,
        Some(
            SocResetReason::CoreRtcWdt
                | SocResetReason::Cpu0RtcWdt
                | SocResetReason::SysRtcWdt
                | SocResetReason::CoreMwdt0
                | SocResetReason::CoreMwdt1
                | SocResetReason::Cpu0Mwdt0
                | SocResetReason::Cpu0Mwdt1
        )
    )
}

fn read() -> ResetRecord {
    ResetRecord::decode(Aon::regs().store0().read().bits())
}

fn write(record: ResetRecord) {
    Aon::regs()
        .store0()
        .write(|w| unsafe { w.bits(record.encode()) });
}
