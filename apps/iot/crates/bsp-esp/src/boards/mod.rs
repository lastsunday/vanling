//! The boards this family wires, one module per product or devkit.

#[cfg(feature = "esp32c6-devkitc-1")]
pub mod esp32c6_devkitc_1;

#[cfg(feature = "lckfb-szpi-esp32s3")]
pub mod lckfb_szpi_esp32s3;
