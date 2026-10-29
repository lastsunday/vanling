//! How fast the adaptive filter may move, and when it stops.
//!
//! A full-speed filter trained during barge-in learns the talker. The step is not scaled
//! by the residual, because a slow start costs the whole convergence, so double-talk is
//! detected by coherence: echo is coherent with the reference, speech is not.

use super::nlms::COEFFICIENT_SHIFT;
use super::power::{PowerMeter, SignedMeter};

// Residual level (LSB²) above which the error is worth asking about. `[据称]` reasoned
// from the board's calibration, not measured with a person in the room.
const LEAKAGE_FREEZE: i64 = 1 << 24;

// Coherence above which a loud residual is the filter's own echo. `[据称]` wants a bench
// measurement with a person in the room.
const MAX_COHERENCE: u32 = (3 << COHERENCE_FRACTION) / 5;

// Samples the residual must stay loud *and* incoherent before the filter stops,
// and back below the loudness before it resumes. At
// [`SAMPLE_RATE_HZ`](super::SAMPLE_RATE_HZ) this is 8 ms either way: long
// enough that a single onset does not stop adaptation, short enough that a
// pause in the talk brings it back.
const HOLD_SAMPLES: u32 = 128;

/// Largest step the filter ever takes, in the coefficient scale.
pub const MAX_STEP_Q30: i64 = 1 << (COEFFICIENT_SHIFT - 1);

const COHERENCE_FRACTION: u32 = 15;

/// Whether the filter may adapt this sample, and by how much.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdaptationGate {
    residual: PowerMeter,
    reference: PowerMeter,
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

    /// Folds one sample in: what the microphone heard, what survived it, what the filter
    /// thinks the echo was, and the reference that went to the speaker.
    pub fn update(&mut self, residual: i16, reference: i16) -> bool {
        self.residual.update(residual);
        self.reference.update(reference);
        self.cross
            .update(i64::from(residual) * i64::from(reference));
        let loud = self.residual.level() >= LEAKAGE_FREEZE;
        let explained_by_echo = self.coherence() >= MAX_COHERENCE;
        self.above = if loud && !explained_by_echo {
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

    /// How much of the active pair's first signal the second accounts for, in Q15.
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

    /// Whether the filter is stopped.
    pub const fn frozen(&self) -> bool {
        self.frozen
    }

    /// The residual that survived, as a mean square.
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

    const LOUD: i32 = 8_000;
    const _: () = assert!(LOUD * LOUD / 3 > LEAKAGE_FREEZE as i32);

    fn unconverged(seed: u32) -> AdaptationGate {
        let mut noise = Noise::new(seed);
        let mut rate = AdaptationGate::new();
        for _ in 0..8_000 {
            let sample = noise.next(LOUD);
            rate.update(sample, sample);
        }
        rate
    }

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
        assert_eq!(
            unconverged(1).step_q30(),
            MAX_STEP_Q30,
            "a loud room must not cost the filter its convergence speed"
        );
    }

    #[test]
    fn an_unconverged_filter_is_not_frozen_by_its_own_startup() {
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
        let rate = talking(7);
        assert!(
            rate.leakage_level() >= LEAKAGE_FREEZE,
            "a talker this loud is what the threshold is meant to catch, and the \
             level it judged on is reportable"
        );
    }

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

    // A recorded blind spot: a talker below the level gate is not detected. `[据称]`
    // `LEAKAGE_FREEZE` is reasoned from the calibration; moving it is a bench job.
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
