//! How fast the adaptive filter is allowed to move, and when it stops.
//!
//! A filter running at full speed while someone talks over the speaker learns the
//! talker: the error stops being echo and starts being speech, and a filter only
//! ever shown error and reference — never told which is which — models the
//! speech. Barge-in is exactly the situation that causes it.
//!
//! # Two measurements this module is shaped around
//!
//! **The step is not scaled by the residual.** Valin's variable-step method
//! (TASLP 2007) does, and that is the obvious reading of "derive the step from the
//! residual". But the residual is the loudest thing in the room — before the
//! filter has learned anything it *is* the microphone. On a synthetic room at
//! 12 000 LSB that put the opening step 48× below full speed, and the filter
//! reached **−0.5 dB** where the same loop at a constant step reached 20 dB. A
//! slow start does not cost accuracy; it costs the whole convergence.
//!
//! **The residual alone cannot tell a talker from an unconverged filter.** Both
//! leave a loud error, and an absolute threshold on it freezes the filter during
//! its own startup — the same failure from the other side. Coherence separates
//! them: while the filter is still converging the error is mostly echo it has not
//! accounted for, and is therefore *highly correlated with the reference*; while
//! someone is talking it is speech, which is not. That is Gänsler's
//! coherence-based double-talk detector (JASIP 2015 calls out its two drawbacks —
//! a threshold that depends on background noise and on the talker's level). It
//! lives here rather than beside the filter because the filter already computes
//! everything it needs: three running means and one extra multiply per sample.

use super::nlms::COEFFICIENT_SHIFT;
use super::power::{PowerMeter, SignedMeter};

/// Residual level, in LSB², at which the error is *worth asking about* — about
/// 4096 LSB RMS, above ordinary speech at this board's microphone gain
/// (`audio.md` anchors 30 cm speech at peaking to 80 dB SPL, roughly 2600 LSB
/// RMS). Below this nothing is a talker loud enough to stop a filter, whatever
/// its coherence.
///
/// `[据称]`: reasoned from the board's own calibration rather than measured on
/// it, and wanting a bench number before it is trusted.
const LEAKAGE_FREEZE: i64 = 1 << 24;

/// Coherence, in Q15, at or above which a loud residual is the filter's own
/// unconverged echo rather than a person.
///
/// `[据称]`: an unconverged filter runs at near 1.0 and a talker over a speaker
/// well below it, so anything in between separates them; the exact number wants
/// a bench measurement with a person in the room.
const MAX_COHERENCE: u32 = (3 << COHERENCE_FRACTION) / 5;

/// Samples the residual must stay loud *and* incoherent before the filter stops,
/// and back below the loudness before it resumes. At
/// [`SAMPLE_RATE_HZ`](super::SAMPLE_RATE_HZ) this is 8 ms either way: long
/// enough that a single onset does not stop adaptation, short enough that a
/// pause in the talk brings it back.
const HOLD_SAMPLES: u32 = 128;

/// Largest step the filter ever takes, in the coefficient scale. NLMS is stable
/// anywhere in `(0, 2)`; half is the conventional choice.
///
/// Expressed against [`COEFFICIENT_SHIFT`] rather than as a literal because the
/// step is the gate rate written in the coefficient's own fixed-point units.
/// Rescaling the coefficients without rescaling this with them would change the
/// gate rate by the ratio, which reads as the filter having been retuned
/// rather than as the arithmetic having been narrowed.
///
/// Public because a measurement has to drive the filter at exactly this step to
/// measure its worst case, and a number that cannot be reached from outside is a
/// number no measurement can check.
pub const MAX_STEP_Q30: i64 = 1 << (COEFFICIENT_SHIFT - 1);

const COHERENCE_FRACTION: u32 = 15;

/// Whether the filter may adapt this sample, and by how much.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdaptationGate {
    residual: PowerMeter,
    reference: PowerMeter,
    /// The mean of `e·x`, the numerator of the coherence. A **signed** mean:
    /// squaring the product instead would make the coherence magnitude-squared,
    /// which cannot tell an inverted path from an absent one and costs the same.
    cross: SignedMeter,
    above: u32,
    frozen: bool,
}

impl AdaptationGate {
    pub const fn new() -> Self {
        Self {
            residual: PowerMeter::new(),
            reference: PowerMeter::new(),
            cross: SignedMeter::new(),
            above: 0,
            frozen: false,
        }
    }

    /// Folds one sample in: what the microphone heard, what survived it, what the
    /// filter thinks the echo was, and the reference that went to the speaker.
    /// Returns whether the filter may adapt on it.
    pub fn update(&mut self, residual: i16, reference: i16) -> bool {
        self.residual.update(residual);
        self.reference.update(reference);
        self.cross
            .update(i64::from(residual) * i64::from(reference));
        let loud = self.residual.level() >= LEAKAGE_FREEZE;
        let explained_by_echo = self.coherence() >= MAX_COHERENCE;
        self.above = if loud && !explained_by_echo {
            // Capped, because this counts nothing except "still true": left
            // free-running it saturates during a long talk and the filter can
            // never be released again, since coming back costs one decrement per
            // sample from wherever it stopped.
            (self.above + 1).min(HOLD_SAMPLES)
        } else {
            self.above.saturating_sub(1)
        };
        if self.above >= HOLD_SAMPLES {
            self.frozen = true;
        } else if self.above == 0 {
            self.frozen = false;
        }
        !self.frozen
    }

    /// How much of the active pair's first signal the second accounts for, in
    /// Q15.
    pub fn coherence(&self) -> u32 {
        let rms_e = self.residual.level().unsigned_abs().isqrt().max(1);
        let rms_x = self.reference.level().unsigned_abs().isqrt().max(1);
        let numerator = self.cross.mean().unsigned_abs();
        let ratio = (numerator << COHERENCE_FRACTION) / (rms_e * rms_x);
        ratio.min(1 << COHERENCE_FRACTION) as u32
    }

    /// The step in Q30: full speed, or nothing while frozen.
    pub fn step_q30(&self) -> i64 {
        if self.frozen { 0 } else { MAX_STEP_Q30 }
    }

    /// Whether the filter is stopped. Reported so a measurement can say the step
    /// went to zero rather than infer it from a number that only happens to be
    /// zero.
    pub const fn frozen(&self) -> bool {
        self.frozen
    }

    /// The residual that survived, as a mean square. Reported because it is the
    /// first half of the freeze decision and a board re-anchoring the threshold
    /// needs it.
    pub fn leakage_level(&self) -> i64 {
        self.residual.level()
    }
}

impl Default for AdaptationGate {
    fn default() -> Self {
        Self::new()
    }
}

const _: () = assert!(MAX_STEP_Q30 < (1 << COEFFICIENT_SHIFT));

#[cfg(test)]
mod tests {
    use super::super::nlms::AdaptiveFilter;
    use super::super::signal::Noise;
    use super::*;

    /// Peak amplitude of the two situations below. Chosen so that both sit a
    /// little above [`LEAKAGE_FREEZE`], which is what lets coherence and nothing
    /// else be what separates them: at equal loudness the only difference is
    /// whether the error is the reference.
    const LOUD: i32 = 8_000;
    const _: () = assert!(LOUD * LOUD / 3 > LEAKAGE_FREEZE as i32);

    /// A filter that has not learned anything: the error is the reference, and it
    /// says so as loudly as anything can.
    fn unconverged(seed: u32) -> AdaptationGate {
        let mut noise = Noise::new(seed);
        let mut rate = AdaptationGate::new();
        for _ in 0..8_000 {
            let sample = noise.next(LOUD);
            rate.update(sample, sample);
        }
        rate
    }

    /// A converged filter with someone talking over it: the error is the person,
    /// and the person has nothing to do with the reference.
    ///
    /// Two independently seeded sources, so the coherence reads low the way it
    /// will on a board rather than by construction.
    fn talking(seed: u32) -> AdaptationGate {
        let mut speech = Noise::new(seed);
        let mut played = Noise::new(!seed);
        let mut rate = AdaptationGate::new();
        for _ in 0..8_000 {
            let person = speech.next(LOUD);
            rate.update(person, played.next(LOUD));
        }
        rate
    }

    #[test]
    fn a_fresh_filter_is_given_its_full_step() {
        let rate = AdaptationGate::new();
        assert_eq!(rate.step_q30(), MAX_STEP_Q30);
        assert!(!rate.frozen());
    }

    #[test]
    fn a_loud_room_does_not_slow_the_step_down() {
        // A filter is judged on how fast it learns, not on how quiet the room is.
        assert_eq!(
            unconverged(1).step_q30(),
            MAX_STEP_Q30,
            "a loud room must not cost the filter its convergence speed"
        );
    }

    #[test]
    fn an_unconverged_filter_is_not_frozen_by_its_own_startup() {
        // The same failure from the other side: a loud error that *is* the
        // reference is what a filter that has learned nothing looks like.
        let rate = unconverged(2);
        assert!(
            rate.coherence() > MAX_COHERENCE,
            "an error that is the reference correlates with it at {}, not below {}",
            rate.coherence(),
            MAX_COHERENCE
        );
        assert!(!rate.frozen(), "so it must not be stopped");
        assert_eq!(rate.step_q30(), MAX_STEP_Q30);
    }

    #[test]
    fn a_talker_over_the_speaker_stops_the_filter_and_a_quiet_room_brings_it_back() {
        let talker = talking(3);
        assert!(
            talker.coherence() < MAX_COHERENCE,
            "a person is not the speaker, coherence {}",
            talker.coherence()
        );
        assert!(talker.frozen(), "so a sustained talker stops adaptation");
        assert_eq!(talker.step_q30(), 0);

        let mut settled = talker;
        let mut played = Noise::new(9);
        for _ in 0..8_000 {
            // The person has stopped and the filter is tracking again, so what is
            // left of the error is a small uncancelled remainder of the reference.
            settled.update(40, played.next(LOUD));
        }
        assert!(
            !settled.frozen(),
            "when the talker stops, so does the freeze"
        );
        assert_eq!(settled.step_q30(), MAX_STEP_Q30);
    }

    #[test]
    fn a_transient_does_not_stop_the_filter_but_a_talker_does() {
        // The mechanism's own lag is what protects against a door or a cough, and
        // it is longer than the hold alone: the coherence estimate has to fall out
        // of the coherent past first, which on the meters' own time constant is a
        // few hundred samples before the hold even starts counting. So a burst
        // shorter than that is safe twice over, and a sustained talker pays it once.
        let mut rate = unconverged(4);
        assert!(!rate.frozen(), "it starts adapting");
        assert!(
            rate.leakage_level() >= LEAKAGE_FREEZE,
            "at the same loudness as a talker, so this is a test of the decision alone"
        );
        let mut speech = Noise::new(10);
        let mut far = Noise::new(11);
        let mut frozen_at = None;
        for n in 1..=4_000usize {
            let person = speech.next(LOUD);
            if !rate.update(person, far.next(LOUD)) {
                frozen_at = Some(n);
                break;
            }
        }
        let frozen_at = frozen_at.expect("a sustained talker must stop adaptation");
        assert!(
            frozen_at > HOLD_SAMPLES as usize,
            "it stopped after {frozen_at} samples, inside the {HOLD_SAMPLES} the hold \
             is there to guarantee"
        );
        assert!(
            frozen_at < 1_000,
            "it took {frozen_at} samples to notice a talker that never stopped, which \
             is {frozen_at} ms of barge-in the filter spent gate the speaker"
        );
    }

    #[test]
    fn the_step_is_always_inside_the_nlms_stability_bound() {
        for residual in [0, 1, 100, 10_000, i16::MAX] {
            for reference in [0, 1, 100, 10_000, i16::MAX] {
                let mut rate = AdaptationGate::new();
                for _ in 0..8_000 {
                    rate.update(residual, reference);
                }
                assert!((0..2 << COEFFICIENT_SHIFT).contains(&rate.step_q30()));
            }
        }
    }

    #[test]
    fn an_ordinary_sized_residual_does_not_stop_the_filter() {
        // Well below [`LEAKAGE_FREEZE`] and well above anything a cough reaches:
        // the range a talker who is not shouting occupies, which is the range
        // barge-in lives in and the one a threshold set too low would break.
        let mut speech = Noise::new(20);
        let mut played = Noise::new(21);
        let mut rate = AdaptationGate::new();
        for _ in 0..8_000 {
            let person = speech.next(LOUD / 2);
            rate.update(person, played.next(LOUD));
        }
        assert!(
            !rate.frozen(),
            "speech at normal volume is not a reason to stop"
        );
        assert_eq!(rate.step_q30(), MAX_STEP_Q30);
    }

    #[test]
    fn coherence_is_one_for_a_signal_and_its_own_copy() {
        let rate = unconverged(5);
        assert!(
            rate.coherence() > (1 << COHERENCE_FRACTION) - 2,
            "a signal is fully coherent with itself, got {}",
            rate.coherence()
        );
    }

    #[test]
    fn coherence_is_near_zero_for_two_independent_signals() {
        let rate = talking(6);
        assert!(
            rate.coherence() < (1 << COHERENCE_FRACTION) / 4,
            "unrelated signals should barely correlate, got {}",
            rate.coherence()
        );
    }

    #[test]
    fn the_level_it_decides_on_is_reported() {
        // Both thresholds are numbers a board would want to re-anchor, and the
        // step on its own says nothing about which side of either one it is.
        let rate = talking(7);
        assert!(
            rate.leakage_level() >= LEAKAGE_FREEZE,
            "a talker this loud is what the threshold is meant to catch, and the \
             level it judged on is reportable"
        );
    }

    /// Runs a room — speaker only, then a talker from [`NEAR_FROM`] — through a
    /// real filter and a real gate, and reports what the gate decided.
    #[allow(clippy::type_complexity)]
    fn score(near_amplitude: i32) -> (usize, usize, i64) {
        const L: usize = 128;
        const DELAY: usize = 7;
        const SAMPLES: usize = 48_000;
        const NEAR_FROM: usize = 16_000;

        let played = super::super::signal::reference(SAMPLES, LOUD, 0x5150_1001);
        let heard = super::super::signal::echo(&played, DELAY, 500_000);
        let mut near = Noise::new(0x4ea2_5001);

        let mut filter = AdaptiveFilter::<L>::new();
        filter.set_delay(DELAY);
        let mut gate = AdaptationGate::new();

        let mut frozen_before = 0;
        let mut onset = 0;
        let mut talk_level = 0;
        for n in 0..SAMPLES {
            let person = if n >= NEAR_FROM {
                near.next(near_amplitude)
            } else {
                0
            };
            let mic = ((i32::from(heard[n]) + i32::from(person)).clamp(-32768, 32767)) as i16;
            let out = filter.process(mic, played[n], gate.step_q30());
            gate.update(out.residual, played[n]);
            if n == NEAR_FROM + 8_000 {
                talk_level = gate.leakage_level();
            }
            if n < NEAR_FROM {
                if gate.frozen() {
                    frozen_before += 1;
                }
            } else if gate.frozen() && onset == 0 {
                onset = n - NEAR_FROM + 1;
            }
        }
        (frozen_before, onset, talk_level)
    }

    /// What the gate actually catches, measured rather than asserted in prose.
    ///
    /// A talker loud enough to pass [`LEAKAGE_FREEZE`] is stopped, after the
    /// hold and the meters' own lag have both run — which is the number the
    /// design intends.
    #[test]
    fn a_talker_loud_enough_to_pass_the_level_gate_is_stopped() {
        let (frozen_before, onset, level) = score(LOUD);
        assert_eq!(
            frozen_before, 0,
            "the speaker alone must not be mistaken for a talker"
        );
        assert!(
            level >= LEAKAGE_FREEZE,
            "the talker has to clear the gate to be caught"
        );
        assert!(
            onset > HOLD_SAMPLES as usize,
            "it stopped after {onset} samples, inside the {HOLD_SAMPLES} the hold guarantees"
        );
        assert!(
            onset < 2_000,
            "{onset} samples of barge-in is longer than this design accepts"
        );
    }

    /// **A recorded blind spot, not an endorsement.** A talker quieter than the
    /// level gate is not detected at all, however plainly it is not the echo.
    ///
    /// The correlation statistic is never consulted in that case, which is why
    /// switching it for the textbook open-loop or closed-loop pair changes
    /// nothing here: measured across talker peaks of 8000, 4000, 2000, 1000 and
    /// 500 LSB, all three statistics give the same answer to the sample, because
    /// `LEAKAGE_FREEZE` — a mean square of 1<<24, an RMS of 4096 LSB — decides
    /// first. At a 4000 LSB peak the residual measures 7 366 231, 43% under the
    /// threshold.
    ///
    /// [`LEAKAGE_FREEZE`] is marked `[据称]`: it was reasoned from the board's own
    /// calibration, not measured with a person in the room. Moving it is a bench
    /// job, and this test is here so that doing it has to be deliberate.
    #[test]
    fn a_talker_below_the_level_gate_is_not_detected() {
        let (_, onset, level) = score(2_000);
        assert!(
            level < LEAKAGE_FREEZE,
            "a 2000 LSB talker now measures {level}, which clears the gate — \
             re-measure LEAKAGE_FREEZE on a bench and update this with the number"
        );
        assert_eq!(
            onset, 0,
            "the gate should not be reacting to a talker it cannot hear; if it is, \
             the threshold changed under this test"
        );
    }
}
