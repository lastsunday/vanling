//! The render loop's frame clock.

use embassy_time::{Duration, Instant};

pub const STEP_MS: u32 = 20;

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

    pub fn tick(&mut self, animating: bool) -> Wait {
        // Logical time advances through a park: a light caught mid-breath must not freeze when
        // the panel settles.
        self.now_ms = self.now_ms.wrapping_add(STEP_MS);
        if !animating {
            return Wait::Park;
        }
        // Absolute, not `now + step`: `Timer::at` returns at once for a deadline already behind,
        // so a late frame is replayed step by step rather than clamped.
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
        assert_eq!(clock.tick(true), Wait::At(at(1_020)));
        assert_eq!(clock.elapsed_ms(), STEP_MS);
    }

    #[test]
    fn animating_passes_stay_on_the_grid_however_late_the_work_is() {
        let mut clock = FrameClock::new(at(1_000));
        assert_eq!(clock.tick(true), Wait::At(at(1_020)));
        assert_eq!(clock.tick(true), Wait::At(at(1_040)));
        assert_eq!(clock.tick(true), Wait::At(at(1_060)));
        assert_eq!(clock.elapsed_ms(), 3 * STEP_MS);
    }

    #[test]
    fn a_parked_pass_asks_to_park_and_re_anchoring_puts_the_next_frame_after_the_wake() {
        let mut clock = FrameClock::new(at(1_000));
        assert_eq!(clock.tick(false), Wait::Park);
        assert_eq!(clock.elapsed_ms(), STEP_MS);
        clock.reanchor(at(5_000));
        assert_eq!(clock.tick(true), Wait::At(at(5_020)));
    }

    #[test]
    fn a_frame_that_took_too_long_is_replayed_rather_than_clamped() {
        let mut clock = FrameClock::new(at(1_000));
        let _ = clock.tick(true);
        let _ = clock.tick(true);
        let now = at(9_000);
        assert!(
            now > at(1_040),
            "the deadline this pass inherited is behind"
        );
        assert_eq!(
            clock.tick(true),
            Wait::At(at(1_060)),
            "one step past the inherited deadline, so the skipped steps replay"
        );
    }
}
