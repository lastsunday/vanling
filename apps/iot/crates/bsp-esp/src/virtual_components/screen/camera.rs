//! The camera feed bound to this screen's frame buffer.

use alloc::boxed::Box;

use esp_hal::time::Instant;
use iot_core::demand::{CAMERA_THUMBNAIL_PERIOD_MS, DemandKey};
use iot_core::drivers::camera::{CameraTarget, FrameAdvance, FrameSource};
use iot_ui::{CameraReadout, Canvas, Surface};

use crate::components::es7210::SPL_OFFSET_DECIBELS;
use crate::stack::note_stack_high_water;

use super::*;

const THUMBNAIL_INTERVAL_MS: u64 = CAMERA_THUMBNAIL_PERIOD_MS as u64;

pub(super) struct CameraFeed {
    source: Option<Box<dyn FrameSource>>,
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

    pub(super) fn pause(&mut self) {
        if let Some(source) = self.source.as_mut() {
            source.pause();
        }
    }

    pub(super) fn resume(&mut self, frame: &mut [u8]) -> bool {
        let Some(source) = self.source.as_mut() else {
            return false;
        };
        source.resume(frame);
        true
    }

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

impl CameraTarget for Screen {
    fn takes_frames(&self) -> bool {
        true
    }

    fn paint_camera(&mut self, now_ms: u64) -> bool {
        Screen::paint_camera(self, now_ms)
    }

    fn attach_camera(&mut self, camera: Box<dyn FrameSource>) {
        self.camera.attach(camera);
    }
}

impl Screen {
    fn paint_camera(&mut self, now_ms: u64) -> bool {
        let bytes = self.panel.frame_bytes();
        let width = usize::from(self.panel.width());
        let height = usize::from(self.panel.height());
        let frame = &mut self.frame[..bytes];

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
