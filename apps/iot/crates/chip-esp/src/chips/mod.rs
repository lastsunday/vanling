//! The parts this family's runtime knows, one module per part.

#[cfg(feature = "esp32c6")]
pub(crate) mod esp32c6;

#[cfg(feature = "esp32s3")]
pub(crate) mod esp32s3;
