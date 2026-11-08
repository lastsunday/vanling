//! The coordinator's clock.

use embassy_time::{Duration, Instant};

pub const STEP_MS: u32 = 20;

/// How far behind the wall clock the frame grid may run before the missed steps are dropped.
///
/// A frame that overran by a second has no useful intermediate states to draw; replaying all
/// fifty makes the catch-up burst longer than the stall that caused it.
pub const MAX_CATCH_UP_MS: u32 = STEP_MS * 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    At(Instant),
    Park,
}

pub struct FrameClock {
    now_ms: u32,
    next: Instant,
}

impl FrameClock {
    pub const fn new(now: Instant) -> Self {
        Self {
            now_ms: 0,
            next: now,
        }
    }

    pub const fn elapsed_ms(&self) -> u32 {
        self.now_ms
    }

    /// Ends the frame that just ran and names when the next one is due.
    ///
    /// `now` is read after the frame's work, not before it: that is the moment the grid fell
    /// behind. Lateness inside [`MAX_CATCH_UP_MS`] still replays every step, so short overruns
    /// keep their intermediate animation frames. A longer stall drops the steps past the bound
    /// and advances `elapsed_ms` by what was dropped, so animation time stays aligned with the
    /// wall clock instead of slowing down for the rest of the run.
    pub fn tick(&mut self, animating: bool, now: Instant) -> Wait {
        self.now_ms = self.now_ms.wrapping_add(STEP_MS);
        if !animating {
            return Wait::Park;
        }
        let late_ms = now.saturating_duration_since(self.next).as_millis() as u32;
        if late_ms > MAX_CATCH_UP_MS {
            let skipped_ms = late_ms - MAX_CATCH_UP_MS;
            self.now_ms = self.now_ms.wrapping_add((skipped_ms / STEP_MS) * STEP_MS);
            self.next = now
                .checked_sub(Duration::from_millis(u64::from(MAX_CATCH_UP_MS)))
                .unwrap_or(now);
        }
        self.next += Duration::from_millis(STEP_MS.into());
        Wait::At(self.next)
    }

    pub fn reanchor(&mut self, now: Instant) {
        self.next = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn at(ms: u64) -> Instant {
        Instant::from_millis(ms)
    }

    #[test]
    fn the_first_frame_is_drawn_at_zero_and_the_next_is_one_step_away() {
        let mut clock = FrameClock::new(at(1_000));
        assert_eq!(clock.elapsed_ms(), 0);
        assert_eq!(clock.tick(true, at(1_000)), Wait::At(at(1_020)));
        assert_eq!(clock.elapsed_ms(), STEP_MS);
    }

    #[test]
    fn animating_passes_stay_on_the_grid_however_late_the_work_is() {
        let mut clock = FrameClock::new(at(1_000));
        assert_eq!(clock.tick(true, at(1_000)), Wait::At(at(1_020)));
        assert_eq!(clock.tick(true, at(1_020)), Wait::At(at(1_040)));
        assert_eq!(clock.tick(true, at(1_040)), Wait::At(at(1_060)));
        assert_eq!(clock.elapsed_ms(), 3 * STEP_MS);
    }

    #[test]
    fn a_parked_pass_asks_to_park_and_re_anchoring_puts_the_next_frame_after_the_wake() {
        let mut clock = FrameClock::new(at(1_000));
        assert_eq!(clock.tick(false, at(1_000)), Wait::Park);
        assert_eq!(clock.elapsed_ms(), STEP_MS);
        clock.reanchor(at(5_000));
        assert_eq!(clock.tick(true, at(5_000)), Wait::At(at(5_020)));
    }

    #[test]
    fn a_frame_within_the_bound_still_replays_its_skipped_steps() {
        let mut clock = FrameClock::new(at(1_000));
        let _ = clock.tick(true, at(1_000));
        let _ = clock.tick(true, at(1_000));
        let now = at(1_100);
        assert!(
            now > at(1_040),
            "the deadline this pass inherited is behind"
        );
        assert_eq!(
            clock.tick(true, now),
            Wait::At(at(1_060)),
            "one step past the inherited deadline, so the skipped steps replay"
        );
        assert_eq!(clock.elapsed_ms(), 3 * STEP_MS);
    }

    #[test]
    fn a_stall_past_the_bound_drops_the_extra_steps_and_keeps_frame_time_on_the_wall_clock() {
        let mut clock = FrameClock::new(at(1_000));
        let _ = clock.tick(true, at(1_000));
        let _ = clock.tick(true, at(1_000));
        let stalled_at = at(9_000);

        assert_eq!(
            clock.tick(true, stalled_at),
            Wait::At(at(8_920)),
            "only the bound's worth of backlog is left to replay"
        );
        assert_eq!(clock.elapsed_ms(), 7_920);

        let mut wait = Wait::Park;
        for _ in 0..(MAX_CATCH_UP_MS / STEP_MS - 1) {
            wait = clock.tick(true, stalled_at);
        }
        assert_eq!(
            clock.elapsed_ms(),
            8_000,
            "the dropped steps advanced frame time, so animation does not run slow"
        );
        assert_eq!(
            wait,
            Wait::At(at(9_000)),
            "the next frame is the one the wall clock is on"
        );
    }
}
