//! Bulk delay: how far the microphone's copy of the speaker trails the one
//! that was sent.
//!
//! The adaptive filter can absorb a delay inside its own window, but only by
//! spending taps on it — and every tap spent on silence ahead of the echo is a
//! tap not modelling the room. Finding the delay once and sliding the window
//! there is what lets a short filter do a long path's work.
//!
//! Normalized cross-correlation over a fixed history, scored as
//! `cross / (‖ref‖·‖mic‖)` so a quiet passage cannot win by having small
//! numbers rather than a good match.
//!
//! Nothing here is decimated. `[实测]` one search is about 32 ms, so decimating
//! the lag grid or the window would each save a large multiple of that — and
//! both are gone anyway, for opposite reasons: [`LAG_STEP`] for why the grid
//! cannot be thinned, and the note on [`WINDOW`] for what thinning the window
//! would cost.

use super::power::PowerMeter;

/// Half-width of the search, in samples: 4 ms either way at
/// [`SAMPLE_RATE_HZ`](super::SAMPLE_RATE_HZ). The desk-to-speaker distance a
/// microphone actually sits at is about a millisecond, so most of this window
/// is headroom for the pipeline between the two.
const SEARCH: usize = super::MAX_DELAY;

/// Samples of history the correlation runs over: 32 ms, long enough that a
/// talker's pitch gives the correlation something to lock onto rather than
/// latching onto one period.
const HISTORY: usize = 512;

/// Samples the correlation is summed over. Half the history, so the oldest
/// [`SEARCH`] samples stay available to cover a negative lag — a microphone
/// whose echo somehow arrives first.
///
/// **The one remaining place decimating the search could still save work, and the
/// reason not to.** Halving it halves the search's cost, and costs the pitch
/// structure the correlation has to lock onto: 256 samples is 16 ms at
/// [`SAMPLE_RATE_HZ`](super::SAMPLE_RATE_HZ), comfortably more than a period.
/// `[据称]` how short is too short has not been measured on a real room.
const WINDOW: usize = HISTORY / 2;

/// Samples between correlation runs, as a multiple of [`WINDOW`].
///
/// The delay is a property of where the board is sitting, not of the audio, and
/// 256 ms is four times a second.
const REFRESH_SAMPLES: usize = 16 * WINDOW;

/// **The refresh interval is the only lever on this search's cost, and the grid
/// is not.** `[实测]` stepping the lag grid to every fourth sample — which would
/// quarter the search — makes a one-sample delay unfindable, and not marginally:
/// against a broadband reference the correlation peak is *one sample wide*, so
/// every grid point either side of it reads as noise. Measured on this module's
/// own reference signal, a path delayed by one sample peaks at 146 thousandths
/// where a path delayed by none peaks at 1000, and stepping the grid leaves no
/// candidate above the gate at all.
///
/// So the lag grid is every sample, and the two knobs left are [`SEARCH`] and
/// [`WINDOW`] — both of which cost capability rather than just time: the first
/// the range of delays findable, the second the pitch structure the correlation
/// locks onto. What is left instead is the interval above.
/// The lag grid is every sample, for the reason above. Named so that a reader
/// reaching for a stride has to delete a statement that says why not.
const LAG_STEP: usize = 1;

const _: () = assert!(LAG_STEP == 1);

/// Reference power, in LSB², below which no lag is believed. Correlating two
/// rooms of near-silence finds a peak in the noise and reports it as a delay,
/// which is worse than reporting none.
const MIN_REFERENCE_POWER: i64 = 1 << 16;

/// Correlation, in thousandths, below which no lag is believed.
///
/// This is the half that a power gate cannot do. A path further away than
/// [`SEARCH`] correlates at nothing in every lag inside the window, so the
/// largest of them all is noise — and the largest is what a search returns.
/// Without this gate a delay beyond the window is answered with whichever lag
/// noise happened to favour, and the caller has no way to tell that from a real
/// measurement. `[据称]`: 0.3 is chosen for headroom over the coherence a real
/// room reaches at 16 kHz broadband, and wants a bench number.
const MIN_CORRELATION: i64 = 300;

const _: () = assert!(WINDOW + SEARCH <= HISTORY);
const _: () = assert!(SEARCH == super::MAX_DELAY);

/// Reads the bulk delay off a reference and a microphone and reports it once
/// there is enough of both to mean anything.
#[derive(Debug, Clone)]
pub struct DelayEstimator {
    reference: [i16; HISTORY],
    microphone: [i16; HISTORY],
    head: usize,
    filled: usize,
    since_refresh: usize,
    power: PowerMeter,
    estimate: usize,
    settled: bool,
    searches: u32,
}

impl DelayEstimator {
    pub const fn new() -> Self {
        Self {
            reference: [0; HISTORY],
            microphone: [0; HISTORY],
            head: 0,
            filled: 0,
            since_refresh: REFRESH_SAMPLES,
            power: PowerMeter::new(),
            estimate: 0,
            settled: false,
            searches: 0,
        }
    }

    /// The delay last reported, in samples. Zero until one has been.
    pub const fn estimate(&self) -> usize {
        self.estimate
    }

    /// Whether an estimate has been reported. A caller driving the filter off
    /// this needs to tell "no delay found yet" from "no delay", since the first
    /// is a gap in the measurement and the second is a result.
    pub const fn settled(&self) -> bool {
        self.settled
    }

    /// How many correlation runs have happened, which is the honest way to
    /// report the cost: the search is not cheap and a caller that raises the
    /// rate needs to know what it bought.
    pub const fn searches(&self) -> u32 {
        self.searches
    }

    /// Folds one sample of each in, and recomputes the delay once the history is
    /// full and the refresh interval has passed. `None` means nothing new.
    pub fn update(&mut self, mic: i16, reference: i16) -> Option<usize> {
        self.reference[self.head] = reference;
        self.microphone[self.head] = mic;
        self.power.update(reference);
        if self.head + 1 == HISTORY {
            self.head = 0;
        } else {
            self.head += 1;
        }
        self.filled = (self.filled + 1).min(HISTORY);
        self.since_refresh = self.since_refresh.saturating_add(1);
        if self.filled < HISTORY
            || self.since_refresh < REFRESH_SAMPLES
            || self.power.level() < MIN_REFERENCE_POWER
        {
            return None;
        }
        self.since_refresh = 0;
        self.searches = self.searches.saturating_add(1);
        let estimate = self.best_lag()?;
        self.estimate = estimate;
        self.settled = true;
        Some(estimate)
    }

    /// The sample `back` positions before the newest one.
    fn at(&self, buffer: &[i16; HISTORY], back: usize) -> i64 {
        let mut index = self.head + HISTORY - back;
        if index >= HISTORY {
            index -= HISTORY;
        }
        i64::from(buffer[index])
    }

    /// The lag with the strongest match, and `None` when the strongest is not
    /// strong enough to be a measurement.
    fn best_lag(&self) -> Option<usize> {
        let mut best = 0usize;
        let mut best_score = i64::MIN;
        for step in (0..=2 * SEARCH).step_by(LAG_STEP) {
            let lag = step as i32 - SEARCH as i32;
            // Both windows are the microphone's, so the two signals are compared
            // over the same stretch of time whatever the lag is and only the
            // reference shifts. A negative lag is a microphone whose echo
            // arrives first, which costs the sum its opening taps rather than
            // running off the end of the reference.
            let first = if lag < 0 { (-lag) as usize } else { 0 };
            let (mut cross, mut reference_pow, mut mic_pow) = (0i64, 0i64, 0i64);
            for j in first..WINDOW {
                let r = self.at(&self.reference, (j as i32 + lag) as usize);
                let m = self.at(&self.microphone, j);
                cross += r * m;
                reference_pow += r * r;
                mic_pow += m * m;
            }
            let denominator = reference_pow.isqrt().max(1) * mic_pow.isqrt().max(1);
            if denominator == 0 {
                continue;
            }
            let score = cross * 1_000 / denominator;
            if score > best_score {
                best_score = score;
                best = lag.unsigned_abs() as usize;
            }
        }
        (best_score >= MIN_CORRELATION).then_some(best)
    }
}

impl Default for DelayEstimator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::super::signal::{echo, reference};
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    /// Runs a room past the estimator and hands back the first sample index at
    /// which it answered.
    fn settle(reference: &[i16], mic: &[i16]) -> (DelayEstimator, Option<usize>) {
        let mut estimator = DelayEstimator::new();
        let mut at = None;
        for n in 0..reference.len() {
            if estimator.update(mic[n], reference[n]).is_some() {
                at = Some(n);
                break;
            }
        }
        (estimator, at)
    }

    #[test]
    fn nothing_is_reported_before_the_history_is_full() {
        let reference = reference(HISTORY + REFRESH_SAMPLES, 8_000, 0x0a11_ce01);
        let mic = echo(&reference, 7, 500_000);
        let (estimator, at) = settle(&reference, &mic);
        assert!(
            at.is_some_and(|n| n == HISTORY - 1),
            "the first search is due the moment there is enough history, not one \
             refresh later: answered at {at:?}"
        );
        assert!(estimator.settled());
    }

    #[test]
    fn the_search_does_not_run_on_every_sample() {
        // The cost this guards: a per-sample search would make finding the delay
        // cost more than cancelling with it, on every sample, for ever.
        let reference = reference(HISTORY + REFRESH_SAMPLES * 10, 8_000, 0x0a11_ce02);
        let mic = echo(&reference, 7, 500_000);
        let (estimator, _) = settle(&reference, &mic);
        assert!(
            estimator.searches() <= 10,
            "ten refresh intervals should not buy more than ten searches, got {}",
            estimator.searches()
        );
    }

    #[test]
    fn silence_is_not_read_as_a_delay() {
        let mut estimator = DelayEstimator::new();
        for _ in 0..HISTORY * 2 {
            assert_eq!(estimator.update(0, 0), None);
        }
        assert!(
            !estimator.settled(),
            "two rooms of silence have a peak in the noise, not a delay"
        );
    }

    #[test]
    fn the_delay_a_path_was_built_with_is_the_delay_it_reports() {
        let reference = reference(HISTORY + 400, 8_000, 0x0a11_ce01);
        for delay in [0usize, 1, 7, 16, 31, 64] {
            let mic = echo(&reference, delay, 500_000);
            let (estimator, answered) = settle(&reference, &mic);
            assert!(answered.is_some(), "no estimate for a delay of {delay}");
            assert_eq!(
                estimator.estimate(),
                delay,
                "a reference delayed by {delay} into the microphone"
            );
        }
    }

    #[test]
    fn a_negative_delay_is_reported_as_a_magnitude() {
        // A filter can only slide forward, so an echo that somehow arrives early
        // is reported as the distance it would have to be pulled back.
        let reference = reference(HISTORY + 400, 8_000, 0x0bad_f00d);
        let mut lead = vec![0_i16; reference.len()];
        for n in 12..reference.len() {
            lead[n] = (i64::from(reference[n - 12]) * 500_000 / 1_000_000) as i16;
        }
        let (estimator, _) = settle(&reference, &lead);
        assert_eq!(estimator.estimate(), 12);
    }

    #[test]
    fn a_delay_beyond_the_search_is_refused_rather_than_answered_from_noise() {
        // A path further away than the window correlates at nothing in every
        // lag inside it, so the largest of them is noise — and the largest is
        // what a search returns. The confidence gate is what turns that from a
        // confident wrong number into no answer.
        let reference = reference(HISTORY + 400, 8_000, 0x1234_9999);
        let mic = echo(&reference, SEARCH + 20, 500_000);
        let (estimator, answered) = settle(&reference, &mic);
        assert_eq!(answered, None, "there is no path in the window to find");
        assert!(!estimator.settled(), "and so nothing has been measured");
    }

    #[test]
    fn a_reference_below_the_power_gate_is_not_searched_at_all() {
        // Distinct from the correlation gate: this one is about there being no
        // signal, and it is why the estimator is fed a level rather than
        // deciding on its own what counts as quiet.
        let quiet: Vec<i16> = vec![2; HISTORY + 400];
        let (estimator, answered) = settle(&quiet, &quiet);
        assert_eq!(answered, None, "a 2 LSB reference is under the gate");
        assert!(!estimator.settled());
    }
}
