//! Per-screen frame accounting: how often the panel ships, and what each ship costs.

use esp_hal::time::{Duration, Instant};
use iot_core::stats::windowed_rate;

use crate::components::st7789::Wire;

const FPS_WINDOW_MS: u64 = 500;

const COST_WINDOW_MS: u64 = 2_000;

// How much deeper the stack has to get before the peak is worth another line. The window
// closes every 500 ms and the peak changes only when a page draws deeper, so without a step
// the one number would repeat for the life of the device; a kilobyte is the resolution the
// board's budget is read at.
const STACK_PEAK_STEP_BYTES: usize = 1_024;

pub(super) struct CostSample {
    pub(super) frames: u32,
    pub(super) window: Duration,
    /// Every render this window: what the frame path costs the coordinator's own core. The
    /// transfer is not in here — it runs on the bus's core — so this is the number that says
    /// whether rendering is expensive, and it is the only part of the path that holds the
    /// executor at all. Summed rather than peaked: 11 frames at 3 ms is not 3 ms, and a
    /// window reporting only a peak reads as cheaper than one spending twice as long on
    /// shorter frames.
    pub(super) hold: Duration,
    pub(super) worst_render: Duration,
    /// The longest the coordinator was parked waiting for the bus to answer. Idle time, not
    /// held time — reported because a wait that grows is a panel falling behind, and because
    /// on a build with the bus inline this is the executor time the offload gave back.
    pub(super) worst_wait: Duration,
    pub(super) worst_write: Duration,
    /// The longest single transfer in the window. Kept apart from `worst_write` because the
    /// chunking only moves this one.
    pub(super) worst_chunk: Duration,
}

pub(super) struct Metrics {
    fps: u8,
    fps_frames: u32,
    fps_anchor: Instant,
    cost_frames: u32,
    cost_anchor: Instant,
    worst_render: Duration,
    worst_wait: Duration,
    worst_write: Duration,
    worst_chunk: Duration,
    hold: Duration,
    reported_high_water: usize,
}

impl Metrics {
    pub(super) fn new() -> Self {
        Self {
            fps: 0,
            fps_frames: 0,
            fps_anchor: Instant::now(),
            cost_frames: 0,
            cost_anchor: Instant::now(),
            worst_render: Duration::ZERO,
            worst_wait: Duration::ZERO,
            worst_write: Duration::ZERO,
            worst_chunk: Duration::ZERO,
            hold: Duration::ZERO,
            reported_high_water: 0,
        }
    }

    pub(super) fn fps(&self) -> u8 {
        self.fps
    }

    pub(super) fn fold_frame(
        &mut self,
        render: Duration,
        wire: Wire,
        wait: Duration,
    ) -> Option<CostSample> {
        self.cost_frames += 1;
        if render > self.worst_render {
            self.worst_render = render;
        }
        if wait > self.worst_wait {
            self.worst_wait = wait;
        }
        if wire.total > self.worst_write {
            self.worst_write = wire.total;
        }
        if wire.longest > self.worst_chunk {
            self.worst_chunk = wire.longest;
        }
        self.hold += render;
        let now = Instant::now();
        let window = now - self.cost_anchor;
        if window.as_millis() < COST_WINDOW_MS {
            return None;
        }
        let sample = CostSample {
            frames: self.cost_frames,
            window,
            hold: self.hold,
            worst_render: self.worst_render,
            worst_wait: self.worst_wait,
            worst_write: self.worst_write,
            worst_chunk: self.worst_chunk,
        };
        self.cost_frames = 0;
        self.cost_anchor = now;
        self.worst_render = Duration::ZERO;
        self.worst_wait = Duration::ZERO;
        self.worst_write = Duration::ZERO;
        self.worst_chunk = Duration::ZERO;
        self.hold = Duration::ZERO;
        Some(sample)
    }

    pub(super) fn fold_fps(&mut self) -> bool {
        self.fps_frames += 1;
        let now = Instant::now();
        let window = now - self.fps_anchor;
        if window.as_millis() < FPS_WINDOW_MS {
            return false;
        }
        self.fps = windowed_rate(self.fps_frames, window.as_millis());
        self.fps_frames = 0;
        self.fps_anchor = now;
        true
    }

    pub(super) fn fold_stack_peak(&mut self, high: usize) -> bool {
        if high
            < self
                .reported_high_water
                .saturating_add(STACK_PEAK_STEP_BYTES)
        {
            return false;
        }
        self.reported_high_water = high;
        true
    }
}
