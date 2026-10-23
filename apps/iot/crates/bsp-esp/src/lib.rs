#![no_std]

extern crate alloc;

#[cfg(feature = "esp32c6-devkitc-1")]
pub mod esp32c6_devkitc_1;

#[cfg(feature = "esp32c6-devkitc-1")]
pub use esp32c6_devkitc_1::{Board, PullButton, Ws2812RgbLed};

#[cfg(feature = "lckfb-szpi-esp32s3")]
pub mod lckfb_szpi_esp32s3;

#[cfg(feature = "lckfb-szpi-esp32s3")]
pub use lckfb_szpi_esp32s3::{Board, PullButton, Screen, SharedI2cDevice};

/// Board-agnostic real components: chip drivers parameterized over the bus /
/// pin instances handed in by the board wiring.
pub mod components;

/// Who this board is, and what came up on it — a boot inventory, printed at the
/// end of a board's bring-up.
///
/// Ungated, unlike the board modules around it: only [`post::identity`] needs a
/// chip and that one function carries its own gate, so the states and the
/// reporting can be built and tested on a host.
pub mod post;

/// The sound catalogue: which MIDI file each sound is.
///
/// Its own module, ungated like [`post`], because the files are plain bytes and
/// the speaker driver that plays them needs a chip to exist. Reading them here
/// is what lets a sound file that would not parse be caught by the host tests
/// rather than by the first tap on hardware.
pub mod sounds;

/// Abstract components assembled from real ones. Gated on whichever is present
/// rather than on one of them: each virtual component carries its own gate
/// below, so a board wiring only the speaker still gets its transport and a
/// board wiring only the microphone still gets its own.
///
/// Every virtual component's own feature has to appear here. A component left
/// out compiles under every board that happens to enable a sibling and under
/// none of its own, so `check-features` — which builds each feature alone —
/// silently never builds it.
#[cfg(any(
    feature = "screen",
    feature = "audio",
    feature = "audio-out",
    feature = "camera"
))]
pub mod virtual_components;

/// The panel's stack probe, shared by every paint path that can reach deep enough to matter.
#[cfg(feature = "screen")]
pub(crate) mod stack;
