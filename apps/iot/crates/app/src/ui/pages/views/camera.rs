//! The Camera page: the sensor's frame, and the readout stamped over it.

use iot_core::drivers::camera::CameraSnapshot;
use iot_core::overlay::{Block, block_value_x, block_x, overlay_row_top};

use crate::ui::pages::surface::Surface;
use crate::ui::pages::text::{
    format_pair, format_u32, frame_fingerprint, light_mode_word, write_ratio,
};

impl Surface<'_> {
    /// The camera's own frame, after the sensor has filled it.
    pub(crate) fn stamp_camera(&mut self, readout: CameraSnapshot) {
        let fault = readout.fault;
        let fingerprint =
            (!readout.starting && fault.is_none()).then(|| frame_fingerprint(self.frame()));

        self.stamp_text(b"CAMERA", block_x(Block::First), overlay_row_top(0));
        match (fault, readout.starting) {
            (Some(_), _) => {
                self.stamp_text(b"NO CAM", block_value_x(Block::First), overlay_row_top(0))
            }
            (None, true) => {
                self.stamp_text(b"WAIT", block_value_x(Block::First), overlay_row_top(0))
            }
            (None, false) => {}
        }
        if fault.is_none() {
            if let Some(geometry) = readout.geometry {
                let ratio = &mut [0u8; 6];
                let ratio = write_ratio(ratio, geometry.subsample);
                self.stamp_text(b"WIN", block_x(Block::First), overlay_row_top(1));
                self.stamp_text(ratio, block_value_x(Block::First), overlay_row_top(1));
                let out = &mut [0u8; 12];
                let out = format_pair(Some((geometry.out_width, geometry.out_height)), out);
                self.stamp_text(b"OUT", block_x(Block::First), overlay_row_top(2));
                self.stamp_text(out, block_value_x(Block::First), overlay_row_top(2));
                let read = &mut [0u8; 12];
                let read = format_pair(Some((geometry.win_width, geometry.win_height)), read);
                self.stamp_text(b"RD", block_x(Block::First), overlay_row_top(3));
                self.stamp_text(read, block_value_x(Block::First), overlay_row_top(3));
            } else {
                self.stamp_text(
                    if readout.starting { b"----" } else { b"NOBUS" },
                    block_value_x(Block::First),
                    overlay_row_top(1),
                );
            }

            let frames = &mut [0u8; 12];
            let frames = format_u32(readout.counters.frames, frames);
            self.stamp_text(b"FRM", block_x(Block::Second), overlay_row_top(0));
            self.stamp_text(frames, block_value_x(Block::Second), overlay_row_top(0));
            let restarts = &mut [0u8; 12];
            let restarts = format_u32(readout.counters.restarts, restarts);
            self.stamp_text(b"RM", block_x(Block::Second), overlay_row_top(1));
            self.stamp_text(restarts, block_value_x(Block::Second), overlay_row_top(1));
            let live = self.diagnostics.lights[0];
            let mode = light_mode_word(live.mode);
            self.stamp_text(b"MOD", block_x(Block::Second), overlay_row_top(2));
            self.stamp_text(mode, block_value_x(Block::Second), overlay_row_top(2));

            if let Some(fingerprint) = fingerprint {
                let sampled = &mut [0u8; 12];
                let sampled = format_u32(fingerprint, sampled);
                self.stamp_text(b"FP", block_x(Block::Third), overlay_row_top(0));
                self.stamp_text(sampled, block_value_x(Block::Third), overlay_row_top(0));
            }
        } else if let Some(reason) = fault {
            self.stamp_text(reason.as_bytes(), block_x(Block::First), overlay_row_top(5));
            self.stamp_text(b"RETRYING", block_x(Block::First), overlay_row_top(6));
        }
        if self.debug {
            self.stamp_fps();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::state::DisplayPage;
    use crate::ui::diagnostics::Diagnostics;
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
            page: DisplayPage::Camera,
            ..Diagnostics::default()
        };
        let before = frame.clone();
        let canvas = Canvas::new(&mut frame, WIDTH, HEIGHT, None);
        let mut surface = Surface::new(canvas, &diagnostics, 0, 60, false, 100);
        surface.stamp_camera(CameraSnapshot::STARTING);
        let starting = surface.frame().to_vec();
        assert!(
            starting
                .iter()
                .zip(&before)
                .any(|(after, before)| after != before),
            "the waiting readout is drawn over the frame"
        );
        surface.stamp_camera(CameraSnapshot {
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
            page: DisplayPage::Camera,
            ..Diagnostics::default()
        };
        let canvas = Canvas::new(&mut frame, WIDTH, HEIGHT, None);
        let mut surface = Surface::new(canvas, &diagnostics, 0, 60, false, 100);
        surface.stamp_camera(CameraSnapshot {
            counters: CameraCounters::ZERO,
            geometry: None,
            starting: false,
            fault: Some("the sensor did not answer"),
        });
        let failed = surface.frame().to_vec();

        let mut healthy = vec![0u8; WIDTH * HEIGHT * 2];
        let canvas = Canvas::new(&mut healthy, WIDTH, HEIGHT, None);
        let mut surface = Surface::new(canvas, &diagnostics, 0, 60, false, 100);
        surface.stamp_camera(CameraSnapshot {
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
