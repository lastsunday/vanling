//! The portable page surface: a [`Canvas`] plus the snapshot the current page draws from.
//!
//! The snapshot is borrowed, never copied: `Diagnostics` embeds the audio envelope, and a
//! by-value copy is a kilobyte and a half of a main stack this path already runs close to. The
//! ground is the fill the board last painted the panel with, which is what a page's own paper is
//! made from.
//!
//! Every page here is immediate-mode ink: `render` clears nothing and keeps no widget tree, so
//! what is on the panel is exactly what the last call drew. The board owns the frame buffer,
//! the DMA chain and the page choice; it hands those in and ships the bytes back out.

use core::ops::{Deref, DerefMut};

use iot_core::diagnostics::Diagnostics;
use iot_core::drivers::camera::{CameraCounters, WindowGeometry};
use iot_core::drivers::light::{Rgb, rgb_hue};
use iot_core::overlay::{Block, GLYPH_PITCH, OVERLAY_GAP, block_value_x, block_x, overlay_row_top};
use iot_core::render::MODE_BREATH;
use iot_core::state::DisplayPage;

use crate::canvas::Canvas;
use crate::layout::{FPS_Y, PANEL_RIGHT};
use crate::pages::draw::{brightness_of, format_u16, mode_word};
use crate::text::{format_pair, format_u32, frame_fingerprint, write_ratio};

/// The camera's readings at the instant its page is stamped. The frame itself is the canvas;
/// these are the facts about it that are not pixels.
#[derive(Clone, Copy, Debug, Default)]
pub struct CameraReadout {
    pub counters: CameraCounters,
    pub geometry: Option<WindowGeometry>,
    /// The page has just been entered and no frame has arrived yet, so the numbers would all read
    /// zero and `NOBUS` would claim there is no sensor. Says it is starting instead.
    pub starting: bool,
}

impl CameraReadout {
    /// The tick where the page has just been entered and the sensor has not delivered yet.
    pub const STARTING: Self = Self {
        counters: CameraCounters::ZERO,
        geometry: None,
        starting: true,
    };
}

/// One page's drawing: the frame, the snapshot it reads, and the surface's own identity.
pub struct Surface<'a> {
    canvas: Canvas<'a>,
    pub(crate) diagnostics: &'a Diagnostics,
    pub(crate) instance: u8,
    fps: u8,
    debug: bool,
    pub(crate) spl_offset_db: i16,
    /// The platform's own stack probe, run where a page's working set is deepest. Host tests
    /// have none; the board hands in the one that reads its linker symbols.
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
            DisplayPage::Camera => self.stamp_camera(CameraReadout::STARTING),
            DisplayPage::Network => self.stamp_network(),
            DisplayPage::Ambient if self.debug => self.stamp_diagnostics(self.live()),
            DisplayPage::Ambient => {}
        }
        if self.debug && page != DisplayPage::Camera {
            self.stamp_fps();
        }
    }

    /// The camera's own frame, after the sensor has filled it. Kept separate from [`Self::render`]
    /// because the frame arrives on the camera's clock rather than the page's.
    pub fn stamp_camera(&mut self, readout: CameraReadout) {
        let fingerprint = (!readout.starting).then(|| frame_fingerprint(self.canvas.frame()));

        self.stamp_text(b"CAMERA", block_x(Block::First), overlay_row_top(0));
        if readout.starting {
            self.stamp_text(b"WAIT", block_value_x(Block::First), overlay_row_top(0));
        }
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

        // Frames, and the re-arms that cost: on a single-buffer ring every whole frame costs one
        // rebuild, so the two counters move together and a gap between them is what a stall looks
        // like.
        let frames = &mut [0u8; 12];
        let frames = format_u32(readout.counters.frames, frames);
        self.stamp_text(b"FRM", block_x(Block::Second), overlay_row_top(0));
        self.stamp_text(frames, block_value_x(Block::Second), overlay_row_top(0));
        let restarts = &mut [0u8; 12];
        let restarts = format_u32(readout.counters.restarts, restarts);
        self.stamp_text(b"RM", block_x(Block::Second), overlay_row_top(1));
        self.stamp_text(restarts, block_value_x(Block::Second), overlay_row_top(1));
        // The light's own row, because the Camera page does not draw the fill that row would
        // otherwise sit on — the state still moved, so the readout should still say where it is.
        // Three glyphs, like every other label: the reading column sits a block label slot from
        // the block's left edge, and a four-glyph label fills that slot exactly, leaving the value
        // butted against it. The value is the mode word either way, so the label says as much as
        // it needs to and no more.
        let live = self.diagnostics.lights[0];
        let mode = mode_word(live.mode);
        self.stamp_text(b"MOD", block_x(Block::Second), overlay_row_top(2));
        self.stamp_text(mode, block_value_x(Block::Second), overlay_row_top(2));

        // A fold over the frame's own bytes, over a prime stride so the walk lands on a spread of
        // rows rather than tracking one. Sparse on purpose: this runs on every painted frame.
        // Folded before any glyph goes down, so the number describes the sensor's picture and not
        // the readout sitting on it — otherwise the readout itself moves it every repaint.
        if let Some(fingerprint) = fingerprint {
            let sampled = &mut [0u8; 12];
            let sampled = format_u32(fingerprint, sampled);
            self.stamp_text(b"FP", block_x(Block::Third), overlay_row_top(0));
            self.stamp_text(sampled, block_value_x(Block::Third), overlay_row_top(0));
        }
        if self.debug {
            self.stamp_fps();
        }
    }

    fn live(&self) -> (u8, u8) {
        let mine = self.diagnostics.lights[usize::from(self.instance)];
        if mine.mode == MODE_BREATH {
            let color = self
                .canvas
                .ground()
                .map_or(Rgb(0, 0, 0), |(_, color)| color);
            (brightness_of(color), rgb_hue(color))
        } else {
            (mine.brightness, mine.hue)
        }
    }

    fn stamp_fps(&mut self) {
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
    use iot_core::drivers::light::Fill;

    const WIDTH: usize = iot_core::overlay::PANEL_COLUMNS;
    const HEIGHT: usize = iot_core::overlay::PANEL_ROWS;

    fn render(page: DisplayPage, debug: bool, ground: Option<(Fill, Rgb)>) -> Vec<u8> {
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

    #[test]
    fn every_page_stamps_something_and_does_it_the_same_way_twice() {
        let pages = [
            DisplayPage::Ambient,
            DisplayPage::Attitude,
            DisplayPage::Audio,
            DisplayPage::Speaker,
            DisplayPage::Camera,
            DisplayPage::Network,
            DisplayPage::Splash,
        ];
        for page in pages {
            let ground = (page != DisplayPage::Camera).then_some((Fill::Uniform, Rgb(0, 0, 0)));
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
        surface.stamp_camera(CameraReadout::STARTING);
        let starting = surface.frame().to_vec();
        assert!(
            starting
                .iter()
                .zip(&before)
                .any(|(after, before)| after != before),
            "the waiting readout is drawn over the frame"
        );
        surface.stamp_camera(CameraReadout {
            counters: CameraCounters::ZERO,
            geometry: None,
            starting: false,
        });
        assert_ne!(
            starting,
            surface.frame(),
            "a real frame adds its fingerprint"
        );
    }
}
