#![no_std]

extern crate alloc;

#[cfg(feature = "esp32c6-devkitc-1")]
pub mod esp32c6_devkitc_1;

#[cfg(feature = "esp32c6-devkitc-1")]
pub use esp32c6_devkitc_1::{Board, PullButton, Ws2812RgbLed};

#[cfg(feature = "lckfb-szpi-esp32s3")]
pub mod lckfb_szpi_esp32s3;

#[cfg(feature = "lckfb-szpi-esp32s3")]
pub use lckfb_szpi_esp32s3::{Board, DisplayLight, PullButton};

/// Board-agnostic real components: chip drivers parameterized over the bus /
/// pin instances handed in by the board wiring.
pub mod components;

/// Abstract components assembled from real ones. Gated on whichever of the
/// three is present rather than on one of them: each virtual component carries
/// its own gate below, so a board wiring only the speaker still gets its
/// transport and a board wiring only the microphone still gets its own.
#[cfg(any(feature = "display-light", feature = "audio", feature = "audio-out"))]
pub mod virtual_components;
