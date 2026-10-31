#![no_std]

extern crate alloc;

#[cfg(feature = "esp32c6-devkitc-1")]
pub mod esp32c6_devkitc_1;

#[cfg(feature = "esp32c6-devkitc-1")]
pub use esp32c6_devkitc_1::{Board, PullButton, SETUP_HOST, Startup, Ws2812RgbLed};

#[cfg(feature = "lckfb-szpi-esp32s3")]
pub mod lckfb_szpi_esp32s3;

#[cfg(feature = "lckfb-szpi-esp32s3")]
pub use lckfb_szpi_esp32s3::{Board, PullButton, SETUP_HOST, Screen, SharedI2cDevice, Startup};

/// Board-agnostic real components: chip drivers parameterized over the bus / pin instances
/// handed in by the board wiring.
pub mod components;

pub mod hardware;

// Abstract components assembled from real ones. Gated on whichever is present
// rather than on one of them: each virtual component carries its own gate
// below, so a board wiring only the speaker still gets its transport and a
// board wiring only the microphone still gets its own.
//
// Every virtual component's own feature has to appear here. A component left
// out compiles under every board that happens to enable a sibling and under
// none of its own, so `check-features` — which builds each feature alone —
// silently never builds it.
#[cfg(any(
    feature = "screen",
    feature = "audio",
    feature = "audio-out",
    feature = "camera"
))]
pub mod virtual_components;

#[cfg(feature = "screen")]
pub(crate) mod stack;
