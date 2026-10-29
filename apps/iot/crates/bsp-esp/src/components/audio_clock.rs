//! The one clock a board's capture and playback halves are both programmed from.

/// The rate both halves run at, declared once in the core layer.
pub use iot_core::drivers::audio::SAMPLE_RATE_HZ;

/// A board-wiring fact, not a codec one: the part does not generate it.
pub const MCLK_HZ: u32 = 12_288_000;

const _: () = assert!(MCLK_HZ / 256 == SAMPLE_RATE_HZ);
