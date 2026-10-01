//! The one clock a board's capture and playback halves are both programmed from.
//!
//! Both codecs are slaves of one host MCLK. A board running them from separately
//! declared clocks could sit a few hertz apart with nothing reporting it — the panel
//! would meter correctly and the speaker would be sharp, which sounds like hardware.

/// The rate both halves run at, declared once in the core layer.
pub use iot_core::drivers::audio::SAMPLE_RATE_HZ;

/// A board-wiring fact, not a codec one: the part does not generate it.
pub const MCLK_HZ: u32 = 12_288_000;

/// Pins the divider to the rate the panel counts against, so retuning one without the
/// other fails here rather than leaving a capture and a speaker a few hertz apart.
const _: () = assert!(MCLK_HZ / 256 == SAMPLE_RATE_HZ);
