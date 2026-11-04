//! The camera feed bound to this screen's frame buffer.

use alloc::boxed::Box;
use core::any::Any;

use esp_hal::time::Instant;
use iot_core::demand::CAMERA_THUMBNAIL_PERIOD_MS;
use iot_core::drivers::camera::{CameraSnapshot, CameraTarget, FrameAdvance, FrameSource};
use iot_core::drivers::surface::{PanelPainter, PanelTarget};

use crate::stack::note_stack_high_water;

use super::*;

const THUMBNAIL_INTERVAL_MS: u64 = CAMERA_THUMBNAIL_PERIOD_MS as u64;

pub(super) struct CameraFeed {
    source: Option<Box<dyn FrameSource + Send>>,
    thumbnail_last_ms: Option<u64>,
}

impl CameraFeed {
    pub(super) fn new() -> Self {
        Self {
            source: None,
            thumbnail_last_ms: None,
        }
    }

    pub(super) fn attach(&mut self, camera: Box<dyn FrameSource + Send>) {
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
    ) -> Option<CameraSnapshot> {
        let source = self.source.as_mut()?;
        let advance = source.advance(frame, now_ms);
        let fault = source.fault();
        if fault.is_none() && advance != FrameAdvance::Fresh {
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
        Some(CameraSnapshot {
            counters: source.counters(),
            geometry: source.geometry(),
            starting: false,
            fault,
        })
    }
}

impl CameraTarget for Screen {
    fn paint_camera(
        &mut self,
        now_ms: u64,
        period_ms: u32,
        painter: &dyn PanelPainter,
        snapshot: &dyn Any,
    ) -> bool {
        Screen::paint_camera(self, now_ms, period_ms, painter, snapshot)
    }

    fn attach_camera(&mut self, camera: Box<dyn FrameSource + Send>) {
        self.camera.attach(camera);
    }

    fn release_camera(&mut self) {
        if self.owner.is_camera() {
            self.camera.pause();
            self.owner.released();
        }
    }
}

impl Screen {
    fn paint_camera(
        &mut self,
        now_ms: u64,
        period_ms: u32,
        painter: &dyn PanelPainter,
        snapshot: &dyn Any,
    ) -> bool {
        let bytes = self.panel.frame_bytes();
        let width = usize::from(self.panel.width());
        let height = usize::from(self.panel.height());
        let context = self.context();
        let frame = &mut self.frame[..bytes];

        if !self.owner.is_camera() {
            if !self.camera.resume(frame) {
                return false;
            }
            self.owner.taken();
        }
        let Some(readout) = self.camera.advance(frame, now_ms, period_ms) else {
            return false;
        };

        let render_start = Instant::now();
        painter.paint_camera(
            PanelTarget {
                frame,
                width,
                height,
                ground: None,
            },
            readout,
            context,
            snapshot,
        );
        let render = render_start.elapsed();
        self.pending = Some(Pending {
            camera: true,
            render,
        });
        // Sampled here because the camera page reaches the panel through this path and nowhere
        // else: without it the reading is `0 B` on this page for the whole run, which says
        // nothing about the stack. `repaint` covers the light pages and the audio page samples
        // its own working set through the surface's probe.
        note_stack_high_water();
        true
    }
}
