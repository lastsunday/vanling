//! The portable page surface: a [`Canvas`] plus the snapshot the current page draws from.

use core::ops::{Deref, DerefMut};

use crate::display::diagnostics::Diagnostics;
use crate::model::state::DisplayPage;
use iot_core::drivers::camera::CameraSnapshot;
use iot_core::overlay::{GLYPH_PITCH, OVERLAY_GAP};
use iot_core::paint::MODE_BREATH;
use iot_core::paint::{Rgb, brightness_of, rgb_hue};

use crate::display::ui::placement::{FPS_Y, PANEL_RIGHT};
use crate::display::ui::text::format_u16;
use iot_ui::Canvas;

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

    pub fn render(&mut self, page: DisplayPage) {
        match page {
            DisplayPage::Splash => self.stamp_splash(),
            DisplayPage::Attitude => self.stamp_attitude(),
            DisplayPage::Audio => self.stamp_audio(),
            DisplayPage::Speaker => self.stamp_speaker(),
            DisplayPage::Camera => self.stamp_camera(CameraSnapshot::STARTING),
            DisplayPage::Network => self.stamp_network(),
            DisplayPage::Ambient if self.debug => self.stamp_diagnostics(self.live()),
            DisplayPage::Ambient => {}
        }
        if self.debug && page != DisplayPage::Camera {
            self.stamp_fps();
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
    use alloc::vec::Vec;
    use core::sync::atomic::{AtomicUsize, Ordering};
    use iot_core::drivers::audio::ENVELOPE_COLUMNS;

    const WIDTH: usize = iot_core::overlay::PANEL_COLUMNS;
    const HEIGHT: usize = iot_core::overlay::PANEL_ROWS;

    fn render(page: DisplayPage, debug: bool, ground: Option<Rgb>) -> Vec<u8> {
        let mut frame = vec![0u8; WIDTH * HEIGHT * 2];
        let diagnostics = Diagnostics {
            page,
            ..Diagnostics::default()
        };
        {
            let canvas = Canvas::new(&mut frame, WIDTH, HEIGHT, ground);
            let mut surface = Surface::new(canvas, &diagnostics, 0, 60, debug, 100);
            surface.render(page);
        }
        frame
    }

    /// A full-frame FNV-1a, used only by the golden table: the sampling fingerprint the camera
    /// page compares frames with is deliberately sparse and would miss small ink changes.
    fn fingerprint(frame: &[u8]) -> u32 {
        let mut hash = 0x811c_9dc5u32;
        for &byte in frame {
            hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
        }
        hash
    }

    /// One line per page and debug state. Regenerate with `PAGE_GOLDEN_UPDATE=1`.
    const GOLDEN: [(&str, bool, u32); 14] = [
        ("ambient", false, 0xc18e_7dc5),
        ("ambient", true, 0xc104_7691),
        ("attitude", false, 0x3d8d_fc2f),
        ("attitude", true, 0xc9e3_2c21),
        ("audio", false, 0xe52b_405d),
        ("audio", true, 0x5dca_544f),
        ("speaker", false, 0x9370_7f03),
        ("speaker", true, 0x0302_faf5),
        ("camera", false, 0x311b_aeb1),
        ("camera", true, 0x9903_7aa3),
        ("network", false, 0x53df_7d41),
        ("network", true, 0x5c55_fd33),
        ("splash", false, 0x328a_4c63),
        ("splash", true, 0xe51b_5e55),
    ];

    #[test]
    fn every_page_draws_the_picture_the_golden_table_expects() {
        let update = std::env::var_os("PAGE_GOLDEN_UPDATE").is_some();
        for page in DisplayPage::ALL {
            let ground = (page != DisplayPage::Camera).then_some(Rgb(0, 0, 0));
            for debug in [false, true] {
                let actual = fingerprint(&render(page, debug, ground));
                if update {
                    std::println!("    (\"{}\", {debug}, 0x{actual:08x}),", page.path());
                    continue;
                }
                let expected = GOLDEN
                    .iter()
                    .find(|(path, state, _)| *path == page.path() && *state == debug)
                    .map(|(_, _, hash)| *hash)
                    .expect("every page has a golden row; run with PAGE_GOLDEN_UPDATE=1");
                assert_eq!(
                    actual,
                    expected,
                    "{} (debug={debug}) changed; regenerate only for an intended pixel change",
                    page.path()
                );
            }
        }
    }

    #[test]
    fn every_page_stamps_something_and_does_it_the_same_way_twice() {
        for page in DisplayPage::ALL {
            let ground = (page != DisplayPage::Camera).then_some(Rgb(0, 0, 0));
            let first = render(page, true, ground);
            let second = render(page, true, ground);
            assert!(
                first.iter().any(|byte| *byte != 0),
                "{page:?} stamped no ink"
            );
            assert_eq!(first, second, "{page:?} is not deterministic");
        }
    }

    #[test]
    fn the_camera_page_puts_the_fps_badge_where_every_page_does() {
        let mut expected = render(DisplayPage::Camera, false, None);
        let diagnostics = Diagnostics {
            page: DisplayPage::Camera,
            ..Diagnostics::default()
        };
        {
            let canvas = Canvas::new(&mut expected, WIDTH, HEIGHT, None);
            let mut surface = Surface::new(canvas, &diagnostics, 0, 60, false, 100);
            surface.stamp_fps();
        }
        assert_eq!(render(DisplayPage::Camera, true, None), expected);
    }

    #[test]
    fn the_sweep_runs_the_platform_stack_probe() {
        static PROBES: AtomicUsize = AtomicUsize::new(0);
        fn probe() {
            PROBES.fetch_add(1, Ordering::Relaxed);
        }

        let mut frame = vec![0u8; WIDTH * HEIGHT * 2];
        let diagnostics = Diagnostics::default();
        let canvas = Canvas::new(&mut frame, WIDTH, HEIGHT, None);
        let mut surface =
            Surface::new(canvas, &diagnostics, 0, 60, false, 100).with_stack_probe(probe);
        surface.stamp_sweep(&[0; ENVELOPE_COLUMNS], &[0; ENVELOPE_COLUMNS], None);
        assert_eq!(
            PROBES.load(Ordering::Relaxed),
            1,
            "the stack reading must be taken while the sweep's arrays are live"
        );
    }
}
