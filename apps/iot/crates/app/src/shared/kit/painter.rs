//! The portable page painter: a [`Canvas`] plus the surface's own draw state.
//!
//! The painter owns the marks, not the frame data: [`paint`](crate::pages::paint) hands it the
//! snapshot and decides which page draws what.

use core::ops::{Deref, DerefMut};

use crate::shared::kit::placement::{FPS_Y, PANEL_RIGHT};
use crate::shared::kit::text::format_u16;
use crate::shared::snapshot::FrameSnapshot;
use iot_core::light::MODE_BREATH;
use iot_core::light::{Rgb, brightness_of, rgb_hue};
use iot_core::overlay::{GLYPH_PITCH, OVERLAY_GAP};
use iot_core::ui::Canvas;

/// One surface's draw target: the frame buffer and the settings this panel draws with.
pub struct Painter<'a> {
    canvas: Canvas<'a>,
    pub(crate) instance: u8,
    fps: u8,
    pub(crate) debug: bool,
    pub(crate) spl_offset_db: i16,
    stack_probe: Option<fn()>,
}

impl<'a> Painter<'a> {
    pub fn new(canvas: Canvas<'a>, instance: u8, fps: u8, debug: bool, spl_offset_db: i16) -> Self {
        Self {
            canvas,
            instance,
            fps,
            debug,
            spl_offset_db,
            stack_probe: None,
        }
    }

    /// Hands the painter a platform-specific probe for the stack reporting. It is called at the
    /// start of the sweep, where the Audio page's two 400 B arrays and the page's own frames are
    /// all live — the deepest the paint path reaches.
    pub fn with_stack_probe(mut self, probe: fn()) -> Self {
        self.stack_probe = Some(probe);
        self
    }

    pub(crate) fn probe_stack(&self) {
        if let Some(probe) = self.stack_probe {
            probe();
        }
    }

    pub(crate) fn live(&self, snapshot: &FrameSnapshot) -> (u8, u8) {
        let mine = snapshot.lights[usize::from(self.instance)];
        if mine.mode == MODE_BREATH {
            let color = self.canvas.ground().unwrap_or(Rgb(0, 0, 0));
            (brightness_of(color), rgb_hue(color))
        } else {
            (mine.brightness, mine.hue)
        }
    }

    pub(crate) fn draw_fps(&mut self) {
        self.draw_fps_at(FPS_Y);
    }

    fn draw_fps_at(&mut self, top: usize) {
        let fps = self.fps;
        let buf = &mut [0u8; 6];
        let digits = format_u16(u16::from(fps), buf);
        let value_left = (PANEL_RIGHT + OVERLAY_GAP as isize
            - digits.len() as isize * GLYPH_PITCH as isize)
            .max(0) as usize;
        let label_left = (value_left as isize - (3 * GLYPH_PITCH as isize + OVERLAY_GAP as isize))
            .max(0) as usize;
        self.draw_text(b"FPS", label_left, top);
        self.draw_text(digits, value_left, top);
    }
}

impl<'a> Deref for Painter<'a> {
    type Target = Canvas<'a>;

    fn deref(&self) -> &Self::Target {
        &self.canvas
    }
}

impl<'a> DerefMut for Painter<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.canvas
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use core::sync::atomic::{AtomicUsize, Ordering};
    use iot_core::drivers::audio::ENVELOPE_COLUMNS;

    const WIDTH: usize = iot_core::overlay::PANEL_COLUMNS;
    const HEIGHT: usize = iot_core::overlay::PANEL_ROWS;

    #[test]
    fn the_sweep_runs_the_platform_stack_probe() {
        static PROBES: AtomicUsize = AtomicUsize::new(0);
        fn probe() {
            PROBES.fetch_add(1, Ordering::Relaxed);
        }

        let mut frame = vec![0u8; WIDTH * HEIGHT * 2];
        {
            let canvas = Canvas::new(&mut frame, WIDTH, HEIGHT, None);
            let mut painter = Painter::new(canvas, 0, 60, false, 100).with_stack_probe(probe);
            painter.draw_sweep(&[0; ENVELOPE_COLUMNS], &[0; ENVELOPE_COLUMNS], None);
        }
        assert_eq!(
            PROBES.load(Ordering::Relaxed),
            1,
            "the stack reading must be taken while the sweep's arrays are live"
        );
    }
}
