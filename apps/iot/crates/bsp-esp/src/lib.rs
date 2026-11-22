#![no_std]

extern crate alloc;

pub mod boards;

#[cfg(feature = "esp32c6-devkitc-1")]
pub use boards::esp32c6_devkitc_1::{Board, PullButton, SETUP_HOST, Startup, Ws2812RgbLed};

#[cfg(feature = "lckfb-szpi-esp32s3")]
pub use boards::lckfb_szpi_esp32s3::{
    Board, PanelBus, PullButton, SETUP_HOST, Screen, SharedI2cDevice, Startup, panel_service,
};

/// Board-agnostic real components: chip drivers parameterized over the bus / pin instances
/// handed in by the board wiring.
pub mod components;

pub mod hardware;

// Abstract components assembled from real ones. Each carries its own gate below, so a board
// wiring only the speaker still gets its transport. Every virtual component's feature has to
// appear here: a component left out compiles under a sibling's board and under none of its own,
// so `check-features` would never build it alone.
#[cfg(any(
    feature = "screen",
    feature = "audio",
    feature = "audio-out",
    feature = "camera"
))]
pub mod virtual_components;

#[cfg(feature = "screen")]
pub(crate) mod stack;
