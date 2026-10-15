#![no_std]

use esp_backtrace as _;

/// Where a general-purpose heap comes from. Not gated: every image has a runtime, and the
/// part it runs on is the one that says whether there is memory to point it at.
mod heap;
pub(crate) mod logging;
#[cfg(feature = "radio")]
mod radio;

pub use heap::init_heap;

#[cfg(feature = "radio")]
pub use radio::{init_radio_heap, radio_heap_free, radio_heap_total, ungate_radio};

/// The one sink that routes a demand's speed to the part this build targets.
///
/// [`set_speed`] reports what the part reached rather than what was asked, so a part
/// with fewer presets than the vocabulary has shows up in the log here instead of
/// silently running something else.
pub struct Part;

impl iot_core::demand::ClockSink for Part {
    type Table = Table;

    fn set_speed(&mut self, speed: iot_core::demand::Speed) -> iot_core::demand::Speed {
        let reached = set_speed(speed);
        if reached != speed {
            log::warn!("[CLOCK] asked for {speed:?}; this part reached {reached:?}");
        }
        reached
    }
}

#[cfg(feature = "esp32c6")]
pub(crate) mod esp32c6;

#[cfg(feature = "esp32c6")]
pub use esp32c6::{Table, chip_init, init_logging, set_speed, start_rtos};

#[cfg(feature = "esp32s3")]
pub(crate) mod esp32s3;

#[cfg(feature = "esp32s3")]
pub use esp32s3::{
    Table, chip_init, chip_init_at, init_logging, set_cpu_clock, set_speed, start_feed_executor,
    start_rtos,
};
