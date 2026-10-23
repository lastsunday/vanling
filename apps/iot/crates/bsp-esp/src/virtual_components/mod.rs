// The reference is what the two audio halves share, so it is gated on *either*:
// `audio` alone is a capture with no speaker and uses `Side::Silent`, and
// `audio-out` alone is a speaker nothing listens to. Gating it on both would fail
// features.md's rule that every subset stands on its own — the capture would be
// unable to build without a speaker it does not require.
#[cfg(any(feature = "audio", feature = "audio-out"))]
pub mod reference;
#[cfg(any(feature = "audio", feature = "audio-out"))]
pub use reference::{Reference, Shared, Side, Silent, Writer, share};
#[cfg(feature = "audio")]
pub mod audio;
#[cfg(feature = "audio")]
pub use audio::{EchoCapture, EchoReport, Es7210Rx};
// The transmit side states the shared log's size, because it is the side whose
// ring the log has to span; the capture takes it as a const parameter rather than
// reading it, so `audio` still builds on its own.
#[cfg(feature = "audio-out")]
pub mod audio_out;
#[cfg(feature = "audio-out")]
pub use audio_out::{Es8311Tx, REFERENCE_SAMPLES, SWEEP_MS};
#[cfg(feature = "screen")]
pub mod screen;
#[cfg(feature = "screen")]
pub use screen::Screen;

#[cfg(feature = "camera")]
pub mod camera;
