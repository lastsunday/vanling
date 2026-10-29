//! A-weighting for the acoustic readout.
//!
//! The panel's scope and dBFS rows see the capture as the codec handed it over, and
//! that is unweighted — so a room humming at 60 Hz scores far higher than a phone app's
//! dB(A). This turns the same stream into the IEC 61672-1 A-weighted level, so the
//! corner readout can say "dBA" and mean a number comparable with one from elsewhere.
//!
//! Textbook analog A-weighting (zeros at DC; poles at 20.6, 107.7, 737.9 Hz and two at
//! 12.194 kHz), prewarped and bilinearly transformed at
//! [`SAMPLE_RATE_HZ`](super::SAMPLE_RATE_HZ), run as a fixed point IIR. Fixed point
//! because there is no floating point in this crate's capture path.

const COEFFICIENT_SHIFT: u32 = 30;

// Extra fractional bits the signal carries between stages. The near-unit-circle
// poles (the 20.6 Hz pair maps to a radius of ~0.999) recycle a state step's
// quantization error into a floor the poles amplify; carrying the signal eight
// bits finer shrinks that amplifier by 2⁸ with the coefficients untouched.
const STATE_FRACTION: u32 = 8;

/// One second-order section of the cascade, in direct form one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Biquad {
    b0: i32,
    b1: i32,
    b2: i32,
    a1: i32,
    a2: i32,
    x1: i32,
    x2: i32,
    y1: i32,
    y2: i32,
}

impl Biquad {
    const fn new(b0: i32, b1: i32, b2: i32, a1: i32, a2: i32) -> Self {
        Self {
            b0,
            b1,
            b2,
            a1,
            a2,
            x1: 0,
            x2: 0,
            y1: 0,
            y2: 0,
        }
    }

    fn update(&mut self, sample: i32) -> i32 {
        let acc = i64::from(self.b0) * i64::from(sample)
            + i64::from(self.b1) * i64::from(self.x1)
            + i64::from(self.b2) * i64::from(self.x2)
            - i64::from(self.a1) * i64::from(self.y1)
            - i64::from(self.a2) * i64::from(self.y2);
        self.x2 = self.x1;
        self.x1 = sample;
        self.y2 = self.y1;
        let out = (acc >> COEFFICIENT_SHIFT) as i32;
        self.y1 = out;
        out
    }
}

/// The full IEC 61672-1 A-weighting response, as a cascade of the three sections below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AWeight {
    hum_shelf: Biquad,
    voice_shelf: Biquad,
    midband: Biquad,
}

impl AWeight {
    // Coefficients computed with a bilinear transform of the analog poles and
    // zeros, gain-normalized so 0 dB lands on 1 kHz. Prewarping the 12.194 kHz
    // pole pair keeps the mid-band shelf where IEC puts it instead of letting
    // the transform's frequency-axis compression drag it below 10 kHz.
    pub(crate) const fn new() -> Self {
        Self {
            hum_shelf: Biquad::new(
                1_070_852_286,
                -2_141_704_573,
                1_070_852_286,
                -2_141_700_679,
                1_067_966_642,
            ),
            voice_shelf: Biquad::new(
                1_017_067_956,
                -2_034_135_912,
                1_017_067_956,
                -2_033_442_876,
                961_087_123,
            ),
            midband: Biquad::new(346_571_357, 693_142_713, 346_571_357, 27_268_646, 173_128),
        }
    }

    pub(crate) fn filter(&mut self, sample: i16) -> i16 {
        let out = self.midband.update(
            self.voice_shelf
                .update(self.hum_shelf.update(i32::from(sample) << STATE_FRACTION)),
        );
        (out >> STATE_FRACTION).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
    }
}

const METER_RELEASE_SHIFT: u32 = 16;

/// A continuously-running A-weighted sound level: the capture's samples through
/// [`AWeight`], folded into a slow exponential average of squares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DbaMeter {
    avg_sq: u64,
}

impl DbaMeter {
    pub const fn new() -> Self {
        Self { avg_sq: 0 }
    }

    /// Folds one A-weighted sample in, as produced by [`AWeight::filter`].
    pub fn update(&mut self, weighted: i16) {
        let square = i64::from(weighted) * i64::from(weighted);
        let avg = self.avg_sq as i64 + ((square - self.avg_sq as i64) >> METER_RELEASE_SHIFT);
        self.avg_sq = avg as u64;
    }

    /// The level the meter currently holds, as an RMS in LSB — the unit the envelope's own
    /// RMS is counted in, so the corner readout can be expressed in exactly the same
    /// decibels.
    pub const fn level_lsb(&self) -> u16 {
        let level = self.avg_sq.isqrt();
        if level > u16::MAX as u64 {
            u16::MAX
        } else {
            level as u16
        }
    }
}

impl Default for DbaMeter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::audio::AudioEnvelope;

    // A full cycle of a 125 Hz sine at [`SAMPLE_RATE_HZ`](super::SAMPLE_RATE_HZ),
    // one 384-sample period at full scale, in 16-bit LSB. Every other test
    // frequency divides 48 kHz over this table: reading every nth entry walks
    // the same waveform at an exact multiple — no interpolation, no floating
    // point, nothing a host-test can drift on.
    const SINE_384: [i16; 384] = [
        0, 536, 1072, 1608, 2143, 2678, 3212, 3745, 4277, 4808, 5338, 5866, 6393, 6917, 7441, 7962,
        8481, 8997, 9512, 10024, 10533, 11039, 11542, 12042, 12539, 13033, 13523, 14010, 14492,
        14971, 15446, 15917, 16383, 16846, 17303, 17756, 18204, 18648, 19086, 19519, 19947, 20370,
        20787, 21199, 21605, 22005, 22399, 22788, 23170, 23546, 23915, 24279, 24636, 24986, 25329,
        25666, 25996, 26319, 26635, 26943, 27245, 27539, 27826, 28105, 28377, 28641, 28898, 29147,
        29388, 29621, 29846, 30064, 30273, 30474, 30667, 30852, 31028, 31196, 31356, 31507, 31650,
        31785, 31911, 32028, 32137, 32238, 32329, 32412, 32487, 32552, 32609, 32657, 32697, 32728,
        32749, 32763, 32767, 32763, 32749, 32728, 32697, 32657, 32609, 32552, 32487, 32412, 32329,
        32238, 32137, 32028, 31911, 31785, 31650, 31507, 31356, 31196, 31028, 30852, 30667, 30474,
        30273, 30064, 29846, 29621, 29388, 29147, 28898, 28641, 28377, 28105, 27826, 27539, 27245,
        26943, 26635, 26319, 25996, 25666, 25329, 24986, 24636, 24279, 23915, 23546, 23170, 22788,
        22399, 22005, 21605, 21199, 20787, 20370, 19947, 19519, 19086, 18648, 18204, 17756, 17303,
        16846, 16383, 15917, 15446, 14971, 14492, 14010, 13523, 13033, 12539, 12042, 11542, 11039,
        10533, 10024, 9512, 8997, 8481, 7962, 7441, 6917, 6393, 5866, 5338, 4808, 4277, 3745, 3212,
        2678, 2143, 1608, 1072, 536, 0, -536, -1072, -1608, -2143, -2678, -3212, -3745, -4277,
        -4808, -5338, -5866, -6393, -6917, -7441, -7962, -8481, -8997, -9512, -10024, -10533,
        -11039, -11542, -12042, -12539, -13033, -13523, -14010, -14492, -14971, -15446, -15917,
        -16384, -16846, -17303, -17756, -18204, -18648, -19086, -19519, -19947, -20370, -20787,
        -21199, -21605, -22005, -22399, -22788, -23170, -23546, -23915, -24279, -24636, -24986,
        -25329, -25666, -25996, -26319, -26635, -26943, -27245, -27539, -27826, -28105, -28377,
        -28641, -28898, -29147, -29388, -29621, -29846, -30064, -30273, -30474, -30667, -30852,
        -31028, -31196, -31356, -31507, -31650, -31785, -31911, -32028, -32137, -32238, -32329,
        -32412, -32487, -32552, -32609, -32657, -32697, -32728, -32749, -32763, -32767, -32763,
        -32749, -32728, -32697, -32657, -32609, -32552, -32487, -32412, -32329, -32238, -32137,
        -32028, -31911, -31785, -31650, -31507, -31356, -31196, -31028, -30852, -30667, -30474,
        -30273, -30064, -29846, -29621, -29388, -29147, -28898, -28641, -28377, -28105, -27826,
        -27539, -27245, -26943, -26635, -26319, -25996, -25666, -25329, -24986, -24636, -24279,
        -23915, -23546, -23170, -22788, -22399, -22005, -21605, -21199, -20787, -20370, -19947,
        -19519, -19086, -18648, -18204, -17756, -17303, -16846, -16384, -15917, -15446, -14971,
        -14492, -14010, -13523, -13033, -12539, -12042, -11542, -11039, -10533, -10024, -9512,
        -8997, -8481, -7962, -7441, -6917, -6393, -5866, -5338, -4808, -4277, -3745, -3212, -2678,
        -2143, -1608, -1072, -536,
    ];

    // The gain a steady full-scale tone at each frequency should produce, as
    // the RMS of the filtered output, in LSB. An exact integer copy of this
    // module produced these, so they pin the implementation within one LSB
    // instead of letting either drift.
    //
    // `(table stride, expected RMS)` with stride 1 at 125 Hz up to 64 at 8 kHz.
    const EXPECTED_OUTPUTS: [(usize, u16); 7] = [
        (1, 3_589),
        (2, 8_523),
        (4, 15_925),
        (8, 23_195),
        (16, 25_254),
        (32, 25_108),
        (64, 22_043),
    ];

    const INPUT_RMS: u16 = 23_169;

    fn filtered_rms(stride: usize, cycles: usize) -> u16 {
        let mut weight = AWeight::new();
        let mut sum_sq: u64 = 0;
        let mut count: u64 = 0;
        for _ in 0..cycles {
            for sample in SINE_384.iter().step_by(stride) {
                let out = i64::from(weight.filter(*sample));
                sum_sq += (out * out) as u64;
                count += 1;
            }
        }
        (sum_sq / count).isqrt() as u16
    }

    #[test]
    fn table_is_one_clean_full_scale_cycle() {
        let sum_sq: u64 = SINE_384
            .iter()
            .map(|&s| u64::from(s.unsigned_abs()) * u64::from(s.unsigned_abs()))
            .sum();
        assert_eq!((sum_sq / 384).isqrt() as u16, INPUT_RMS);
    }

    #[test]
    fn cascade_matches_its_integer_prototype() {
        for (stride, expected) in EXPECTED_OUTPUTS {
            let measured = filtered_rms(stride, 96);
            let diff = i32::from(measured) - i32::from(expected);
            assert!(
                diff.abs() <= 1,
                "stride {stride}: prototype predicted {expected} LSB, cascade read {measured}"
            );
        }
    }

    // The filter is the IEC 61672-1 A-weighting curve, within the band an
    // unclaimed household meter would be embarrassed to miss. Bounds are the
    // IEC table value with ±0.75 dB of slack, converted to RMS LSB against
    // [`INPUT_RMS`].
    #[test]
    fn cascade_sits_in_the_iec_frequency_band() {
        // (stride, lower, upper) in RMS LSB, IEC ± 0.75 dB.
        let bands = [
            (1, 3_329, 3_958),
            (2, 7_896, 9_385),
            (4, 14_703, 17_475),
            (8, 21_252, 25_259),
            (16, 24_400, 29_001),
            (32, 23_845, 28_341),
            (64, 18_724, 22_254),
        ];
        for (stride, lower, upper) in bands {
            let measured = filtered_rms(stride, 96);
            assert!(
                (lower..=upper).contains(&measured),
                "stride {stride}: {measured} LSB outside IEC band {lower}..{upper}"
            );
        }
    }

    #[test]
    fn meter_rises_on_tone_and_settles_back_after() {
        let mut envelope = AudioEnvelope::ZERO;
        for i in 0..262_144 {
            envelope.push(&[SINE_384[(i * 8) % 384]]);
        }
        let peak = envelope.dba_lsb();
        // Steady 1 kHz tone at full scale: converged to the tone's RMS minus
        // the release constant still smoothing it, within five percent.
        assert!(
            (22_011..=24_327).contains(&peak),
            "steady tone read {peak} LSB, expected near full-scale RMS"
        );
        for _ in 0..524_288 {
            envelope.push(&[0]);
        }
        let settled = envelope.dba_lsb();
        assert!(
            settled <= 2_000,
            "after silence the meter still read {settled} LSB"
        );
        assert!(settled < peak, "meter rose while the room went quiet");
    }

    #[test]
    fn meter_ignores_dc_input() {
        let mut envelope = AudioEnvelope::ZERO;
        for _ in 0..262_144 {
            envelope.push(&[i16::MIN]);
        }
        assert!(
            envelope.dba_lsb() <= 2_000,
            "DC held the meter at {}",
            envelope.dba_lsb()
        );
    }

    #[test]
    fn sample_stream_rides_its_bytes_into_the_meter() {
        use crate::drivers::audio::SampleStream;
        // Walk the table at stride 8, so the stream carries a full-scale 1 kHz
        // tone — the band A-weighting passes at unity — rather than 125 Hz,
        // which the filter cuts to a sixteenth.
        let mut bytes = alloc::vec![0u8; SINE_384.len() / 8 * 2];
        for (i, sample) in SINE_384.iter().step_by(8).enumerate() {
            bytes[2 * i..2 * i + 2].copy_from_slice(&sample.to_le_bytes());
        }
        let mut stream = SampleStream::new();
        assert_eq!(stream.dba_lsb(), 0, "a fresh stream is silent");
        for _ in 0..512 {
            stream.push_bytes(&bytes);
        }
        let loud = stream.dba_lsb();
        assert!(
            (8_000..=18_000).contains(&loud),
            "a full-scale tone woke the meter to {loud} LSB"
        );
        for _ in 0..4096 {
            stream.push_bytes(&[0; 32]);
        }
        let quiet = stream.dba_lsb();
        assert!(quiet < loud, "the meter held {quiet} LSB after silence");
    }

    #[test]
    fn a_quiet_floor_reads_quiet_instead_of_feeding_a_limit_cycle() {
        let n = 600_000;
        let mut dig = 0x1234_5678u32;
        let mut sum_sq: u64 = 0;
        let mut envelope = AudioEnvelope::ZERO;
        for i in 0..n {
            dig = dig.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let sample = (((dig >> 16) as i32 % 65) - 32) as i16 + (i % 480 < 48) as i16;
            sum_sq += u64::from(sample.unsigned_abs()) * u64::from(sample.unsigned_abs());
            envelope.push(&[sample]);
        }
        let input_rms = ((sum_sq / n) as f64).sqrt();
        let level = envelope.dba_lsb();
        assert!(
            level <= 200,
            "input RMS {input_rms:.1} LSB read {level} LSB, a limit-cycle floor must not \
         sit in the path a level below the window's own floor reads"
        );
    }
}
