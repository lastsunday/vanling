//! One signal for every test in this module, because they all need the same property:
//! **broadband and aperiodic**.

use alloc::vec::Vec;

/// A deterministic noise source.
pub struct Noise(u32);

impl Noise {
    /// The xorshift below is stuck at zero forever if it starts there, and zero is the one
    /// seed a caller is likely to write — so that one is substituted.
    pub fn new(seed: u32) -> Self {
        Self(if seed == 0 { 0x9e37_79b9 } else { seed })
    }

    /// The next sample, uniform in `[−amplitude, amplitude)`.
    pub fn next(&mut self, amplitude: i32) -> i16 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        ((self.0 >> 8) as i64 * 2 * i64::from(amplitude) / (1 << 24) - i64::from(amplitude)) as i16
    }
}

/// `samples` of noise.
pub fn reference(samples: usize, amplitude: i32, seed: u32) -> Vec<i16> {
    let mut noise = Noise::new(seed);
    (0..samples).map(|_| noise.next(amplitude)).collect()
}

/// The microphone signal a single-tap echo path produces: the reference delayed by `delay`
/// and scaled by `ppm` parts per million.
pub fn echo(reference: &[i16], delay: usize, ppm: i32) -> Vec<i16> {
    (0..reference.len())
        .map(|n| {
            (if n >= delay {
                i64::from(reference[n - delay]) * i64::from(ppm) / 1_000_000
            } else {
                0
            }) as i16
        })
        .collect()
}

/// The sum of squares of a stretch, the unit every cancellation reading here is compared
/// in.
pub fn power(samples: &[i16]) -> i64 {
    samples.iter().map(|s| i64::from(*s) * i64::from(*s)).sum()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbouring_seeds_do_not_collapse_onto_each_other() {
        assert_ne!(
            reference(64, 4_000, 2),
            reference(64, 4_000, 3),
            "seeds 2 and 3 produced the same samples"
        );
        assert_ne!(
            reference(64, 4_000, 1),
            reference(64, 4_000, 0),
            "seed 0 is substituted, but only for zero"
        );
    }

    #[test]
    fn the_same_seed_reproduces_the_same_samples() {
        assert_eq!(reference(128, 4_000, 0xabc), reference(128, 4_000, 0xabc));
    }
}
