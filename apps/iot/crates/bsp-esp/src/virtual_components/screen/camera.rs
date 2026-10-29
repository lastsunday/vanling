//! The camera feed bound to this screen's frame buffer.
//!
//! A sensor is not a display part: it is a second writer of the same bytes, and the buffer
//! is the screen's. So the feed sits beside [`Panel`](super::panel::Panel) rather than inside
//! it — the glass has no camera — and the owner bookkeeping stays with the screen, because
//! that is where the buffer is. Nothing here draws a page; the portable surface stamps the
//! readout over the frame this module reports.

use alloc::boxed::Box;

use esp_hal::time::Instant;
use iot_core::demand::{CAMERA_THUMBNAIL_PERIOD_MS, DemandKey};
use iot_core::drivers::camera::{CameraTarget, FrameAdvance, FrameSource};
use iot_ui::{CameraReadout, Canvas, Surface};

use crate::components::es7210::SPL_OFFSET_DECIBELS;
use crate::stack::note_stack_high_water;

use super::*;

/// How often a frame is written to the panel under a thumbnail request (a camera demand at
/// [`CAMERA_THUMBNAIL_PERIOD_MS`] or slower). The whole frame write is what the thumbnail
/// request exists to avoid.
const THUMBNAIL_INTERVAL_MS: u64 = CAMERA_THUMBNAIL_PERIOD_MS as u64;

/// A sensor bound to the screen's buffer, with the clock that throttles its page.
pub(super) struct CameraFeed {
    /// Absent means the page never appears; it is an `Option` rather than a required
    /// parameter so a board with no sensor needs no branch anywhere else.
    source: Option<Box<dyn FrameSource>>,
    /// When the panel last wrote a camera frame. `None` before the first one, which is what
    /// lets the first frame through without a warm-up.
    thumbnail_last_ms: Option<u64>,
}

impl CameraFeed {
    pub(super) fn new() -> Self {
        Self {
            source: None,
            thumbnail_last_ms: None,
        }
    }

    pub(super) fn attach(&mut self, camera: Box<dyn FrameSource>) {
        self.source = Some(camera);
    }

    /// Stops the chain and leaves the buffer to the CPU.
    pub(super) fn pause(&mut self) {
        if let Some(source) = self.source.as_mut() {
            source.pause();
        }
    }

    /// Gives `frame` back to the camera. `false` when none is bound.
    pub(super) fn resume(&mut self, frame: &mut [u8]) -> bool {
        let Some(source) = self.source.as_mut() else {
            return false;
        };
        source.resume(frame);
        true
    }

    /// Advances the sensor and returns the readout to stamp when a whole frame is due and
    /// the thumbnail clock allows it.
    pub(super) fn advance(
        &mut self,
        frame: &mut [u8],
        now_ms: u64,
        period_ms: u32,
    ) -> Option<CameraReadout> {
        let source = self.source.as_mut()?;
        if source.advance(frame, now_ms) != FrameAdvance::Fresh {
            return None;
        }
        // `advance` still runs (it is what repairs the chain), so a throttled camera leaves
        // the picture fresh without shipping it.
        if period_ms >= CAMERA_THUMBNAIL_PERIOD_MS {
            let due = self
                .thumbnail_last_ms
                .is_none_or(|last| now_ms.saturating_sub(last) >= THUMBNAIL_INTERVAL_MS);
            if !due {
                return None;
            }
            self.thumbnail_last_ms = Some(now_ms);
        }
        Some(CameraReadout {
            counters: source.counters(),
            geometry: source.geometry(),
            starting: false,
        })
    }
}

/// How the app hands this surface a camera, and how a surface with none says so: with nothing
/// bound `paint_camera` answers `false`, which a board with no sensor is the ordinary case for.
impl CameraTarget for Screen {
    fn paint_camera(&mut self, now_ms: u64) -> bool {
        Screen::paint_camera(self, now_ms)
    }

    fn attach_camera(&mut self, camera: Box<dyn FrameSource>) {
        self.camera.attach(camera);
    }
}

impl Screen {
    /// Advances the camera and, if a whole frame arrived, stamps the readout over it and
    /// ships the panel. Returns whether this pass painted.
    ///
    /// The frame is not copied: the frame the camera just filled is the buffer the panel
    /// reads, which is why they are the same bytes. The ordering is the load-bearing part —
    /// `advance` reports a frame and does not re-arm until the next call, so nothing writes
    /// the buffer between it being reported and the panel being given it, and the readout is
    /// stamped while the DMA is idle.
    fn paint_camera(&mut self, now_ms: u64) -> bool {
        // Each field is borrowed where it is needed rather than copied out, and the borrows
        // are of disjoint fields so they hold at once. The snapshot is the reason: it embeds a
        // 1.6 KB audio envelope, and copying it here would put a kilobyte and a half on a stack
        // this path is already close to the floor of — see the note on `paint`.
        let bytes = self.panel.frame_bytes();
        let width = usize::from(self.panel.width());
        let height = usize::from(self.panel.height());
        let frame = &mut self.frame[..bytes];

        // Parked by the last page change, so this is where it comes back — with the chain it
        // handed back, over a buffer whose contents were a colour fill until now.
        if !self.owner.is_camera() {
            if !self.camera.resume(frame) {
                return false;
            }
            self.owner.taken();
        }
        let period_ms = self.diagnostics.demand.claim(DemandKey::CAMERA).period_ms;
        let Some(readout) = self.camera.advance(frame, now_ms, period_ms) else {
            return false;
        };

        let render_start = Instant::now();
        {
            let canvas = Canvas::new(frame, width, height, None);
            let mut surface = Surface::new(
                canvas,
                &self.diagnostics,
                self.instance,
                self.metrics.fps(),
                DEBUG_DIAGNOSTICS,
                SPL_OFFSET_DECIBELS,
            );
            surface.stamp_camera(readout);
        }
        let render = render_start.elapsed();

        let write_start = Instant::now();
        if let Err(error) = self.panel.write(frame) {
            log::error!("[DISPLAY] camera frame write failed: {error:?}");
        } else {
            self.panel.note_frame_shipped();
        }
        self.account_frame(render, write_start.elapsed());
        // Sampled here because the camera page reaches the panel through this path and nowhere
        // else: without it the reading is `0 B` on this page for the whole run, which says
        // nothing about the stack. `consume` covers the light pages and the audio page samples
        // its own working set through the surface's probe.
        note_stack_high_water();
        true
    }
}
