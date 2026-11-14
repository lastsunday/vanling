//! The frame pacer: the loop's clocks, and the deadline it sleeps to.

use embassy_time::{Duration, Instant};

pub const STEP_MS: u32 = 20;

/// How far behind its deadline a frame may fall before the backlog is dropped.
///
/// Five steps. Catching up is drawing, so replaying more than this is itself a visible freeze;
/// inside the bound every missed step is replayed, past it the rest are skipped and frame time
/// jumps with them.
pub const MAX_CATCH_UP_MS: u32 = STEP_MS * 5;

/// What the loop does after a frame: sleep to the next deadline, or wait for a wake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// The next frame is due then. Nothing else is awaited: a boundary is at most one step away.
    Until(Instant),
    /// Nothing wants a timed frame; wait for an intent or a worker's wake.
    Park,
}

/// The loop's timing: the frame clock renderers read, and the deadline it sleeps to.
///
/// A pass advances frame time by one [`STEP_MS`] however long its work took. With something
/// active the pacer is periodic at that step; with nothing to animate it parks, and the loop
/// waits for an intent or a worker's wake instead.
pub struct FramePacer {
    frame_ms: u32,
    deadline: Instant,
}

impl FramePacer {
    pub const fn new(now: Instant) -> Self {
        Self {
            frame_ms: 0,
            deadline: now,
        }
    }

    pub const fn frame_ms(&self) -> u32 {
        self.frame_ms
    }

    /// Ends the frame that just ran and answers what the loop waits for next.
    ///
    /// `now` is read after the frame's work, so it is the moment the pacer fell behind. Lateness
    /// inside [`MAX_CATCH_UP_MS`] replays every step, so a short overrun keeps its intermediate
    /// frames; past the bound the rest is skipped and frame time advances by what was skipped,
    /// keeping animation on the wall clock instead of running slow for the rest of the boot.
    pub fn end_frame(&mut self, active: bool, now: Instant) -> Wait {
        self.frame_ms = self.frame_ms.wrapping_add(STEP_MS);
        if !active {
            return Wait::Park;
        }
        let late_ms = now.saturating_duration_since(self.deadline).as_millis() as u32;
        if late_ms > MAX_CATCH_UP_MS {
            let skipped_ms = late_ms - MAX_CATCH_UP_MS;
            self.frame_ms = self.frame_ms.wrapping_add((skipped_ms / STEP_MS) * STEP_MS);
            self.deadline = now
                .checked_sub(Duration::from_millis(u64::from(MAX_CATCH_UP_MS)))
                .unwrap_or(now);
        }
        self.deadline += Duration::from_millis(STEP_MS.into());
        Wait::Until(self.deadline)
    }

    pub fn reanchor(&mut self, now: Instant) {
        self.deadline = now;
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
        let mut pacer = FramePacer::new(at(1_000));
        assert_eq!(pacer.frame_ms(), 0);
        assert_eq!(pacer.end_frame(true, at(1_000)), Wait::Until(at(1_020)));
        assert_eq!(pacer.frame_ms(), STEP_MS);
    }

    #[test]
    fn active_passes_stay_on_the_grid_however_late_the_work_is() {
        let mut pacer = FramePacer::new(at(1_000));
        assert_eq!(pacer.end_frame(true, at(1_000)), Wait::Until(at(1_020)));
        assert_eq!(pacer.end_frame(true, at(1_020)), Wait::Until(at(1_040)));
        assert_eq!(pacer.end_frame(true, at(1_040)), Wait::Until(at(1_060)));
        assert_eq!(pacer.frame_ms(), 3 * STEP_MS);
    }

    #[test]
    fn a_parked_pass_asks_to_park_and_re_anchoring_puts_the_next_frame_after_the_wake() {
        let mut pacer = FramePacer::new(at(1_000));
        assert_eq!(pacer.end_frame(false, at(1_000)), Wait::Park);
        assert_eq!(pacer.frame_ms(), STEP_MS);
        pacer.reanchor(at(5_000));
        assert_eq!(pacer.end_frame(true, at(5_000)), Wait::Until(at(5_020)));
    }

    #[test]
    fn a_frame_within_the_bound_still_replays_its_skipped_steps() {
        let mut pacer = FramePacer::new(at(1_000));
        let _ = pacer.end_frame(true, at(1_000));
        let _ = pacer.end_frame(true, at(1_000));
        let now = at(1_100);
        assert!(
            now > at(1_040),
            "the deadline this pass inherited is behind"
        );
        assert_eq!(
            pacer.end_frame(true, now),
            Wait::Until(at(1_060)),
            "one step past the inherited deadline, so the skipped steps replay"
        );
        assert_eq!(pacer.frame_ms(), 3 * STEP_MS);
    }

    #[test]
    fn a_stall_past_the_bound_drops_the_extra_steps_and_keeps_frame_time_on_the_wall_clock() {
        let mut pacer = FramePacer::new(at(1_000));
        let _ = pacer.end_frame(true, at(1_000));
        let _ = pacer.end_frame(true, at(1_000));
        let stalled_at = at(9_000);

        assert_eq!(
            pacer.end_frame(true, stalled_at),
            Wait::Until(at(8_920)),
            "only the bound's worth of backlog is left to replay"
        );
        assert_eq!(pacer.frame_ms(), 7_920);

        let mut wait = Wait::Park;
        for _ in 0..(MAX_CATCH_UP_MS / STEP_MS - 1) {
            wait = pacer.end_frame(true, stalled_at);
        }
        assert_eq!(
            pacer.frame_ms(),
            8_000,
            "the dropped steps advanced frame time, so animation does not run slow"
        );
        assert_eq!(
            wait,
            Wait::Until(at(9_000)),
            "the next frame is the one the wall clock is on"
        );
    }
}
