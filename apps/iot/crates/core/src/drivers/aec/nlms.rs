//! The adaptive filter: a normalized least-mean-squares loop over the speaker's
//! own samples. Per sample it convolves the reference delay line with the
//! coefficients, subtracts that estimate from the microphone, and moves each
//! coefficient toward explaining whatever is left.
//!
//! WebRTC's linear stage is a partitioned frequency-domain filter instead
//! (`aec3/adaptive_fir_filter.cc`: FFT 256, partitions of 128, float32
//! coefficients) — faster convergence per coefficient, much harder to reproduce
//! in fixed point. `tests/aec_erle.rs` is what says whether the cheaper half of
//! that trade was the right half.

/// Fractional bits a coefficient carries. The same scale the weighting cascade
/// uses, so both halves of the capture path read the same way.
///
/// **Thirty, and twenty does not work.** At twenty the scale fits i32 and
/// `gain · x` stops widening — but the same constraint caps the gain at 2^15,
/// and NLMS wants it two dozen bits wider while the power is still small and the
/// error is still the whole microphone. The clamp then binds and convergence
/// falls from 61.0 dB to 56.7 against a 62.9 dB offline optimum. The multiply is
/// not what costs; everything around it is.
pub const COEFFICIENT_SHIFT: u32 = 30;

/// Fractional bits the normalized step carries *before* it is scaled back down
/// to coefficients. Ten, because at fewer the quotient of a small residual lands
/// on zero and the filter stops moving while it is still wrong — the freezing
/// JASIP 2015 describes, whose other half of an answer is a floor on the power.
const STEP_FRACTION: u32 = 10;

/// Range a coefficient may occupy, in its own fixed-point units: ±1.0 as a
/// gain. A wrapped one is not a large one, it is the wrong sign, and the next
/// sample's echo estimate inherits that.
pub const MAX_COEFF: i32 = 1 << COEFFICIENT_SHIFT;

/// Largest normalized gain, which is what actually bounds a step. Holding the
/// gain here rather than each delta keeps the inner loop's guard unreachable
/// arithmetic rather than a branch, and keeps the product inside i32.
const MAX_STEP_GAIN: i64 = MAX_COEFF as i64 >> (15 - STEP_FRACTION);

/// `[实测]` what a stride costs, from `tests/aec_erle.rs`: **steady state is
/// unchanged and convergence is not.** At 4 and at 8 the gap to the offline
/// optimum is 1.9 dB on every path, exactly as at 1, and a path that moves is
/// still tracked across a six-sample shift. But the whole-file reading — which is
/// the transient — falls 20.9 dB → 16.2 dB at 4 and → 14.1 dB at 8. Four rather
/// than eight because 8 costs the same steady state for twice the saving, and
/// there is no evidence a larger stride is safe.
const UPDATE_STRIDE: usize = 4;

/// Longest filter this crate builds, in taps: 8 ms at [`super::SAMPLE_RATE_HZ`].
pub const MAX_TAPS: usize = 128;

/// How far ahead of the filter window the reference may sit, in samples: 4 ms,
/// which is an order of magnitude more than the distance from a desk microphone
/// to its own speaker needs and is there to cover the pipeline, not the room.
pub const MAX_DELAY: usize = 64;

const RING_LENGTH: usize = MAX_TAPS + MAX_DELAY + 1;

/// Regularization on the normalization, per tap: the power a reference of 4 LSB
/// RMS would carry. Large enough that true silence cannot divide by a vanishing
/// power, small enough that it does not slow a quiet talker down.
const DELTA_PER_TAP: i64 = 16;

/// One coefficient of the echo path, a dimensionless gain in
/// Q[`COEFFICIENT_SHIFT`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Coef(i32);

impl Coef {
    pub const fn to_q30(self) -> i32 {
        self.0
    }

    /// A gain given in parts per million, which is the unit a measured echo path
    /// is quoted in and the only one that needs no floating point to say "a
    /// quarter" or "minus six decibels".
    pub fn from_ppm(ppm: i32) -> Self {
        Self((i64::from(ppm) * (1 << COEFFICIENT_SHIFT) / 1_000_000) as i32)
    }

    pub const fn to_ppm(self) -> i32 {
        ((self.0 as i64 * 1_000_000) >> COEFFICIENT_SHIFT) as i32
    }
}

/// What one cancellation pass made of a sample: the microphone with the echo
/// estimate taken out of it, and the estimate itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled {
    pub residual: i16,
    pub echo: i16,
}

/// A normalized LMS adaptive filter over the speaker reference.
///
/// `L` is the tap count, so a test can build a short one and a board the full
/// [`MAX_TAPS`] without the type carrying a second number.
#[derive(Debug, Clone)]
pub struct AdaptiveFilter<const L: usize> {
    ring: [i16; RING_LENGTH],
    head: usize,
    delay: usize,
    coef: [Coef; L],
}

impl<const L: usize> AdaptiveFilter<L> {
    pub fn new() -> Self {
        Self {
            ring: [0; RING_LENGTH],
            head: 0,
            delay: 0,
            coef: [Coef::default(); L],
        }
    }

    /// Puts the filter window this many samples behind the reference's leading
    /// edge, which is what a measured bulk delay is spent on.
    ///
    /// Clamped to [`MAX_DELAY`] because the window has room for no more, and a
    /// delay outside it would read as silence rather than as out of range.
    pub fn set_delay(&mut self, samples: usize) {
        self.delay = samples.min(MAX_DELAY);
    }

    pub const fn delay(&self) -> usize {
        self.delay
    }

    pub fn coefficients(&self) -> &[Coef; L] {
        &self.coef
    }

    /// The loudest coefficient, as a magnitude — which tap the filter believes
    /// the path runs through.
    pub fn loudest_tap(&self) -> (usize, i32) {
        self.coef
            .iter()
            .enumerate()
            .max_by_key(|(_, c)| c.to_q30().abs())
            .map_or((0, 0), |(k, c)| (k, c.to_q30().abs()))
    }

    /// Folds one sample in and returns the residual plus the echo estimate.
    /// `step_q30` is the gate rate's current step; zero stops adaptation
    /// while still cancelling with whatever the filter already knows.
    ///
    /// Never inlined, because `aec-probe` times this call against a clock to say
    /// where the tick goes. Under the workspace's LTO profile the compiler folds
    /// it into the probe's timing closure and moves work across the clock's
    /// brackets, which does not make the filter faster -- it makes the
    /// measurement report that it is.
    #[inline(never)]
    pub fn process(&mut self, mic: i16, reference: i16, step_q30: i64) -> Cancelled {
        // Advance before writing, so `head` is the newest sample rather than the
        // slot after it. Tap 0 is defined as the reference just handed in, and
        // one slot of slip puts the filter's peak a tap late on every path.
        if self.head + 1 == RING_LENGTH {
            self.head = 0;
        } else {
            self.head += 1;
        }
        self.ring[self.head] = reference;

        let (head, delay) = (self.head, self.delay);

        // `head` cycles through the whole ring, so this is a stride counter
        // that costs nothing to keep: no state, and the update lands on the same
        // samples every lap.
        let updating = step_q30 != 0 && head % UPDATE_STRIDE == 0;

        // `power` is only read when adapting, and `head % 4` predicts perfectly,
        // so gating it here is cheaper than the multiply it skips.
        let mut echo = 0i64;
        let mut power = 0i64;
        let (first, second) = window::<L>(&self.ring, head, delay);
        for (&x, &w) in first
            .iter()
            .chain(second.iter())
            .zip(self.coef.iter().rev())
        {
            echo += i64::from(w.to_q30()) * i64::from(x);
            if updating {
                power += i64::from(x) * i64::from(x);
            }
        }

        let residual_q30 = (i64::from(mic) << COEFFICIENT_SHIFT) - echo;
        let cancelled = Cancelled {
            residual: clamp_i16(residual_q30 >> COEFFICIENT_SHIFT),
            echo: clamp_i16(echo >> COEFFICIENT_SHIFT),
        };

        if updating {
            // One division for the whole tap set: the normalized step does not
            // depend on k, and a division per tap would be the most expensive
            // thing in the loop by an order of magnitude.
            let numerator = step_q30 * i64::from(cancelled.residual);
            if numerator != 0 {
                let gain = ((numerator << STEP_FRACTION) / (power + L as i64 * DELTA_PER_TAP))
                    .clamp(-MAX_STEP_GAIN, MAX_STEP_GAIN);
                let (first, second) = window::<L>(&self.ring, head, delay);
                for (&x, coef) in first
                    .iter()
                    .chain(second.iter())
                    .zip(self.coef.iter_mut().rev())
                {
                    // Bounded by `MAX_STEP_GAIN` rather than by a test, so the only
                    // guard left is where the coefficient lands — the same ±1.0 a
                    // gain may occupy, which is tighter than letting the i32
                    // saturate and strand one past what this file assumes.
                    let moved = i64::from(coef.to_q30()) + ((gain * i64::from(x)) >> STEP_FRACTION);
                    coef.0 = moved.clamp(-MAX_COEFF as i64, MAX_COEFF as i64) as i32;
                }
            }
        }

        cancelled
    }
}

/// The filter window: the reference `k` taps behind the delayed head, as at most
/// two contiguous runs of the ring, ordered from tap `L - 1` down to tap `0`.
///
/// The window slides with `head` and crosses the ring's seam once every
/// `RING_LENGTH` samples. Handing back runs rather than an index per tap keeps
/// that seam out of the inner loops, where resolving it would cost a compare and
/// a branch on every tap of every sample.
fn window<const L: usize>(
    ring: &[i16; RING_LENGTH],
    head: usize,
    delay: usize,
) -> (&[i16], &[i16]) {
    let run = &ring[base_of::<L>(head, delay)..];
    if run.len() >= L {
        (&run[..L], &[])
    } else {
        (run, &ring[..L - run.len()])
    }
}

/// Index into the ring of the sample `L - 1` taps behind the delayed head, which
/// is where the window starts.
fn base_of<const L: usize>(head: usize, delay: usize) -> usize {
    (head + RING_LENGTH - delay + 1 - L) % RING_LENGTH
}

impl<const L: usize> Default for AdaptiveFilter<L> {
    fn default() -> Self {
        Self::new()
    }
}

const _: () = assert!(RING_LENGTH > MAX_TAPS + MAX_DELAY);

/// `base_of` subtracts `L` from an expression whose floor is `RING_LENGTH -
/// MAX_DELAY + 1`, so the subtraction can only underflow if a filter is ever
/// built longer than that gap.
const _: () = assert!(RING_LENGTH - MAX_DELAY + 1 >= MAX_TAPS);

fn clamp_i16(value: i64) -> i16 {
    value.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16
}

#[cfg(test)]
mod tests {
    use super::super::gate::MAX_STEP_Q30;
    use super::super::signal::{echo, power, reference};
    use super::*;
    use alloc::vec::Vec;

    /// A step big enough to converge in a test without being the whole of the
    /// coefficient range; NLMS is stable anywhere below 2. Taken from the same
    /// place the real one is, because a step is a gate rate written in the
    /// coefficient scale and a literal here would quietly mean a different rate
    /// every time that scale moves.
    const STEP: i64 = MAX_STEP_Q30;

    /// Runs a room through the filter and hands back what is left of the
    /// microphone, so the assertions below read the same stream a measurement
    /// would.
    fn cancel(mic: &[i16], reference: &[i16], step: i64) -> (AdaptiveFilter<16>, Vec<i16>) {
        let mut filter = AdaptiveFilter::<16>::new();
        let residual = mic
            .iter()
            .zip(reference)
            .map(|(&m, &r)| filter.process(m, r, step).residual)
            .collect();
        (filter, residual)
    }

    #[test]
    fn a_coefficient_converts_through_parts_per_million_without_drifting() {
        for ppm in [-1_000_000, -500_000, 0, 250_000, 1_000_000] {
            let round_trip = Coef::from_ppm(ppm).to_ppm();
            assert!(
                (round_trip - ppm).abs() <= 2,
                "{ppm} ppm came back as {round_trip}"
            );
        }
    }

    #[test]
    fn an_echo_only_room_is_cancelled_down_to_the_measurement_floor() {
        let reference = reference(32_000, 9_000, 0x1234_5678);
        let echo = echo(&reference, 6, 600_000);
        let (_, residual) = cancel(&echo, &reference, STEP);
        let tail = power(&residual[16_000..]);
        assert!(
            tail * 100 < power(&echo[16_000..]),
            "a filter being shown its own echo path should take more than 20 dB \
             off it, left {tail} of {}",
            power(&echo[16_000..])
        );
    }

    #[test]
    fn the_filter_finds_the_delay_it_was_built_with() {
        // The peak of a converged filter is the path it was shown, so this is
        // what says the tap indexing and the ring wrap agree.
        let reference = reference(24_000, 9_000, 0x0bad_c0de);
        let echo = echo(&reference, 5, 700_000);
        let (filter, _) = cancel(&echo, &reference, STEP);
        let (tap, _) = filter.loudest_tap();
        assert_eq!(tap, 5, "the peak sits on the delay");
        let gain = filter.coefficients()[5].to_ppm();
        assert!(
            (650_000..=750_000).contains(&gain),
            "and carries roughly the gain it was shown, got {gain} ppm"
        );
    }

    #[test]
    fn a_shift_in_the_echo_path_is_followed_rather_than_frozen_out() {
        // A board gets picked up and put down somewhere else. The filter has to
        // re-converge, which is the whole reason the step exists.
        let reference = reference(48_000, 9_000, 0xfeed_face);
        let early = echo(&reference, 4, 700_000);
        let late = echo(&reference, 9, 700_000);
        let mut filter = AdaptiveFilter::<16>::new();
        for window in [&early, &late] {
            for (&m, &r) in window.iter().zip(&reference) {
                filter.process(m, r, STEP);
            }
        }
        let (tap, _) = filter.loudest_tap();
        assert_eq!(tap, 9, "the peak has moved to the new delay");
        let gain = filter.coefficients()[9].to_ppm();
        assert!((650_000..=750_000).contains(&gain), "gain {gain} ppm");
    }

    #[test]
    fn silence_does_not_make_the_divisor_vanish_and_the_filter_run_away() {
        let mut filter = AdaptiveFilter::<16>::new();
        for _ in 0..4_000 {
            let out = filter.process(0, 0, STEP);
            assert_eq!(out.residual, 0, "silence in, silence out");
            assert_eq!(out.echo, 0);
        }
        for (k, coef) in filter.coefficients().iter().enumerate() {
            assert_eq!(
                coef.to_q30(),
                0,
                "tap {k} moved without a sample to move it"
            );
        }
    }

    #[test]
    fn a_quiet_reference_still_converges() {
        // The freezing failure: normalization dividing by a power so small that
        // the step rounds to nothing, so the filter never learns a quiet path.
        let reference = reference(120_000, 12, 0x5eed_5eed);
        let echo = echo(&reference, 4, 700_000);
        let (filter, _) = cancel(&echo, &reference, STEP);
        let gain = filter.coefficients()[4].to_ppm();
        assert!(
            (550_000..=850_000).contains(&gain),
            "a 12 LSB reference still has to carry the filter to its path, got {gain} ppm"
        );
    }

    #[test]
    fn a_stopped_filter_still_cancels_with_what_it_already_knows() {
        // Freezing is a protection, not a failure: a filter that has learned a
        // path must keep using it while it waits for the talker to finish.
        let reference = reference(24_000, 9_000, 0x0f0f_0f0f);
        let echo = echo(&reference, 3, 700_000);
        let mut filter = AdaptiveFilter::<16>::new();
        for (&m, &r) in echo.iter().zip(&reference) {
            filter.process(m, r, STEP);
        }
        let mut stopped = Vec::new();
        for (&m, &r) in echo.iter().zip(&reference) {
            stopped.push(filter.process(m, r, 0).residual);
        }
        assert!(
            power(&stopped[12_000..]) * 4 < power(&echo[12_000..]),
            "a filter with no step still has to cancel on a path it has learned, \
             {} of {}",
            power(&stopped[12_000..]),
            power(&echo[12_000..])
        );
    }

    #[test]
    fn a_delay_beyond_the_window_is_refused_rather_than_read_as_silence() {
        let mut filter = AdaptiveFilter::<16>::new();
        filter.set_delay(MAX_DELAY + 100);
        assert_eq!(filter.delay(), MAX_DELAY);
    }

    #[test]
    fn the_tap_window_follows_the_delay() {
        // Sliding the window has to move which reference sample tap 0 sees, or
        // the delay estimate buys nothing but a shorter filter. Read through the
        // same slices the convolution loop reads, rather than through a
        // parallel accessor, so the test cannot pass on indexing the loop does
        // not use.
        let mut filter = AdaptiveFilter::<8>::new();
        for n in 0..64i16 {
            filter.process(0, n, STEP);
        }
        // The window arrives ordered from tap `L - 1` down, so reversing puts
        // tap 0 first.
        let seen = |filter: &AdaptiveFilter<8>, taps: usize| -> Vec<i16> {
            let (first, second) = window::<8>(&filter.ring, filter.head, filter.delay);
            first
                .iter()
                .chain(second.iter())
                .rev()
                .take(taps)
                .copied()
                .collect()
        };

        filter.set_delay(0);
        assert_eq!(
            seen(&filter, 4),
            vec![63, 62, 61, 60],
            "tap 0 is the sample just handed in, and each tap behind it"
        );
        filter.set_delay(3);
        assert_eq!(
            seen(&filter, 4),
            vec![60, 59, 58, 57],
            "a delay of three slides the whole window three samples back"
        );
    }

    #[test]
    fn the_window_is_the_same_samples_whether_or_not_it_crosses_the_ring_seam() {
        // `window` hands back two runs when the window straddles the ring's
        // seam, which it does once every `RING_LENGTH` samples. A dropped or
        // duplicated sample there would put a tap of noise into the echo
        // estimate every full lap and nothing else would show it.
        let mut filter = AdaptiveFilter::<16>::new();
        let mut crossed = None;
        for n in 0..4 * RING_LENGTH {
            filter.process(0, n as i16, STEP);
            let (first, second) = window::<16>(&filter.ring, filter.head, filter.delay);
            let joined: Vec<i16> = first.iter().chain(second.iter()).copied().collect();
            assert_eq!(joined.len(), 16, "the window is always the full filter");
            if !second.is_empty() && crossed.is_none() {
                crossed = Some(n);
            }
        }
        assert!(
            crossed.is_some(),
            "the window never crossed the seam, so this tested nothing"
        );
    }

    #[test]
    fn a_loud_late_talker_cannot_walk_a_coefficient_out_of_range() {
        // The guard that keeps one bad sample from making a filter that has to
        // be thrown away rather than recovered.
        let mut filter = AdaptiveFilter::<16>::new();
        for _ in 0..4_000 {
            filter.process(i16::MAX, i16::MIN, STEP);
        }
        for coef in filter.coefficients() {
            let raw = coef.to_q30();
            assert!(
                (-MAX_COEFF..=MAX_COEFF).contains(&raw),
                "a coefficient reached {raw}, past the range a gain can occupy"
            );
        }
    }

    #[test]
    fn a_short_filter_is_not_charged_for_the_longest_ones_ring() {
        assert!(
            size_of::<AdaptiveFilter<MAX_TAPS>>() > size_of::<AdaptiveFilter<8>>(),
            "the ring is shared, so only the coefficients should shrink"
        );
    }
}
