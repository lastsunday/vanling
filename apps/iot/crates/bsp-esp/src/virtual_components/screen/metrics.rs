//! Per-screen frame accounting: how often the panel ships, and what each ship costs.
//!
//! The two costs are kept apart because they have opposite fixes: a slow render is CPU the
//! shared executor does not get back, while a slow write is GDMA arbitration the playback DMA
//! has to win. The repaint count is the point as much as the peaks are: it is what says
//! whether a page predicate is actually holding the blocking duty cycle down, or only
//! appears to.

use esp_hal::time::{Duration, Instant};
use iot_core::render::windowed_rate;

/// On-panel repaint-rate sampling window.
const FPS_WINDOW_MS: u64 = 500;

/// Frame-cost sampling window.
const COST_WINDOW_MS: u64 = 2_000;

/// How much deeper the stack has to get before the peak is worth another line. The window
/// closes every 500 ms and the peak changes only when a page draws deeper, so without a step
/// the one number would repeat for the life of the device; a kilobyte is the resolution the
/// board's budget is read at.
const STACK_PEAK_STEP_BYTES: usize = 1_024;

/// One closed cost window, as the log line reports it.
pub(super) struct CostSample {
    pub(super) frames: u32,
    pub(super) window: Duration,
    /// Every render-plus-write this window. The peaks cannot be summed into it:
    /// 11 frames at 16 ms is not 16 ms, and a window reporting only a peak reads as cheaper
    /// than one spending twice as long on shorter frames.
    pub(super) hold: Duration,
    pub(super) worst_render: Duration,
    pub(super) worst_write: Duration,
}

/// The screen's frame counters: the repaint rate and the current window's cost peaks.
pub(super) struct Metrics {
    fps: u8,
    fps_frames: u32,
    fps_anchor: Instant,
    cost_frames: u32,
    cost_anchor: Instant,
    worst_render: Duration,
    worst_write: Duration,
    hold: Duration,
    /// The deepest stack the log has already reported, so a peak is news once rather than
    /// once per rate window.
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
            worst_write: Duration::ZERO,
            hold: Duration::ZERO,
            reported_high_water: 0,
        }
    }

    /// Panel repaint rate over the last window, `0` while nothing repaints.
    pub(super) fn fps(&self) -> u8 {
        self.fps
    }

    /// Folds one frame's two costs into the current window, returning the sample once the
    /// window closes.
    pub(super) fn fold_frame(&mut self, render: Duration, write: Duration) -> Option<CostSample> {
        self.cost_frames += 1;
        if render > self.worst_render {
            self.worst_render = render;
        }
        if write > self.worst_write {
            self.worst_write = write;
        }
        self.hold += render + write;
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
            worst_write: self.worst_write,
        };
        self.cost_frames = 0;
        self.cost_anchor = now;
        self.worst_render = Duration::ZERO;
        self.worst_write = Duration::ZERO;
        self.hold = Duration::ZERO;
        Some(sample)
    }

    /// Counts one shipped frame; returns whether the rate window closed.
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

    /// Whether `high` is a peak the log has not reported yet, by at least
    /// [`STACK_PEAK_STEP_BYTES`]. A first sample of zero is not a depth, so it is not news.
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
