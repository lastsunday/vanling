#[cfg(any(feature = "audio", feature = "audio-out"))]
pub mod reference;
#[cfg(any(feature = "audio", feature = "audio-out"))]
pub use reference::{Reference, Shared, Side, Silent, Writer, share};
#[cfg(feature = "audio")]
pub mod audio;
#[cfg(feature = "audio")]
pub use audio::{EchoCapture, EchoReport, Es7210Rx};
#[cfg(feature = "audio-out")]
pub mod audio_out;
#[cfg(feature = "audio-out")]
pub use audio_out::{Es8311Tx, REFERENCE_SAMPLES};
#[cfg(feature = "screen")]
pub mod screen;
#[cfg(feature = "screen")]
pub use screen::Screen;

#[cfg(feature = "camera")]
pub mod camera;
