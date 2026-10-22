#![no_std]

extern crate alloc;

#[cfg(feature = "esp32c6-devkitc-1")]
pub mod esp32c6_devkitc_1;

#[cfg(feature = "esp32c6-devkitc-1")]
pub use esp32c6_devkitc_1::{Board, PullButton, Ws2812RgbLed};

#[cfg(feature = "lckfb-szpi-esp32s3")]
pub mod lckfb_szpi_esp32s3;

#[cfg(feature = "lckfb-szpi-esp32s3")]
pub use lckfb_szpi_esp32s3::{Board, DisplayLight, PullButton, SharedI2cDevice};

/// Board-agnostic real components: chip drivers parameterized over the bus /
/// pin instances handed in by the board wiring.
pub mod components;

/// The camera page's readout text: formatting over glyph buffers and frame bytes, with no
/// peripheral in it.
///
/// Its own module rather than part of the panel surface because it can then be built and tested
/// without a chip, and the arithmetic is where the bugs were — a reading wider than its buffer
/// indexes past the end, and only a test finds that before hardware does.
///
/// The panel's own coordinate formatting came out with it, because it is the same arithmetic
/// and the panel is the one surface that draws it — which is why `display-light` depends on
/// this module rather than the other way round, and why the module is unconditional: every
/// surface draws coordinates, and a gate here would leave `display-light` naming a function
/// that only some features define.
pub mod camera_readout;

/// The panel's glyph table, and the shapes a five-column cell has to keep apart.
///
/// Its own module because it is plain bytes with no peripheral in it: the host tests can hold
/// the table to "every glyph draws something" and "the look-alike pairs differ", which is the
/// only check a table swap gets before the pixels do. Gated on the surface that draws it, apart
/// from the tests, so a board with no panel does not carry the table.
#[cfg(any(feature = "display-light", test))]
pub mod font;

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
    feature = "display-light",
    feature = "audio",
    feature = "audio-out",
    feature = "camera"
))]
pub mod virtual_components;
