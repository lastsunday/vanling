//! Smoothed power, and the ratio that reads as "how much echo is left".
//!
//! Every quantity here is a mean square in LSB², so the ratios between them are
//! dimensionless and the decibels come out of a single integer logarithm — the
//! same accounting the panel's `PK`/`RMS` rows already use, at a far slower
//! ballistics.

/// How many bits the exponential average shifts toward the newest square per
/// sample. At [`SAMPLE_RATE_HZ`](super::SAMPLE_RATE_HZ) this is a ~30 ms time
/// constant: long enough that one talker syllable does not move the reading,
/// short enough that a filter converging over a few hundred milliseconds is
/// followed rather than averaged flat.
const AVERAGE_SHIFT: u32 = 9;

/// Fractional bits an integer `log2` is carried at.
const LOG_FRACTION: u32 = 16;

/// Ceiling on the reported ratio, in decibels. Beyond this the residual is
/// numerically zero and the honest answer is "gone", not an infinity the
/// caller has to know how to render.
pub const MAX_DECIBELS: i32 = 90;

/// Milli-decibels one doubling of a power ratio is worth: 10·log₁₀2, which is
/// what the decibel is *defined* as.
const MILLIDECIBELS_PER_OCTAVE: i64 = 3_010;

/// `log2` of a value of at least one, in Q[`LOG_FRACTION`].
///
/// Digit-at-a-time by repeated squaring: squaring the mantissa doubles its
/// exponent, so a mantissa that passes two has spent a binary digit. Sixteen
/// rounds cost thirty-two multiplies and land inside 0.003 dB — finer than the
/// whole decibel the caller rounds to, and with no table to get wrong.
fn log2_q16(value: u64) -> u32 {
    debug_assert!(value > 0);
    let integer = 63 - value.leading_zeros();
    let mut mantissa = (value << value.leading_zeros()) >> (64 - LOG_FRACTION - 1);
    let mut fraction = 0u32;
    for bit in 0..LOG_FRACTION {
        let squared = (mantissa * mantissa) >> LOG_FRACTION;
        if squared >= 2 << LOG_FRACTION {
            mantissa = squared >> 1;
            fraction |= 1 << (LOG_FRACTION - 1 - bit);
        } else {
            mantissa = squared;
        }
    }
    (integer << LOG_FRACTION) + fraction
}

/// A continuously running mean square, held in LSB².
///
/// The update is a shift rather than a divide so it costs a handful of cycles
/// per sample: at this rate a division would be the most expensive thing in the
/// loop, and the difference between one shift and `>> 9` is the difference
/// between that and nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerMeter {
    avg_sq: i64,
}

impl PowerMeter {
    pub const fn new() -> Self {
        Self { avg_sq: 0 }
    }

    pub fn update(&mut self, sample: i16) {
        let square = i64::from(sample) * i64::from(sample);
        let delta = square - self.avg_sq;
        // Once the difference is under 2⁹ the shift truncates to zero and the
        // meter stops wherever it was, half a kiloblock short of the target.
        // For a level of a few hundred that is most of the reading — a residual
        // settling at 389 against a true 900 — so the last step is taken whole,
        // the same escape `released_peak` gives the envelope's bars.
        //
        // Not `max`, which picks the *larger* value and would therefore hold a
        // falling meter at one LSB per sample instead of 2⁹. The floor applies
        // only where the shift produced nothing at all.
        let mut step = delta >> AVERAGE_SHIFT;
        if step == 0 {
            step = match delta.cmp(&0) {
                core::cmp::Ordering::Greater => 1,
                core::cmp::Ordering::Less => -1,
                core::cmp::Ordering::Equal => 0,
            };
        }
        self.avg_sq += step;
    }

    /// The level currently held, as a mean square in LSB². Never negative: the
    /// update moves toward a non-negative target from a non-negative state, and
    /// the arithmetic shift rounds toward negative infinity on the way down,
    /// which can overshoot by at most one and is corrected here rather than
    /// allowed to make the ratio below read backwards.
    pub const fn level(&self) -> i64 {
        if self.avg_sq < 0 { 0 } else { self.avg_sq }
    }
}

impl Default for PowerMeter {
    fn default() -> Self {
        Self::new()
    }
}

/// A continuously running mean of a signed quantity, at the same shift as
/// [`PowerMeter`] and for the same reason: the truncation floor is what lets it
/// reach its target rather than stopping half a step short of it, and on a signed
/// value that matters twice over — a mean that cannot cross zero is not a mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedMeter {
    avg: i64,
}

impl SignedMeter {
    pub const fn new() -> Self {
        Self { avg: 0 }
    }

    pub fn update(&mut self, value: i64) {
        let delta = value - self.avg;
        let mut step = delta >> AVERAGE_SHIFT;
        if step == 0 {
            step = match delta.cmp(&0) {
                core::cmp::Ordering::Greater => 1,
                core::cmp::Ordering::Less => -1,
                core::cmp::Ordering::Equal => 0,
            };
        }
        self.avg += step;
    }

    pub const fn mean(&self) -> i64 {
        self.avg
    }
}

impl Default for SignedMeter {
    fn default() -> Self {
        Self::new()
    }
}

/// Reads the microphone against its own echo-free residual.
///
/// The figure is only meaningful where the far end is speaking, so this does
/// not decide which samples count — the caller feeds it what it wants measured
/// and the gating is a property of the room, not of the ratio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErleMeter {
    mic: PowerMeter,
    residual: PowerMeter,
}

impl ErleMeter {
    pub const fn new() -> Self {
        Self {
            mic: PowerMeter::new(),
            residual: PowerMeter::new(),
        }
    }

    /// Folds one sample of each. `mic` is what the microphone heard, `residual`
    /// what is left of it after the echo estimate was subtracted; during far-end
    /// single talk the difference between the two is the cancellation.
    pub fn update(&mut self, mic: i16, residual: i16) {
        self.mic.update(mic);
        self.residual.update(residual);
    }

    /// Echo return loss enhancement: how far the residual sits below the
    /// microphone, in decibels, never above [`MAX_DECIBELS`].
    ///
    /// The floor of one LSB² keeps a converged filter on a silent passage from
    /// dividing by zero. Whether a segment had any far end in it at all is
    /// answered by what the caller fed in, not by this number.
    pub fn decibels(&self) -> i32 {
        let residual = self.residual.level().max(1) as u64;
        let mic = self.mic.level().max(1) as u64;
        // Plain integer division rather than a Q-scaled ratio: scaling the
        // numerator by 2¹⁶ would come back out of the logarithm as sixteen
        // extra octaves, and the truncation costs under half a decibel.
        let octave = log2_q16((mic / residual).max(1)) as i64;
        let milli = octave * MILLIDECIBELS_PER_OCTAVE / (1 << LOG_FRACTION);
        ((milli + 500) / 1000).clamp(0, MAX_DECIBELS as i64) as i32
    }
}

impl Default for ErleMeter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log2_reads_exact_powers_of_two() {
        for power in 0..24u32 {
            assert_eq!(
                log2_q16(1u64 << power),
                power << LOG_FRACTION,
                "2^{power} has no fractional part to get wrong"
            );
        }
    }

    #[test]
    fn log2_is_within_a_thousandth_of_a_decade_fraction() {
        // The hardest place for a digit-at-a-time logarithm is either side of
        // where the mantissa crosses sqrt(2), which is 1.5 in the mantissa and
        // 3 in the value.
        let whole = log2_q16(2) as f64 / (1 << LOG_FRACTION) as f64;
        assert!((whole - 1.0).abs() < 0.001, "log2(2) came out {whole}");
        let fractional = log2_q16(3) as f64 / (1 << LOG_FRACTION) as f64;
        assert!(
            (fractional - 1.584_963).abs() < 0.001,
            "log2(3) came out {fractional}"
        );
    }

    #[test]
    fn a_meter_starts_silent_and_claims_nothing() {
        assert_eq!(PowerMeter::new().level(), 0);
        assert_eq!(
            ErleMeter::new().decibels(),
            0,
            "an untouched meter has measured nothing, so it has cancelled nothing"
        );
    }

    #[test]
    fn a_steady_signal_converges_to_its_own_square() {
        let mut meter = PowerMeter::new();
        for _ in 0..8_000 {
            meter.update(1_000);
        }
        assert_eq!(
            meter.level(),
            1_000_000,
            "a 1000 LSB signal is 1e6 LSB^2 and the meter has to reach it, not stop \
             half a kiloblock short of it"
        );
    }

    #[test]
    fn silence_decays_the_meter_back_to_nothing() {
        let mut meter = PowerMeter::new();
        for _ in 0..4_000 {
            meter.update(2_000);
        }
        for _ in 0..64_000 {
            meter.update(0);
        }
        assert_eq!(
            meter.level(),
            0,
            "a room that goes quiet must stop reporting a level, not creep down one \
             LSB at a time"
        );
    }

    #[test]
    fn the_meter_never_reads_a_negative_level() {
        let mut meter = PowerMeter::new();
        meter.avg_sq = 1;
        meter.update(0);
        assert_eq!(meter.level(), 0);
    }

    #[test]
    fn removing_the_echo_reads_as_a_large_enhancement() {
        let mut erased = ErleMeter::new();
        let mut untouched = ErleMeter::new();
        for _ in 0..8_000 {
            erased.update(1_000, 30);
            untouched.update(1_000, 1_000);
        }
        // 1000/30 is 30.5 dB, and an exponential average of the two levels
        // settles within a fraction of a decibel of that.
        let db = erased.decibels();
        assert!(
            (30..=31).contains(&db),
            "a 1000 LSB mic reduced to 30 LSB is 30.5 dB, meter reads {db}"
        );
        assert!(
            untouched.decibels() < 1,
            "a residual equal to the microphone is no cancellation at all"
        );
    }

    #[test]
    fn a_silent_residual_reads_as_almost_gone_rather_than_dividing_by_zero() {
        let mut meter = ErleMeter::new();
        for _ in 0..8_000 {
            meter.update(1_000, 0);
        }
        // 1000 LSB against a zero residual is 60 dB; the ceiling of 90 is only
        // reachable by a residual under one LSB², which the floor forbids.
        assert_eq!(meter.decibels(), 60);
    }

    #[test]
    fn the_ceiling_is_a_ceiling() {
        let mut meter = ErleMeter::new();
        for _ in 0..8_000 {
            meter.update(i16::MAX, 1);
        }
        assert_eq!(meter.decibels(), MAX_DECIBELS);
    }
}
