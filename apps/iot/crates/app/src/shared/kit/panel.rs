//! The portable page surface: a [`Canvas`], the screen it belongs to, and the dispatch that
//! turns a page's declaration into ink.
//!
//! The panel owns the marks, not the frame data: [`paint`](crate::services::paint) hands it the
//! snapshot and decides which page declares what.

use crate::shared::snapshot::FrameSnapshot;
use iot_core::light::{MODE_BREATH, Rgb, brightness_of, rgb_hue};
use iot_core::ui::panel::{CONTENT_BOTTOM, PANEL_COLUMNS, RIGHT};
use iot_core::ui::{BoxConstraints, Canvas, Rect, Screen, UNIT, Ui};

/// What a page declares: its picture, as the boxes and runs it wants at the screen's size.
pub type View = for<'a, 's> fn(&FrameSnapshot, &mut Ui<'a, 's>);

/// A page's own ink for the custom slots its declaration leaves: the screen it is drawn for,
/// the slot, and the rectangle layout gave it.
pub type Custom = fn(&FrameSnapshot, Screen, u8, &mut Canvas<'_>, Rect);

/// A page with nothing to paint itself.
pub const fn no_custom(_: &FrameSnapshot, _: Screen, _: u8, _: &mut Canvas<'_>, _: Rect) {}

/// One surface's draw target: the frame buffer and the settings this panel draws with.
pub struct Panel<'a> {
    canvas: Canvas<'a>,
    screen: Screen,
}

impl<'a> Panel<'a> {
    pub fn new(canvas: Canvas<'a>, screen: Screen) -> Self {
        Self { canvas, screen }
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn canvas(&self) -> &Canvas<'a> {
        &self.canvas
    }

    pub fn into_canvas(self) -> Canvas<'a> {
        self.canvas
    }

    /// The brightness and hue the surface is showing now, which is the phase's own while the
    /// lamp breathes and the declared reading otherwise.
    pub fn live(&self, snapshot: &FrameSnapshot) -> (u8, u8) {
        let mine = snapshot.lights[usize::from(self.screen.instance)];
        if mine.mode == MODE_BREATH {
            let color = self.canvas.ground().unwrap_or(Rgb(0, 0, 0));
            (brightness_of(color), rgb_hue(color))
        } else {
            (mine.brightness, mine.hue)
        }
    }

    /// Lays a page's declaration out in the content rows and draws it, then the footer.
    pub fn view(&mut self, snapshot: &FrameSnapshot, view: View, custom: Custom) {
        self.declare(snapshot, custom, view);
    }

    /// The same, for a page whose declaration needs more than the snapshot: a camera frame
    /// arrives with its own readout in hand.
    pub fn declare(
        &mut self,
        snapshot: &FrameSnapshot,
        custom: Custom,
        body: impl FnOnce(&FrameSnapshot, &mut Ui<'_, '_>),
    ) {
        let screen = self.screen;
        {
            let mut ui = Ui::new_with_height(&mut self.canvas, screen, CONTENT_BOTTOM);
            body(snapshot, &mut ui);
            let mut paint = |slot: u8, canvas: &mut Canvas<'_>, rect: Rect| {
                custom(snapshot, screen, slot, canvas, rect)
            };
            ui.finish(
                BoxConstraints::tight(PANEL_COLUMNS as i32 * UNIT, CONTENT_BOTTOM as i32 * UNIT),
                &mut paint,
            );
        }
        if screen.debug {
            self.draw_fps();
        }
    }

    fn draw_fps(&mut self) {
        let mut digits_buf = [0u8; 6];
        let digits = iot_core::ui::text::format_u16(u16::from(self.screen.fps), &mut digits_buf);
        let mut run = [0u8; 10];
        run[..3].copy_from_slice(b"FPS");
        run[3] = b' ';
        let len = digits.len().min(run.len() - 4);
        run[4..4 + len].copy_from_slice(&digits[..len]);
        self.canvas
            .draw_text_right(&run[..4 + len], RIGHT as isize, CONTENT_BOTTOM);
    }
}
