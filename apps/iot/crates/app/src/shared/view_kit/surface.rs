//! The portable page surface: a [`Canvas`] plus the snapshot the current page draws from.
//!
//! The surface owns the primitives, not the page match: [`paint`](crate::features::paint)
//! decides which page stamps what.

use core::ops::{Deref, DerefMut};

use crate::shared::diagnostics::Diagnostics;
use crate::shared::view_kit::placement::{FPS_Y, PANEL_RIGHT};
use crate::shared::view_kit::text::format_u16;
use iot_core::overlay::{GLYPH_PITCH, OVERLAY_GAP};
use iot_core::paint::MODE_BREATH;
use iot_core::paint::{Rgb, brightness_of, rgb_hue};
use iot_core::ui::Canvas;

/// One page's drawing: the frame, the snapshot it reads, and the surface's own identity.
pub struct Surface<'a> {
    canvas: Canvas<'a>,
    pub(crate) diagnostics: &'a Diagnostics,
    pub(crate) instance: u8,
    fps: u8,
    pub(crate) debug: bool,
    pub(crate) spl_offset_db: i16,
    stack_probe: Option<fn()>,
}

impl<'a> Surface<'a> {
    pub fn new(
        canvas: Canvas<'a>,
        diagnostics: &'a Diagnostics,
        instance: u8,
        fps: u8,
        debug: bool,
        spl_offset_db: i16,
    ) -> Self {
        Self {
            canvas,
            diagnostics,
            instance,
            fps,
            debug,
            spl_offset_db,
            stack_probe: None,
        }
    }

    /// Hands the surface a platform-specific probe for the stack reporting. It is called at the
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

    pub(crate) fn live(&self) -> (u8, u8) {
        let mine = self.diagnostics.lights[usize::from(self.instance)];
        if mine.mode == MODE_BREATH {
            let color = self.canvas.ground().unwrap_or(Rgb(0, 0, 0));
            (brightness_of(color), rgb_hue(color))
        } else {
            (mine.brightness, mine.hue)
        }
    }

    pub(crate) fn stamp_fps(&mut self) {
        self.stamp_fps_at(FPS_Y);
    }

    fn stamp_fps_at(&mut self, top: usize) {
        let fps = self.fps;
        let buf = &mut [0u8; 6];
        let digits = format_u16(u16::from(fps), buf);
        let value_left = (PANEL_RIGHT + OVERLAY_GAP as isize
            - digits.len() as isize * GLYPH_PITCH as isize)
            .max(0) as usize;
        let label_left = (value_left as isize - (3 * GLYPH_PITCH as isize + OVERLAY_GAP as isize))
            .max(0) as usize;
        self.stamp_text(b"FPS", label_left, top);
        self.stamp_text(digits, value_left, top);
    }
}

impl<'a> Deref for Surface<'a> {
    type Target = Canvas<'a>;

    fn deref(&self) -> &Self::Target {
        &self.canvas
    }
}

impl<'a> DerefMut for Surface<'a> {
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
        let diagnostics = Diagnostics::default();
        {
            let canvas = Canvas::new(&mut frame, WIDTH, HEIGHT, None);
            let mut surface =
                Surface::new(canvas, &diagnostics, 0, 60, false, 100).with_stack_probe(probe);
            surface.stamp_sweep(&[0; ENVELOPE_COLUMNS], &[0; ENVELOPE_COLUMNS], None);
        }
        assert_eq!(
            PROBES.load(Ordering::Relaxed),
            1,
            "the stack reading must be taken while the sweep's arrays are live"
        );
    }
}
