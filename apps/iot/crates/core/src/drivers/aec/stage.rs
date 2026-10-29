//! A capture stream and a reference stream, paired and cancelled.
//!
//! The two transfers are drained on their own cadences and the reference is held back by
//! the transmit ring's 120 ms runway. A mispaired stream still converges and reports a
//! lower ERLE, so the pairing is tested on the residual.
use super::{Cancelled, Canceller};

/// What one stage did with a run of capture samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EchoOutcome {
    /// Pairs actually folded through the canceller.
    pub cancelled: usize,
    /// Capture samples that had no reference to pair with — the two streams were not in
    /// step.
    pub unpaired: usize,
    /// The enhancement over the run, in decibels.
    pub erle_decibels: i32,
    /// The delay the canceller settled on, if it has.
    pub delay: Option<usize>,
    /// The best correlation the delay search found, in thousandths, beside the delay it
    /// produced from it.
    pub delay_peak: i64,
    /// Whether the filter is adapting, so a reading can be read against the right state:
    /// frozen is barge-in, adapting is far-end only.
    pub adapting: bool,
    /// The loudest capture sample in the run, as an absolute LSB magnitude.
    pub mic_peak: u16,
    /// The loudest reference sample in the run, as an absolute LSB magnitude.
    pub reference_peak: u16,
}

/// Pairs a capture stream against a reference and cancels it.
#[derive(Debug, Clone)]
pub struct EchoStage<const L: usize> {
    canceller: Canceller<L>,
    paired: bool,
}

impl<const L: usize> EchoStage<L> {
    /// A stage with a reference to pair against.
    pub fn new() -> Self {
        Self {
            canceller: Canceller::new(),
            paired: true,
        }
    }

    /// A stage with no reference: every sample passes through, and the outcome says zero
    /// were cancelled rather than counting them as faults.
    pub fn unpaired() -> Self {
        Self {
            canceller: Canceller::new(),
            paired: false,
        }
    }

    /// Cancels one decimated capture sample against one reference sample.
    pub fn cancel(&mut self, mic: i16, reference: i16) -> Cancelled {
        self.canceller.process(mic, reference)
    }

    /// Cancels a run of capture samples against a run of references of the same length, and
    /// reports what came of it.
    pub fn cancel_run(&mut self, mic: &[i16], reference: &[i16]) -> EchoOutcome {
        let mut outcome = EchoOutcome::default();
        if !self.paired || mic.is_empty() {
            return outcome;
        }
        if mic.len() != reference.len() {
            outcome.unpaired = mic.len();
            return outcome;
        }
        for (&mic, &reference) in mic.iter().zip(reference) {
            outcome.mic_peak = outcome.mic_peak.max(mic.unsigned_abs());
            outcome.reference_peak = outcome.reference_peak.max(reference.unsigned_abs());
            self.cancel(mic, reference);
            outcome.cancelled += 1;
        }
        outcome.erle_decibels = self.canceller.erle_decibels();
        outcome.delay = self.canceller.delay();
        outcome.delay_peak = self.canceller.delay_peak();
        outcome.adapting = self.canceller.adapting();
        outcome
    }

    /// Where the echo path sits over the whole history, and how strongly it matches there.
    /// **A caller runs this when `delay` is `None`, not on every poll**: it is four times
    /// the search's work, and the question it answers — is there no path, or is the path
    /// further back than [`super::MAX_DELAY`] — is worth asking once and then again slowly,
    /// not per buffer.
    pub fn scan_wide(&mut self) -> (usize, i64) {
        self.canceller.scan_wide()
    }

    /// The share of the room's echo path a filter of `taps` cannot reach, in hundredths,
    /// after a [`Self::scan_wide`].
    pub fn unreachable_by(&self, taps: usize) -> i32 {
        self.canceller.unreachable_by(taps)
    }

    /// The enhancement over the stream so far, in decibels.
    pub fn erle_decibels(&self) -> i32 {
        self.canceller.erle_decibels()
    }

    /// The delay the filter is being driven at, once there is one.
    pub fn delay(&self) -> Option<usize> {
        self.canceller.delay()
    }

    /// Whether adaptation is running, so a reading can be read against the right state:
    /// frozen is barge-in, adapting is far-end only.
    pub fn adapting(&self) -> bool {
        self.canceller.adapting()
    }

    /// The loudest coefficient, as a magnitude and which tap it sits on — what a converged
    /// filter says the echo path runs through.
    pub fn loudest_tap(&self) -> (usize, i32) {
        self.canceller.loudest_tap()
    }

    /// How much the whole stage costs to hold, so a board that has to fit it knows before
    /// asking.
    pub const fn state_bytes() -> usize {
        super::state_bytes::<L>()
    }
}

impl<const L: usize> Default for EchoStage<L> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::super::MAX_DELAY;
    use super::super::signal::{power, reference};
    use super::super::stage_signal::{echo, room_gain};
    use super::*;
    use alloc::vec::Vec;

    // Frames of write-pointer lead a 120 ms transmit ring puts between the
    // sample handed to the DMA and the one on the wire. The misalignment that
    // actually breaks pairing, and the reason this stage reads a play cursor.
    const WRITE_POINTER_LEAD: usize = 120 * 48;

    const SAMPLES: usize = 48_000;
    const AMPLITUDE: i32 = 9_000;
    const DELAY: usize = 7;

    fn residual(stage: &mut EchoStage<64>, mic: &[i16], reference: &[i16]) -> Vec<i16> {
        mic.iter()
            .zip(reference)
            .map(|(&m, &r)| stage.cancel(m, r).residual)
            .collect()
    }

    #[test]
    fn the_room_the_tests_use_is_a_room() {
        let played = reference(SAMPLES, AMPLITUDE, 0x5150_1001);
        let heard = echo(&played, DELAY);
        let gain = room_gain(&played, &heard, DELAY);
        assert!(
            (0.4..0.8).contains(&gain),
            "the room's loop gain is {gain:.3}, so it is either nearly silent or a \
             near-end talker rather than an echo to cancel"
        );
    }

    #[test]
    fn a_small_misalignment_is_absorbed_and_a_large_one_is_not() {
        // Measured, not assumed, and the answer was not the one this type was
        // written expecting. The delay estimator searches ±`MAX_DELAY` samples
        // and the filter's own window slides onto whatever it finds, so a
        // reference one frame out is *recoverable*: the filter simply learns the
        // path one tap over. An implementation that asserted "one frame off
        // ruins it" would have been asserting something false.
        //
        // The misalignment that actually matters is one the search cannot reach,
        // and that is not hypothetical — it is exactly the 120 ms the transmit
        // ring's write pointer sits ahead of the wire. So the test pins the
        // threshold where pairing stops being recoverable, and both sides of it.
        let played = reference(SAMPLES, AMPLITUDE, 0x5150_1001);
        let heard = echo(&played, DELAY);
        let settled = SAMPLES / 2;
        let echo_power = power(&heard[settled..]);

        let cancelled_with = |shift: usize| -> i64 {
            let mispaired: Vec<i16> = (0..SAMPLES)
                .map(|n| played[(n + shift) % SAMPLES])
                .collect();
            let mut stage = EchoStage::<64>::new();
            let out = residual(&mut stage, &heard, &mispaired);
            power(&out[settled..])
        };

        assert!(
            cancelled_with(0) * 10 < echo_power,
            "an aligned pairing takes an order of magnitude off the echo"
        );
        assert!(
            cancelled_with(1) * 10 < echo_power,
            "and one frame out is still recoverable: the estimator slides the \
             window onto the path one tap over. Left {} of {}",
            cancelled_with(1),
            echo_power
        );

        // 120 ms of write-pointer lead — the real fault. Far outside the search window
        // the estimator answers nothing, the filter has no path to learn, and the
        // echo survives.
        assert!(
            cancelled_with(WRITE_POINTER_LEAD) * 10 > echo_power,
            "a reference {WRITE_POINTER_LEAD} frames ({:.0} ms) ahead is outside the \
             ±{MAX_DELAY} search window, so nothing can recover it and the echo must \
             survive: left {} of {echo_power}",
            WRITE_POINTER_LEAD as f64 / 48.0,
            cancelled_with(WRITE_POINTER_LEAD),
        );

        assert!(
            cancelled_with(MAX_DELAY + 1) * 10 > echo_power,
            "one frame beyond the search window is already unrecoverable"
        );
    }

    #[test]
    fn a_capture_with_no_speaker_is_not_a_fault() {
        let mut stage = EchoStage::<64>::unpaired();
        let played = reference(SAMPLES, AMPLITUDE, 0x5150_1002);
        let heard = echo(&played, DELAY);

        let outcome = stage.cancel_run(&heard[..1_000], &played[..1_000]);
        assert_eq!(outcome.cancelled, 0);
        assert_eq!(
            outcome.unpaired, 0,
            "an unwired reference is not a pairing fault"
        );
        assert_eq!(
            outcome.erle_decibels, 0,
            "nothing was cancelled, so nothing was measured"
        );
    }

    #[test]
    fn a_reference_run_read_newest_first_pairs_once_reversed() {
        let played = reference(SAMPLES, AMPLITUDE, 0x5150_1005);
        let heard = echo(&played, DELAY);

        let erle_of = |run: &[i16]| {
            let mut stage = EchoStage::<64>::new();
            stage.cancel_run(&heard, run);
            stage.erle_decibels()
        };
        let aligned = erle_of(&played);
        assert!(aligned > 15, "the aligned run cancels, got {aligned}");

        let mut newest_first = played.clone();
        newest_first.reverse();
        let mut reordered = newest_first.clone();
        reordered.reverse();
        assert_eq!(
            erle_of(&reordered),
            aligned,
            "reversing once has to land back on the aligned pairing"
        );

        assert!(
            erle_of(&newest_first) + 10 < aligned,
            "a chronological microphone against a newest-first reference must be              clearly worse: {} against {aligned}",
            erle_of(&newest_first)
        );
    }

    #[test]
    fn two_runs_of_different_lengths_are_reported_rather_than_padded() {
        let mut stage = EchoStage::<64>::new();
        let played = reference(SAMPLES, AMPLITUDE, 0x5150_1003);
        let heard = echo(&played, DELAY);

        let outcome = stage.cancel_run(&heard[..1_000], &played[..999]);
        assert_eq!(outcome.cancelled, 0, "nothing was paired against anything");
        assert_eq!(outcome.unpaired, 1_000);
        assert_eq!(stage.delay(), None, "and the filter was never shown a room");
    }

    #[test]
    fn an_empty_run_cancels_nothing_without_complaining() {
        let mut stage = EchoStage::<64>::new();
        let outcome = stage.cancel_run(&[], &[]);
        assert_eq!(outcome.cancelled, 0);
        assert_eq!(outcome.unpaired, 0);
    }

    #[test]
    fn a_run_reports_the_filter_state_it_ran_in() {
        let played = reference(SAMPLES, AMPLITUDE, 0x5150_1004);
        let heard = echo(&played, DELAY);
        let mut stage = EchoStage::<64>::new();

        let outcome = stage.cancel_run(&heard, &played);
        assert_eq!(outcome.cancelled, SAMPLES);
        assert_eq!(outcome.unpaired, 0);
        assert_eq!(
            outcome.erle_decibels,
            stage.erle_decibels(),
            "the run's reading and the stage's are the same meter"
        );
        assert_eq!(outcome.delay, stage.delay());
        assert_eq!(outcome.adapting, stage.adapting());
        assert!(
            outcome.erle_decibels > 15,
            "an echo-only room is the easy case, got {}",
            outcome.erle_decibels
        );
    }

    #[test]
    fn the_state_the_stage_reports_covers_the_whole_canceller() {
        let whole = EchoStage::<64>::state_bytes();
        let canceller = super::super::state_bytes::<64>();
        assert_eq!(
            whole, canceller,
            "the stage holds a canceller and nothing else"
        );
        assert!(
            whole <= 4 * 1024,
            "{whole} B against the 4 KB the canceller was budgeted at"
        );
    }
}
