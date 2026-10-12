//! One signal for every test in this module, because they all need the same
//! property: **broadband and aperiodic**.
//!
//! A periodic waveform gives an adaptive filter more than one correct answer, so
//! a test built on one passes without the algorithm working. A sawtooth is barely
//! aperiodic at the sample level — `x[n]` and `x[n−1]` are almost the same
//! number, the delay search's peak comes out nearly flat across neighbouring lags,
//! and the filter can put its gain a tap off and still predict the signal.

use alloc::vec::Vec;

/// A deterministic noise source. The seed is part of the call so two tests do
/// not share a sequence and so a failure names the signal that produced it.
pub struct Noise(u32);

impl Noise {
    /// The xorshift below is stuck at zero forever if it starts there, and zero is
    /// the one seed a caller is likely to write — so that one is substituted.
    /// Nothing else is touched: an earlier `seed | 1` folded every even seed onto
    /// the odd one above it, and two "independent" sources so constructed were
    /// the same signal, which is a silent way for a coherence test to measure 1.
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

/// The microphone signal a single-tap echo path produces: the reference delayed
/// by `delay` and scaled by `ppm` parts per million.
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

/// Two seeds give two signals. The failure this guards is quiet and total: a
/// constructor that collapses neighbouring seeds makes an "independent" source
/// identical to the one beside it, and every correlation built on the pair comes
/// out at 1.
pub fn distinct_seeds_give_distinct_signals() {
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

/// The sum of squares of a stretch, the unit every cancellation reading here is
/// compared in.
pub fn power(samples: &[i16]) -> i64 {
    samples.iter().map(|s| i64::from(*s) * i64::from(*s)).sum()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbouring_seeds_do_not_collapse_onto_each_other() {
        distinct_seeds_give_distinct_signals();
    }

    #[test]
    fn the_same_seed_reproduces_the_same_samples() {
        assert_eq!(reference(128, 4_000, 0xabc), reference(128, 4_000, 0xabc));
    }
}
