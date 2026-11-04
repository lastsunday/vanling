//! Turning readings into glyphs: the arithmetic every page agrees on.

mod number;

pub use self::number::*;

/// Bytes between fingerprint samples.
pub const FINGERPRINT_STRIDE: usize = 997;

/// An FNV fold over a sparse sample of the frame, kept to sixteen bits.
pub fn frame_fingerprint(frame: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for &byte in frame.iter().step_by(FINGERPRINT_STRIDE) {
        hash = (hash ^ byte as u32).wrapping_mul(0x0100_0193);
    }
    hash >> 16
}
