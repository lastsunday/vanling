#![no_std]

use esp_backtrace as _;

pub(crate) mod logging;

#[cfg(feature = "esp32c6")]
pub(crate) mod esp32c6;

#[cfg(feature = "esp32c6")]
pub use esp32c6::{chip_init, init_logging, start_rtos};

#[cfg(feature = "esp32s3")]
pub(crate) mod esp32s3;

#[cfg(feature = "esp32s3")]
pub use esp32s3::{chip_init, init_logging, start_rtos};
