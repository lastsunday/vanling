//! The Espressif-family runtime layer: chip bring-up, logging, the RTOS entry point, the
//! radio heap, and the per-part panic handler.
//!
//! The only layer that names a chip. Above it `iot-bsp-esp` wires boards and `iot-app`
//! holds the product; below it there is nothing of ours.

#![no_std]

use esp_backtrace as _;

mod boot_guard;
mod heap;
pub(crate) mod logging;
#[cfg(feature = "radio")]
mod radio;
mod watchdog;

pub use boot_guard::{BootInfo, enter_safe_mode, note_boot, park_safe_mode};
pub use heap::{HeapError, PRODUCT_HEAP_BYTES, heap_free, init_heap, try_init_heap};
pub use watchdog::{arm_boot_watchdog, start_watchdog};

/// Restarts the part from a clean boot.
pub fn reset() -> ! {
    esp_hal::system::software_reset()
}

/// Restarts after a fault: the next boot counts this one as a crashed boot.
///
/// Safe mode suppresses the reset. If that path panics, rebooting only re-enters safe mode and
/// destroys the console evidence, so the boot parks where it can still be read.
pub fn crashed_reset() -> ! {
    if boot_guard::is_safe_mode() {
        boot_guard::park_forever("a fault in safe mode");
    }
    boot_guard::mark_crashed();
    reset()
}

/// The halt `esp-backtrace` calls after printing a panic, once `custom-halt` is on.
///
/// # Safety
///
/// Called by the panic handler with interrupts disabled; it must not return.
#[unsafe(no_mangle)]
pub unsafe extern "Rust" fn custom_halt() -> ! {
    crashed_reset()
}

#[cfg(feature = "radio")]
pub use radio::{init_radio_heap, radio_heap_free, radio_heap_total, ungate_radio};

/// Points `log` at [`logging::LOGGER`] and applies `IOT_LOG_LEVEL`.
pub fn init_logging() {
    log::set_logger(&logging::LOGGER).expect("logger already set");
    let level = logging::configured_level();
    log::set_max_level(level);
    log::info!("[IOT] log level {level} (IOT_LOG_LEVEL)");
}

/// Reports what the part reached rather than what was asked: a part with fewer presets than the
/// vocabulary logs the mismatch instead of silently running at another speed.
pub struct Clock;

impl iot_core::demand::ClockSink for Clock {
    type Table = Speeds;

    fn set_speed(&mut self, speed: iot_core::demand::Speed) -> iot_core::demand::Speed {
        let reached = set_speed(speed);
        if reached != speed {
            log::warn!("[CLOCK] asked for {speed:?}, this part reached {reached:?}");
        }
        log::info!("[CLOCK] running at {} MHz", clock_mhz(reached));
        reached
    }
}

mod chips;

#[cfg(feature = "esp32c6")]
use chips::esp32c6::clock_mhz;
#[cfg(feature = "esp32s3")]
use chips::esp32s3::clock_mhz;

#[cfg(feature = "esp32c6")]
pub use chips::esp32c6::{
    Speeds, chip_init, chip_init_for_product, set_speed, start_rtos, with_other_core_parked,
};

#[cfg(feature = "esp32s3")]
pub use chips::esp32s3::{
    Speeds, chip_init, chip_init_at, chip_init_for_product, panel_core_stack_peak, set_cpu_clock,
    set_speed, start_feed_executor, start_panel_core, start_rtos, start_second_core,
    with_other_core_parked,
};
