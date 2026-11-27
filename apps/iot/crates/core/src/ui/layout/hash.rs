//! One hash for every fingerprint a frame takes: a view type's `Hash` impl is its state.

use core::hash::{Hash, Hasher};

/// 32-bit FNV-1a. The trait's `finish` is 64-bit, so the value is widened rather than rehashed.
#[derive(Clone, Copy)]
pub struct Fnv(u32);

impl Fnv {
    pub const fn new() -> Self {
        Self(0x811C_9DC5)
    }

    /// Continues a chain another fingerprint left off.
    pub const fn seed(seed: u32) -> Self {
        Self(seed)
    }
}

impl Default for Fnv {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher for Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0 ^ u32::from(byte)).wrapping_mul(0x0100_0193);
        }
    }

    fn finish(&self) -> u64 {
        u64::from(self.0)
    }
}

/// The fingerprint of `value`. `Hash` decides what counts: a view leaves out the fields that
/// animate it rather than describe its pixels.
pub fn hash_of(value: &impl Hash) -> u32 {
    let mut hasher = Fnv::new();
    value.hash(&mut hasher);
    hasher.finish() as u32
}
