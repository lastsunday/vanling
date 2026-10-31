//! Bulk delay: how far the microphone's copy of the speaker trails the one that was sent.
//!
//! Normalized cross-correlation over a fixed history, then a lock that follows it slowly.
//! `[实测]` one search is 11 ms at 240 MHz.
use super::power::PowerMeter;

// Half-width of the search, in samples: 4 ms either way at
// [`SAMPLE_RATE_HZ`](super::SAMPLE_RATE_HZ). The desk-to-speaker distance a
// microphone actually sits at is about a millisecond, so most of this window
// is headroom for the pipeline between the two.
const SEARCH: usize = super::MAX_DELAY;

// Samples of history the correlation runs over: 32 ms, long enough that a
// talker's pitch gives the correlation something to lock onto rather than
// latching onto one period.
pub(crate) const HISTORY: usize = 512;

// Correlation window: half the history, so a negative lag has room. A shorter window
// loses the pitch structure the correlation locks onto; `[据称]` how short is too short
// has not been measured on a real room.
pub(crate) const WINDOW: usize = HISTORY / 2;

// The delay is a property of the room, not the audio; 256 ms is four times a second.
const REFRESH_SAMPLES: usize = 16 * WINDOW;

// **The refresh interval is the only lever on cost; the grid is not.** `[实测]` stepping
// the grid to every fourth sample makes a one-sample delay unfindable: the correlation
// peak is one sample wide, and every other grid point reads as noise.
const LAG_STEP: usize = 1;

const _: () = assert!(LAG_STEP == 1);

const MIN_REFERENCE_POWER: i64 = 1 << 16;

// Correlation, in thousandths, below which no lag is believed. A path beyond `SEARCH`
// correlates at noise, and the largest noise peak is what a search would return.
// `[实测]` the measured room peaks at 312, 4% over this gate.
const MIN_CORRELATION: i64 = 300;

// `[实测]` a reverberant room re-peaks by one to two samples between refreshes, so the
// lock follows no more than this; the search itself is unchanged.
const DRIFT_SAMPLES: usize = 2;

const MISSES_BEFORE_MOVING: u32 = 4;

const _: () = assert!(WINDOW + SEARCH <= HISTORY);
const _: () = assert!(SEARCH == super::MAX_DELAY);

/// Bands the wide scan's path energy is accumulated into: two milliseconds each. This is
/// not an impulse response and must not be read as one.
pub const PROFILE_BANDS: usize = 8;
pub const PROFILE_BAND: usize = 32;

const _: () = assert!(
    PROFILE_BANDS * PROFILE_BAND >= HISTORY - WINDOW,
    "the profile has to cover every lag the wide scan reaches"
);

/// Reads the bulk delay off a reference and a microphone and reports it once there is
/// enough of both to mean anything.
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
    peak: i64,
    profile: [u64; PROFILE_BANDS],
    locked_delay: usize,
    has_lock: bool,
    missed: u32,
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
            peak: 0,
            profile: [0; PROFILE_BANDS],
            locked_delay: 0,
            has_lock: false,
            missed: 0,
        }
    }

    /// The delay last reported, in samples.
    #[cfg(test)]
    pub const fn estimate(&self) -> usize {
        self.estimate
    }

    /// Whether an estimate has been reported.
    pub const fn settled(&self) -> bool {
        self.settled
    }

    /// The best correlation the last search found, in thousandths, whether or not it passed
    /// the gate.
    pub const fn peak(&self) -> i64 {
        self.peak
    }

    /// How many correlation runs have happened, which is the honest way to report the cost:
    /// the search is not cheap and a caller that raises the rate needs to know what it
    /// bought.
    pub const fn searches(&self) -> u32 {
        self.searches
    }

    /// Where the strongest match sits over a range far wider than `SEARCH`, found on a
    /// coarse grid, and how strong it is.  **The fine search answers "can I cancel this?";
    /// this answers "where is the path at all?"** A delay past what the filter can hold is
    /// not a marginal result from the fine search — it is no peak anywhere inside it, which
    /// is indistinguishable from a room with no echo until something looks further out.
    pub fn scan_wide(&mut self, reach: usize) -> (usize, i64) {
        let mut best = 0usize;
        let mut best_score = i64::MIN;
        let mut bands = [0u64; PROFILE_BANDS];
        for step in 0..=2 * reach {
            let lag = step as i32 - reach as i32;
            let k = lag.unsigned_abs() as usize;
            if k + WINDOW > HISTORY {
                continue;
            }
            let (mut cross, mut reference_pow, mut mic_pow) = (0i64, 0i64, 0i64);
            for j in 0..WINDOW {
                let (reference_back, mic_back) = if lag < 0 { (j, j + k) } else { (j + k, j) };
                let r = self.at(&self.reference, reference_back);
                let m = self.at(&self.microphone, mic_back);
                cross += r * m;
                reference_pow += r * r;
                mic_pow += m * m;
            }
            let denominator = reference_pow.isqrt().max(1) * mic_pow.isqrt().max(1);
            if denominator == 0 {
                continue;
            }
            let score = cross * 1_000 / denominator;
            if k < PROFILE_BANDS * PROFILE_BAND {
                bands[k / PROFILE_BAND] += (score * score).unsigned_abs();
            }
            if score > best_score {
                best_score = score;
                best = k;
            }
        }
        self.profile = bands;
        (best, best_score)
    }

    /// The last wide scan's path energy by band, oldest lag first.  **Raw, with the noise
    /// floor still in it** — a quiet room correlates everywhere a little and the bands say
    /// so.
    #[cfg(test)]
    pub const fn profile(&self) -> &[u64; PROFILE_BANDS] {
        &self.profile
    }

    /// The share of the path a filter of `taps`, placed at the estimated delay, cannot
    /// reach — in hundredths.  **The number the tap budget is decided by.** A filter models
    /// the path from the delay onward and nothing before it, so whatever lies past its last
    /// tap is what survives however well the rest converges.
    pub fn unreachable_by(&self, taps: usize) -> i32 {
        if !self.settled || self.peak.abs() < MIN_CORRELATION {
            return -1;
        }
        let floor = *self.profile.iter().min().unwrap_or(&0);
        let raised = self.profile.iter().map(|band| band - floor);
        let total: u64 = raised.clone().sum();
        if total == 0 {
            return -1;
        }
        let (window_start, window_end) = (self.estimate, self.estimate + taps);
        let reached: u64 = raised
            .enumerate()
            .filter(|(band, _)| {
                let start = band * PROFILE_BAND;
                start < window_end && start + PROFILE_BAND > window_start
            })
            .map(|(_, energy)| energy)
            .sum();
        (((total - reached) * 100) / total) as i32
    }

    /// Folds one sample of each in, and answers the delay once the history is full and the
    /// refresh interval has passed.
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
        let found = self.best_lag()?;
        if self.has_lock {
            if found.abs_diff(self.locked_delay) <= DRIFT_SAMPLES {
                self.locked_delay = found;
                self.missed = 0;
            } else {
                self.missed = self.missed.saturating_add(1);
                if self.missed >= MISSES_BEFORE_MOVING {
                    self.locked_delay = found;
                    self.missed = 0;
                }
            }
        } else {
            self.locked_delay = found;
            self.has_lock = true;
        }
        self.estimate = self.locked_delay;
        self.settled = true;
        Some(self.estimate)
    }

    fn at(&self, buffer: &[i16; HISTORY], back: usize) -> i64 {
        let mut index = self.head + HISTORY - back;
        if index >= HISTORY {
            index -= HISTORY;
        }
        i64::from(buffer[index])
    }

    fn best_lag(&mut self) -> Option<usize> {
        let mut best = 0usize;
        let mut best_score = i64::MIN;
        for step in (0..=2 * SEARCH).step_by(LAG_STEP) {
            let lag = step as i32 - SEARCH as i32;
            let k = lag.unsigned_abs() as usize;
            let (mut cross, mut reference_pow, mut mic_pow) = (0i64, 0i64, 0i64);
            for j in 0..WINDOW {
                let (reference_back, mic_back) = if lag < 0 { (j, j + k) } else { (j + k, j) };
                let r = self.at(&self.reference, reference_back);
                let m = self.at(&self.microphone, mic_back);
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
        self.peak = best_score;
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
        let reference = reference(HISTORY + 400, 8_000, 0x0bad_f00d);
        let mut lead = vec![0_i16; reference.len()];
        for n in 12..reference.len() {
            lead[n] = (i64::from(reference[n - 12]) * 500_000 / 1_000_000) as i16;
        }
        let (estimator, _) = settle(&reference, &lead);
        assert_eq!(estimator.estimate(), 12);
    }

    fn moving_path(reference: &[i16], step: usize, delays: &[usize]) -> Vec<i16> {
        (0..reference.len())
            .map(|n| {
                let delay = delays[(n / step).min(delays.len() - 1)];
                (if n >= delay {
                    i64::from(reference[n - delay]) * 500_000 / 1_000_000
                } else {
                    0
                }) as i16
            })
            .collect()
    }

    fn answers(reference: &[i16], mic: &[i16]) -> Vec<usize> {
        let mut estimator = DelayEstimator::new();
        let mut answered = Vec::new();
        for n in 0..reference.len() {
            if let Some(delay) = estimator.update(mic[n], reference[n]) {
                answered.push(delay);
            }
        }
        answered
    }

    #[test]
    fn a_path_that_jitters_past_the_drift_bound_does_not_move_the_delay() {
        let searches = 14;
        let delays: Vec<usize> = (0..searches)
            .map(|n| if n % 2 == 0 { 20 } else { 23 })
            .collect();
        let reference = reference(HISTORY - 1 + REFRESH_SAMPLES * searches, 8_000, 0x0a11_ce05);
        let mic = moving_path(&reference, REFRESH_SAMPLES, &delays);
        let answered = answers(&reference, &mic);
        assert_eq!(answered.len(), searches, "one answer per search");
        assert!(
            answered.iter().all(|&delay| delay == 20),
            "a path oscillating between 20 and 23 was reported as {answered:?}"
        );
    }

    #[test]
    fn a_path_that_moves_is_followed_only_once_the_searches_agree() {
        const HELD: usize = 5;
        const SETTLED: usize = 9;
        const BOUND: usize = MISSES_BEFORE_MOVING as usize;
        let mut delays = vec![20; HELD];
        delays.extend(vec![40; SETTLED]);
        let reference = reference(
            HISTORY - 1 + REFRESH_SAMPLES * (HELD + SETTLED),
            8_000,
            0x0a11_ce06,
        );
        let mic = moving_path(&reference, REFRESH_SAMPLES, &delays);
        let answered = answers(&reference, &mic);
        assert_eq!(
            &answered[..HELD + BOUND - 1],
            [20; HELD + BOUND - 1],
            "the delay moved before the searches had agreed on a new one: {answered:?}"
        );
        assert_eq!(
            answered[HELD + BOUND - 1],
            40,
            "the path moved to 40 and was never followed: {answered:?}"
        );
        assert!(
            answered[HELD + BOUND..].iter().all(|&delay| delay == 40),
            "the lock did not stay on the new path: {answered:?}"
        );
    }

    #[test]
    fn a_delay_beyond_the_search_is_refused_rather_than_answered_from_noise() {
        let reference = reference(HISTORY + 400, 8_000, 0x1234_9999);
        let mic = echo(&reference, SEARCH + 20, 500_000);
        let (estimator, answered) = settle(&reference, &mic);
        assert_eq!(answered, None, "there is no path in the window to find");
        assert!(!estimator.settled(), "and so nothing has been measured");
    }

    #[test]
    fn a_reference_below_the_power_gate_is_not_searched_at_all() {
        let quiet: Vec<i16> = vec![2; HISTORY + 400];
        let (estimator, answered) = settle(&quiet, &quiet);
        assert_eq!(answered, None, "a 2 LSB reference is under the gate");
        assert!(!estimator.settled());
    }

    #[test]
    fn a_path_further_out_than_the_search_is_still_located() {
        let reference = reference(HISTORY + 400, 8_000, 0x5eed_0001);
        for far in [SEARCH + 8, 120usize, 200] {
            let mic = echo(&reference, far, 500_000);
            let (mut estimator, answered) = settle(&reference, &mic);
            assert_eq!(
                answered, None,
                "a delay of {far} is not found by the search"
            );
            let (lag, score) = estimator.scan_wide(HISTORY - WINDOW);
            assert_eq!(lag, far, "a path of {far} was located at {lag}");
            assert!(
                score >= MIN_CORRELATION,
                "and it is a peak, not noise: {score}"
            );
        }
    }

    #[test]
    fn the_wide_scan_reports_noise_where_there_is_no_path() {
        let path = reference(HISTORY + 400, 8_000, 0x5eed_0003);
        let silence = reference(HISTORY + 400, 8_000, 0x5eed_0004);
        let (mut estimator, answered) = settle(&path, &silence);
        assert_eq!(answered, None);
        let (_, score) = estimator.scan_wide(HISTORY - WINDOW);
        assert!(
            score < MIN_CORRELATION,
            "two unrelated rooms must not read as a located path: {score}"
        );
    }

    #[test]
    fn the_wide_scan_cannot_reach_a_path_further_out_than_its_own_window() {
        let reference = reference(HISTORY + 400, 8_000, 0x5eed_0005);
        let mic = echo(&reference, HISTORY - WINDOW + 8, 500_000);
        let (mut estimator, _) = settle(&reference, &mic);
        let (_, score) = estimator.scan_wide(HISTORY - WINDOW);
        assert!(
            score < MIN_CORRELATION,
            "a path just past the scan's reach must not be found by it: {score}"
        );
    }

    fn room(
        reference: &[i16],
        delay: usize,
        tail: usize,
        tail_ppm: i32,
        halve_every: usize,
    ) -> Vec<i16> {
        (0..reference.len())
            .map(|n| {
                let total: i64 = (0..=tail)
                    .filter(|tap| n >= delay + tap)
                    .map(|tap| {
                        let ppm = if tap == 0 {
                            700_000i32
                        } else {
                            // The shift is clamped because a tail long enough to halve
                            // it more than the width of an `i32` is a shift that panics
                            // in a test — and a test that panics on a long room is a test
                            // that cannot check a long room.
                            tail_ppm >> (tap / halve_every).min(30)
                        };
                        i64::from(reference[n - delay - tap]) * i64::from(ppm) / 1_000_000
                    })
                    .sum();
                total as i16
            })
            .collect()
    }

    fn damped(reference: &[i16], delay: usize, tail: usize) -> Vec<i16> {
        room(reference, delay, tail, 90_000, 48)
    }

    fn live(reference: &[i16], delay: usize, tail: usize) -> Vec<i16> {
        room(reference, delay, tail, 400_000, 4)
    }

    fn shares(estimator: &DelayEstimator) -> [i32; 4] {
        [16, 64, 128, 256].map(|taps| estimator.unreachable_by(taps))
    }

    #[test]
    fn a_longer_filter_never_reaches_less_of_the_room() {
        let reference = reference(HISTORY + 400, 8_000, 0x7000_0001);
        for room in [damped(&reference, 10, 40), live(&reference, 10, 200)] {
            let (mut estimator, _) = settle(&reference, &room);
            let (_, score) = estimator.scan_wide(HISTORY - WINDOW);
            assert!(
                score >= MIN_CORRELATION,
                "a room with a dominant direct reflection has to be found: {score}"
            );
            let shares = shares(&estimator);
            for pair in shares.windows(2) {
                assert!(
                    pair[1] <= pair[0],
                    "a longer filter reached less of the room: {shares:?}"
                );
            }
            for (taps, share) in [16usize, 64, 128, 256].iter().zip(shares) {
                assert!(
                    (0..=100).contains(&share),
                    "{share}% at {taps} taps is not a share of anything"
                );
            }
        }
    }

    #[test]
    fn a_damped_room_leaves_far_less_over_than_a_live_one() {
        // The comparison the tap budget turns on. **Not** "a short room leaves none
        // over": it cannot, because the correlation behind a band is `WINDOW` long and
        // smears a few percent of itself into the bands beyond. Asserting zero would be
        // asserting a resolution this measure does not have; the honest claim is the
        // contrast between the two rooms.
        let reference = reference(HISTORY + 400, 8_000, 0x7000_0002);
        let shares_of = |room: Vec<i16>| {
            let (mut estimator, _) = settle(&reference, &room);
            let (lag, score) = estimator.scan_wide(HISTORY - WINDOW);
            assert!(
                score >= MIN_CORRELATION,
                "a room with a direct reflection: {score}"
            );
            assert_eq!(lag, 10, "the strongest match is the direct reflection");
            shares(&estimator)
        };

        let quiet = shares_of(damped(&reference, 10, 40));
        let loud = shares_of(live(&reference, 10, 200));
        assert!(
            quiet[2] < 10,
            "a room whose reflections die within one band leaves little over: {quiet:?}"
        );
        assert!(
            loud[2] > quiet[2] * 2,
            "a room still reverberating 200 samples later leaves far more: {loud:?} \
         against {quiet:?}"
        );
    }

    #[test]
    fn a_room_that_is_still_loud_past_the_taps_leaves_something_over() {
        let reference = reference(HISTORY + 400, 8_000, 0x7000_0003);
        let (mut estimator, _) = settle(&reference, &live(&reference, 10, 200));
        estimator.scan_wide(HISTORY - WINDOW);
        let shares = shares(&estimator);
        assert!(
            shares[1] > 0,
            "64 taps cannot hold a room still reverberating 200 samples later: {shares:?}"
        );
        assert!(
            shares.iter().any(|share| *share > 0),
            "and some of it is out there: {shares:?}"
        );
    }

    #[test]
    fn the_profile_is_a_banded_measure_and_not_an_impulse_response() {
        let reference = reference(HISTORY + 400, 8_000, 0x7000_0006);
        let (mut estimator, _) = settle(&reference, &damped(&reference, 40, 8));
        estimator.scan_wide(HISTORY - WINDOW);
        let profile = estimator.profile();
        assert_eq!(profile.len(), PROFILE_BANDS);
        let loudest = (0..PROFILE_BANDS)
            .max_by_key(|band| profile[*band])
            .expect("the scan found something to accumulate");
        assert_eq!(
            loudest,
            40 / PROFILE_BAND,
            "a room delayed by 40 samples puts its energy in the band that holds 40"
        );
    }

    #[test]
    fn a_room_with_no_path_is_not_reported_as_one_a_filter_covers_whole() {
        let played = reference(HISTORY + 400, 8_000, 0x7000_0004);
        let unrelated = reference(HISTORY + 400, 8_000, 0x7000_0005);
        let (mut estimator, _) = settle(&played, &unrelated);
        let (_, score) = estimator.scan_wide(HISTORY - WINDOW);
        assert!(
            score < MIN_CORRELATION,
            "two unrelated rooms must not read as a path: {score}"
        );
        assert_eq!(estimator.unreachable_by(16), -1);
        assert_eq!(estimator.unreachable_by(256), -1);
    }

    #[test]
    fn an_unscanned_room_reports_no_share_rather_than_a_perfect_one() {
        let reference = reference(HISTORY + 400, 8_000, 0x7000_0007);
        let (estimator, _) = settle(&reference, &damped(&reference, 10, 20));
        assert_eq!(
            estimator.unreachable_by(16),
            -1,
            "no scan has run, so there is no shape to read a share from"
        );
    }
}
