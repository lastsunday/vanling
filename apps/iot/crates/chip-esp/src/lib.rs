//! The Espressif-family runtime layer: chip bring-up, logging, the RTOS entry point, the
//! radio heap, and the per-part panic handler.
//!
//! The only layer that names a chip. Above it `iot-bsp-esp` wires boards and `iot-app`
//! holds the product; below it there is nothing of ours.

#![no_std]

use esp_backtrace as _;

mod heap;
pub(crate) mod logging;
#[cfg(feature = "radio")]
mod radio;

pub use heap::{PRODUCT_HEAP_BYTES, heap_free, init_heap};

/// Restarts the part from a clean boot.
pub fn reset() -> ! {
    esp_hal::system::software_reset()
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
    type Table = Table;

    fn set_speed(&mut self, speed: iot_core::demand::Speed) -> iot_core::demand::Speed {
        let reached = set_speed(speed);
        if reached != speed {
            log::warn!("[CLOCK] asked for {speed:?}, this part reached {reached:?}");
        }
        log::info!("[CLOCK] running at {} MHz", clock_mhz(reached));
        reached
    }
}

#[cfg(feature = "esp32c6")]
use esp32c6::clock_mhz;
#[cfg(feature = "esp32s3")]
use esp32s3::clock_mhz;

#[cfg(feature = "esp32c6")]
pub(crate) mod esp32c6;

#[cfg(feature = "esp32c6")]
pub use esp32c6::{Table, chip_init, chip_init_for_product, set_speed, start_rtos};

#[cfg(feature = "esp32s3")]
pub(crate) mod esp32s3;

#[cfg(feature = "esp32s3")]
pub use esp32s3::{
    Table, chip_init, chip_init_at, chip_init_for_product, set_cpu_clock, set_speed,
    start_feed_executor, start_rtos, start_second_core,
};
