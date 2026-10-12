//! Linear acoustic echo cancellation — the half of an AEC that decides whether
//! the speaker's own output can be taken back out of the microphone.
//!
//! The board has the reference for free: `audio_out` writes every sample it
//! sends to the codec, so the playback about to become echo is already a sample
//! the firmware holds. That makes the reference exact rather than a third
//! microphone watching the speaker, and is why the digital tap is preferred over
//! the board's MIC3 wherever both are available.
//!
//! # Linear only
//!
//! [`Canceller`] subtracts a *linear* estimate of the echo: one filter adapted
//! online, modelling the acoustic path from the reference to the microphone.
//!
//! Not the other half. A speaker and its amplifier are not linear — a class-D
//! amplifier driving near clipping distorts, and no filter can predict a
//! distortion. Suppressing what survives the linear part is what WebRTC puts
//! behind `aec_process` and Espressif exposes as `aec_nlp_process`, after
//! exactly this split. Nothing here does that, so an ERLE measured here is the
//! number *before* it.
//!
//! # The pieces
//!
//! [`DelayEstimator`] finds the bulk delay, so the filter's window is not spent
//! on the distance between the microphone and its own speaker. [`AdaptiveFilter`]
//! is the NLMS loop. [`AdaptationGate`] replaces a separate double-talk detector:
//! the ratio of the filter's own echo estimate to the residual that survived
//! *is* the leakage, and a filter chasing a talker is exactly what barge-in
//! would otherwise cause. [`ErleMeter`] reports it in the decibels the literature
//! quotes. All fixed point, like the rest of the capture path.
//!
//! # What has been measured
//!
//! `tests/aec_erle.rs` measures against a **computed offline Wiener optimum on
//! the same path**, not a recording: the public vectors either are not
//! synchronized (Espressif's `aec_in_far/near.wav` pair correlates at 0.137) or
//! hold no echo path to find. Every figure it prints is gap-to-optimum, and a
//! figure that beats the optimum is a bug rather than a result.
//!
//! None of that says anything about this board. The acoustic loop gain, the real
//! path length that would fix the tap count, and the CPU cost per tick need the
//! hardware in front of them — the last is what `aec-probe` is for.

mod delay;
mod gate;
mod nlms;
mod power;

#[cfg(test)]
mod signal;

pub use delay::DelayEstimator;
pub use gate::{AdaptationGate, MAX_STEP_Q30};
pub use nlms::{AdaptiveFilter, COEFFICIENT_SHIFT, Cancelled, Coef, MAX_DELAY, MAX_TAPS};
pub use power::{ErleMeter, MAX_DECIBELS, PowerMeter, SignedMeter};

/// The rate this module works at. Espressif's AEC takes 16 kHz and nothing else;
/// the board's codec runs at 48 kHz, so a caller has to decimate first.
pub const SAMPLE_RATE_HZ: u32 = 16_000;

/// The three pieces in the order they depend on each other.
///
/// `delay` is fed from the same call so that a caller does not have to keep two
/// streams of each signal in step by hand, which is the one way this arrangement
/// can silently cancel the wrong thing.
pub struct Canceller<const L: usize> {
    filter: AdaptiveFilter<L>,
    gate: AdaptationGate,
    delay: DelayEstimator,
    erle: ErleMeter,
}

impl<const L: usize> Canceller<L> {
    pub fn new() -> Self {
        Self {
            filter: AdaptiveFilter::new(),
            gate: AdaptationGate::new(),
            delay: DelayEstimator::new(),
            erle: ErleMeter::new(),
        }
    }

    /// Folds one pair in. `reference` is the sample going to the speaker; `mic`
    /// is what the microphone heard. The enhancement is not in the return value
    /// because it is a running figure over far-end single talk — read it from
    /// [`Canceller::erle_decibels`] once a window has gone by, not per sample.
    ///
    /// The delay estimate is applied the moment there is one, and not before:
    /// sliding the window on a guess would throw away taps the filter has not
    /// yet earned.
    pub fn process(&mut self, mic: i16, reference: i16) -> Cancelled {
        if let Some(delay) = self.delay.update(mic, reference) {
            self.filter.set_delay(delay);
        }
        let cancelled = self.filter.process(mic, reference, self.gate.step_q30());
        self.gate.update(cancelled.residual, reference);
        self.erle.update(mic, cancelled.residual);
        cancelled
    }

    /// The enhancement so far, in decibels. Meaningful only over far-end single
    /// talk — see [`ErleMeter`].
    pub fn erle_decibels(&self) -> i32 {
        self.erle.decibels()
    }

    /// Whether adaptation is currently stopped, which a caller might want to
    /// know before reading a number that is not moving.
    pub const fn adapting(&self) -> bool {
        !self.gate.frozen()
    }

    /// The delay the filter has settled on, and whether it has.
    pub const fn delay(&self) -> Option<usize> {
        if self.delay.settled() {
            Some(self.filter.delay())
        } else {
            None
        }
    }

    /// The loudest coefficient, as a magnitude and which tap it sits on — what a
    /// converged filter says the echo path runs through.
    pub fn loudest_tap(&self) -> (usize, i32) {
        self.filter.loudest_tap()
    }

    pub fn filter(&self) -> &AdaptiveFilter<L> {
        &self.filter
    }
}

impl<const L: usize> Default for Canceller<L> {
    fn default() -> Self {
        Self::new()
    }
}

/// What the whole canceller costs to hold, so a board that has to fit it knows
/// before asking.
///
/// `L` is the tap count rather than a number so this cannot drift from the type
/// it describes: a runtime `taps` argument invited a caller to pass a figure
/// the compiler could not check, and that is how the previous version of this
/// function went on reporting 898 B for a structure that holds 3064 B — it
/// counted the filter and none of the rest.
pub const fn state_bytes<const L: usize>() -> usize {
    size_of::<Canceller<L>>()
}

/// The budget the delay estimator's two history buffers were sized against. The
/// canceller does not fit in a kilobyte and never claimed to; what it claimed was
/// wrong, so the bound below is the number that is actually true of the whole.
const _: () = assert!(state_bytes::<MAX_TAPS>() <= 4096, "4 KB was budgeted");

#[cfg(test)]
mod tests {
    use super::signal::{echo, reference};
    use super::*;
    use core::mem::size_of;

    /// The size a board would budget from, against the two structures that
    /// dominate it. A reported figure smaller than these could not be holding
    /// them — which is exactly how the previous estimate of this passed a
    /// "under a kilobyte" assertion while omitting 2 KB of history.
    #[test]
    fn the_reported_size_covers_the_whole_canceller_not_just_the_filter() {
        let whole = state_bytes::<MAX_TAPS>();
        let filter = size_of::<AdaptiveFilter<MAX_TAPS>>();
        let estimator = size_of::<DelayEstimator>();
        assert!(
            whole >= filter + estimator,
            "{whole} B cannot hold a {filter} B filter and a {estimator} B delay estimator"
        );
        assert_eq!(whole, size_of::<Canceller<MAX_TAPS>>());
    }

    /// A room with a near-end talker who is not part of the echo.
    fn room(played: &[i16], delay: usize, near_end_from: usize, near_amplitude: i32) -> Vec<i16> {
        let spoken = reference(
            played.len(),
            near_amplitude,
            0xfeed_0000 + near_end_from as u32,
        );
        let echo = super::signal::echo(played, delay, 600_000);
        (0..played.len())
            .map(|n| {
                let near = if n >= near_end_from {
                    i64::from(spoken[n])
                } else {
                    0
                };
                i64::from(echo[n]) + near
            })
            .map(|v| v as i16)
            .collect()
    }

    #[test]
    fn a_talker_with_no_speaker_produces_no_echo_and_no_filter() {
        // Nothing to cancel: the reference is silent, so no amount of the
        // microphone's own content is echo. This is the case a canceller that
        // forgets to normalize invents an echo out of.
        let mut canceller = Canceller::<16>::new();
        let silence: Vec<i16> = (0..16_000)
            .map(|n| ((n as i16).wrapping_mul(2_513) >> 4) & 0x0f)
            .collect();
        for sample in silence {
            canceller.process(sample, 0);
        }
        let (_, peak) = canceller.loudest_tap();
        assert!(
            peak < (1 << 30) / 64,
            "a silent reference must not teach the filter a path, peak {peak}"
        );
    }

    #[test]
    fn an_echo_only_room_is_cancelled_and_the_reading_says_so() {
        let played = reference(48_000, 9_000, 0x1111_2222);
        let mic = echo(&played, 7, 600_000);
        let mut canceller = Canceller::<32>::new();
        for (&m, &r) in mic.iter().zip(&played) {
            canceller.process(m, r);
        }
        assert!(
            canceller.erle_decibels() > 15,
            "an echo-only room is the easy case; the reading is {}",
            canceller.erle_decibels()
        );
        assert_eq!(canceller.delay(), Some(7), "and the delay was found");
    }

    #[test]
    fn a_talker_over_the_speaker_does_not_get_learned_as_echo() {
        // The barge-in case, and the reason the gate rate exists: with the
        // near-end present the error is speech, and a filter still adapting at
        // full speed would model the speech as a room.
        let played = reference(48_000, 9_000, 0x3333_4444);
        let mic = room(&played, 7, 12_000, 6_000);
        let mut canceller = Canceller::<32>::new();
        for (&m, &r) in mic.iter().zip(&played) {
            canceller.process(m, r);
        }
        let (tap, peak) = canceller.loudest_tap();
        // Tap *zero*, not seven: the delay finder found the 7-sample bulk delay
        // and slid the window onto it, so the filter's job is now the path from
        // there onward. That is the whole point of paying for the estimator —
        // the echo's own delay stops costing taps.
        assert_eq!(tap, 0, "the window has been slid onto the echo path");
        assert!(
            peak < (1 << 30),
            "and should not have walked past full scale into the talker, {peak}"
        );
    }

    #[test]
    fn the_delay_is_not_guessed_before_there_is_evidence_for_it() {
        let mut canceller = Canceller::<16>::new();
        for _ in 0..64 {
            canceller.process(0, 0);
        }
        assert_eq!(
            canceller.delay(),
            None,
            "no history and no signal is a gap in the measurement, not a zero"
        );
    }
}
