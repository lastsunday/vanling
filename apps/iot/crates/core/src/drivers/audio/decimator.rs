//! Capture-rate frames down to the rate the canceller works at.
//!
//! The codec runs at [`SAMPLE_RATE_HZ`] and Espressif's AEC takes 16 kHz and
//! nothing else, so something has to throw two frames out of every three. Which
//! two is not free: the microphone and the reference are numbered by their own
//! transfers' frame counters, and a pair taken from different frames of the two
//! groups is up to two thirds of a sample apart — a shift the correlation peak
//! cannot sit on, so the canceller would be decorrelating itself from the very
//! signal it exists to predict. [`Decimator::with_phase`] is how a caller pins
//! the two to the same frame of each group.
//!
//! In core because a mismatch here is silent: every number the canceller reports
//! stays plausible while the two streams are a fraction of a sample apart, and
//! nothing on the board can show it.
use super::SAMPLE_RATE_HZ;
use crate::drivers::aec::SAMPLE_RATE_HZ as CANCELLER_HZ;

/// Frames of capture one canceller sample spans: 48 kHz down to 16 kHz.
pub const DECIMATION: u32 = SAMPLE_RATE_HZ / CANCELLER_HZ;

const _: () = assert!(
    SAMPLE_RATE_HZ.is_multiple_of(CANCELLER_HZ),
    "the canceller's rate has to divide the codec's, or the dropped frames are not a whole number"
);

const _: () = assert!(DECIMATION >= 2, "a decimator that drops nothing is not one");

/// Keeps one frame in every [`DECIMATION`], counting down rather than taking a remainder: a
/// remainder on a running counter is a divide per frame, and this runs once per frame for
/// as long as the capture has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decimator {
    phase: u8,
    countdown: u32,
}

impl Decimator {
    /// Keeps every frame whose number is a multiple of [`DECIMATION`].
    pub const fn new() -> Self {
        Self::with_phase(0)
    }

    /// Keeps every frame whose number is congruent to `phase`.
    pub const fn with_phase(phase: u8) -> Self {
        let phase = phase % DECIMATION as u8;
        Self {
            phase,
            countdown: phase as u32,
        }
    }

    /// Which frame of each group is kept, as the number of frames already consumed at which
    /// the first one falls.
    pub const fn phase(&self) -> u8 {
        self.phase
    }

    /// Folds one frame in and hands back the sample to keep, or `None` for a frame this one
    /// drops.
    pub fn push(&mut self, sample: i16) -> Option<i16> {
        if self.countdown == 0 {
            self.countdown = DECIMATION - 1;
            Some(sample)
        } else {
            self.countdown -= 1;
            None
        }
    }

    /// How many of the next `count` frames this decimator keeps, which is the length a
    /// caller has to have ready before it starts folding.
    pub const fn kept_in(&self, count: usize) -> usize {
        let group = DECIMATION as usize;
        if count <= self.countdown as usize {
            return 0;
        }
        1 + (count - self.countdown as usize - 1) / group
    }
}

impl Default for Decimator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn kept(phase: u8, count: usize) -> Vec<usize> {
        let mut decimator = Decimator::with_phase(phase);
        (0..count)
            .filter(|&frame| decimator.push(frame as i16).is_some())
            .collect()
    }

    #[test]
    fn three_frames_come_out_of_every_four_eighteen() {
        assert_eq!(DECIMATION, 3);
        assert_eq!(SAMPLE_RATE_HZ, 48_000);
        assert_eq!(CANCELLER_HZ, 16_000);
    }

    #[test]
    fn the_kept_frames_are_exactly_one_in_every_group() {
        for phase in 0..DECIMATION as u8 {
            assert_eq!(
                kept(phase, 300),
                (0..300)
                    .filter(|frame| frame % DECIMATION as usize == phase as usize)
                    .collect::<Vec<_>>(),
                "phase {phase} kept the wrong frames"
            );
        }
    }

    #[test]
    fn two_phases_that_agree_keep_the_same_frame_of_each_group() {
        let same = kept(0, 300);
        for phase in 1..DECIMATION as u8 {
            let other = kept(phase, 300);
            assert!(
                same.iter().zip(&other).all(|(a, b)| a != b),
                "phase {phase} kept frames phase 0 also kept"
            );
        }
    }

    #[test]
    fn a_phase_past_the_group_is_the_same_phase() {
        assert_eq!(
            Decimator::with_phase(DECIMATION as u8),
            Decimator::with_phase(0)
        );
        assert_eq!(
            Decimator::with_phase(DECIMATION as u8 + 1),
            Decimator::with_phase(1)
        );
    }

    #[test]
    fn the_sample_that_comes_back_is_the_frame_that_was_kept() {
        let mut decimator = Decimator::with_phase(2);
        let kept_values: Vec<i16> = (0..9)
            .filter_map(|frame| decimator.push(frame as i16))
            .collect();
        assert_eq!(kept_values, vec![2, 5, 8]);
    }

    #[test]
    fn the_count_a_caller_prepares_for_is_the_count_it_gets() {
        for phase in 0..DECIMATION as u8 {
            for count in 0..40usize {
                let decimator = Decimator::with_phase(phase);
                assert_eq!(
                    decimator.kept_in(count),
                    kept(phase, count).len(),
                    "phase {phase} over {count} frames"
                );
            }
        }
    }

    #[test]
    fn a_count_holds_at_every_point_in_the_cycle() {
        // The drain is bounded by bytes rather than by frames, so a poll can ask
        // for a window that starts anywhere in the group. Walk the whole cycle
        // and check the prediction against what folding the same window
        // actually keeps, from whatever state the decimator is in at that point
        // — a check against a freshly reset decimator would only ever test
        // phase 0.
        let total = 300usize;
        let mut walker = Decimator::new();
        for frame in 0..total {
            let remaining = total - frame;
            let mut probe = walker.clone();
            let actually = (0..remaining).filter(|_| probe.push(0).is_some()).count();
            assert_eq!(
                walker.kept_in(remaining),
                actually,
                "asked for {remaining} frames starting at {frame}"
            );
            walker.push(0);
        }
    }

    #[test]
    fn one_in_every_three_over_a_long_run() {
        let mut decimator = Decimator::new();
        let kept = (0..300).filter(|_| decimator.push(0).is_some()).count();
        assert_eq!(kept, 100, "300 frames at one in three");
    }
}
