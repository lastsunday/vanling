//! The Camera page: the sensor's frame, and the readout drawn over it.

use iot_core::drivers::camera::CameraSnapshot;
use iot_core::overlay::{Block, block_value_x, block_x, overlay_row_top};
use iot_core::paint::Rgb;

use crate::shared::intent::{BusinessIntent, InputIntent};
use crate::shared::kit::surface::Surface;
use crate::shared::kit::text::{
    format_pair, format_u32, frame_fingerprint, light_mode_word, write_ratio,
};
use crate::shared::model::DeviceState;

/// The camera page draws the sensor's frames and its own readout over them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CameraPage;

pub(super) const GROUND: Option<Rgb> = Some(Rgb(0, 0, 0));
pub(super) const CAMERA_FRAMES: bool = true;
pub(super) const REPAINT_MS: u64 = super::REPAINT_FLOOR_MS;

/// A frame arriving is a data event and not anything a snapshot diff can see. Forcing a
/// repaint here would ship a full frame that is a copy of the last one — ~15 ms of blocking
/// SPI.
pub(super) const REPAINTS: super::Repaint = super::Repaint::Never;

impl super::Page for CameraPage {}

impl super::LocalPage for CameraPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> BusinessIntent {
        super::lamp(op, state)
    }
}

pub(super) fn paint(surface: &mut Surface<'_>) {
    surface.paint_camera(CameraSnapshot::STARTING);
}

impl Surface<'_> {
    /// The camera's own frame, after the sensor has filled it.
    pub(crate) fn paint_camera(&mut self, readout: CameraSnapshot) {
        let fault = readout.fault;
        let fingerprint =
            (!readout.starting && fault.is_none()).then(|| frame_fingerprint(self.frame()));

        self.draw_text(b"CAMERA", block_x(Block::First), overlay_row_top(0));
        match (fault, readout.starting) {
            (Some(_), _) => {
                self.draw_text(b"NO CAM", block_value_x(Block::First), overlay_row_top(0))
            }
            (None, true) => {
                self.draw_text(b"WAIT", block_value_x(Block::First), overlay_row_top(0))
            }
            (None, false) => {}
        }
        if fault.is_none() {
            if let Some(geometry) = readout.geometry {
                let ratio = &mut [0u8; 6];
                let ratio = write_ratio(ratio, geometry.subsample);
                self.draw_text(b"WIN", block_x(Block::First), overlay_row_top(1));
                self.draw_text(ratio, block_value_x(Block::First), overlay_row_top(1));
                let out = &mut [0u8; 12];
                let out = format_pair(Some((geometry.out_width, geometry.out_height)), out);
                self.draw_text(b"OUT", block_x(Block::First), overlay_row_top(2));
                self.draw_text(out, block_value_x(Block::First), overlay_row_top(2));
                let read = &mut [0u8; 12];
                let read = format_pair(Some((geometry.win_width, geometry.win_height)), read);
                self.draw_text(b"RD", block_x(Block::First), overlay_row_top(3));
                self.draw_text(read, block_value_x(Block::First), overlay_row_top(3));
            } else {
                self.draw_text(
                    if readout.starting { b"----" } else { b"NOBUS" },
                    block_value_x(Block::First),
                    overlay_row_top(1),
                );
            }

            let frames = &mut [0u8; 12];
            let frames = format_u32(readout.counters.frames, frames);
            self.draw_text(b"FRM", block_x(Block::Second), overlay_row_top(0));
            self.draw_text(frames, block_value_x(Block::Second), overlay_row_top(0));
            let restarts = &mut [0u8; 12];
            let restarts = format_u32(readout.counters.restarts, restarts);
            self.draw_text(b"RM", block_x(Block::Second), overlay_row_top(1));
            self.draw_text(restarts, block_value_x(Block::Second), overlay_row_top(1));
            let live = self.diagnostics.lights[0];
            let mode = light_mode_word(live.mode);
            self.draw_text(b"MOD", block_x(Block::Second), overlay_row_top(2));
            self.draw_text(mode, block_value_x(Block::Second), overlay_row_top(2));

            if let Some(fingerprint) = fingerprint {
                let sampled = &mut [0u8; 12];
                let sampled = format_u32(fingerprint, sampled);
                self.draw_text(b"FP", block_x(Block::Third), overlay_row_top(0));
                self.draw_text(sampled, block_value_x(Block::Third), overlay_row_top(0));
            }
        } else if let Some(reason) = fault {
            self.draw_text(reason.as_bytes(), block_x(Block::First), overlay_row_top(5));
            self.draw_text(b"RETRYING", block_x(Block::First), overlay_row_top(6));
        }
        if self.debug {
            self.draw_fps();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::diagnostics::Diagnostics;
    use crate::state::PageId;
    use alloc::vec;
    use iot_core::drivers::camera::CameraCounters;

    use iot_core::ui::Canvas;

    const WIDTH: usize = iot_core::overlay::PANEL_COLUMNS;
    const HEIGHT: usize = iot_core::overlay::PANEL_ROWS;

    #[test]
    fn the_camera_readout_reports_a_fingerprint_only_for_a_real_frame() {
        let mut frame = vec![0u8; WIDTH * HEIGHT * 2];
        for (index, byte) in frame.iter_mut().enumerate() {
            *byte = (index % 251) as u8;
        }
        let diagnostics = Diagnostics {
            page: PageId::Camera,
            ..Diagnostics::default()
        };
        let before = frame.clone();
        let canvas = Canvas::new(&mut frame, WIDTH, HEIGHT, None);
        let mut surface = Surface::new(canvas, &diagnostics, 0, 60, false, 100);
        surface.paint_camera(CameraSnapshot::STARTING);
        let starting = surface.frame().to_vec();
        assert!(
            starting
                .iter()
                .zip(&before)
                .any(|(after, before)| after != before),
            "the waiting readout is drawn over the frame"
        );
        surface.paint_camera(CameraSnapshot {
            counters: CameraCounters::ZERO,
            geometry: None,
            starting: false,
            fault: None,
        });
        assert_ne!(
            starting,
            surface.frame(),
            "a real frame adds its fingerprint"
        );
    }

    #[test]
    fn a_camera_that_cannot_start_says_so_and_offers_the_retry() {
        let mut frame = vec![0u8; WIDTH * HEIGHT * 2];
        let diagnostics = Diagnostics {
            page: PageId::Camera,
            ..Diagnostics::default()
        };
        let canvas = Canvas::new(&mut frame, WIDTH, HEIGHT, None);
        let mut surface = Surface::new(canvas, &diagnostics, 0, 60, false, 100);
        surface.paint_camera(CameraSnapshot {
            counters: CameraCounters::ZERO,
            geometry: None,
            starting: false,
            fault: Some("the sensor did not answer"),
        });
        let failed = surface.frame().to_vec();

        let mut healthy = vec![0u8; WIDTH * HEIGHT * 2];
        let canvas = Canvas::new(&mut healthy, WIDTH, HEIGHT, None);
        let mut surface = Surface::new(canvas, &diagnostics, 0, 60, false, 100);
        surface.paint_camera(CameraSnapshot {
            counters: CameraCounters::ZERO,
            geometry: None,
            starting: false,
            fault: None,
        });
        assert_ne!(
            failed,
            surface.frame(),
            "the fault readout is not the no-sensor readout"
        );
    }
}
