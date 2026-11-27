//! The portable page surface: a `Canvas`, its screen, and the dispatch from a page's
//! declaration to painting. Frame data arrives from `services::paint`; the surface owns no page.

use crate::shared::snapshot::FrameSnapshot;
use iot_core::drivers::surface::{DamageRect, FrameDamage};
use iot_core::ui::panel::{CONTENT_BOTTOM, FOOTER_ROWS, PANEL_COLUMNS, RIGHT};
use iot_core::ui::{BoxConstraints, Canvas, FramePrint, Layout, PX, Rect, Screen, Source, Ui};

/// What a page declares: its picture, as the boxes and runs it wants at the screen's size.
pub type View = for<'a, 's> fn(&FrameSnapshot, &mut Ui<'a, 's>);

/// A page's own drawing for the custom slots its declaration leaves: the screen it is drawn for,
/// the slot, and the rectangle layout gave it.
pub type Custom = fn(&FrameSnapshot, Screen, u8, &mut Canvas<'_>, Rect);

/// A page with nothing to paint itself.
pub const fn no_custom(_: &FrameSnapshot, _: Screen, _: u8, _: &mut Canvas<'_>, _: Rect) {}

/// A page's declaration with no canvas, for the one measurement a page does at construction.
pub fn headless() -> Ui<'static, 'static> {
    Ui::headless(PANEL_COLUMNS, CONTENT_BOTTOM, Screen::default())
}

/// The footer band the debug badge draws in. It sits below every page, so it is damage in its
/// own right and never part of a page's tree.
pub fn footer_damage() -> DamageRect {
    DamageRect::new(
        0,
        CONTENT_BOTTOM as u16,
        PANEL_COLUMNS as u16,
        FOOTER_ROWS as u16,
    )
}

/// Lays a declaration out at the content size [`Panel::declare`] paints with.
pub fn finish(ui: &mut Ui<'_, '_>) {
    ui.layout(BoxConstraints::tight(
        PANEL_COLUMNS as i32 * PX,
        CONTENT_BOTTOM as i32 * PX,
    ));
}

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
            let mut ui = Ui::boxed_with_height(&mut self.canvas, screen, CONTENT_BOTTOM);
            body(snapshot, &mut ui);
            let mut paint = |slot: u8, canvas: &mut Canvas<'_>, rect: Rect| {
                custom(snapshot, screen, slot, canvas, rect)
            };
            ui.paint(
                BoxConstraints::tight(PANEL_COLUMNS as i32 * PX, CONTENT_BOTTOM as i32 * PX),
                &mut paint,
            );
        }
        if screen.debug {
            self.draw_fps();
        }
    }

    /// Lays the page out, records its fingerprints in `next`, and draws the pixels `source`
    /// names. Answers the damage the panel has to receive, the debug badge included.
    pub fn view_measured(
        &mut self,
        snapshot: &FrameSnapshot,
        view: View,
        custom: Custom,
        next: &mut FramePrint,
        source: Source<'_>,
    ) -> (FrameDamage, Layout) {
        let screen = self.screen;
        let mut extra = FrameDamage::NONE;
        if screen.debug {
            extra.add(footer_damage());
        }
        let (damage, layout) = {
            let mut ui = Ui::boxed_with_height(&mut self.canvas, screen, CONTENT_BOTTOM);
            view(snapshot, &mut ui);
            let mut paint = |slot: u8, canvas: &mut Canvas<'_>, rect: Rect| {
                custom(snapshot, screen, slot, canvas, rect)
            };
            let damage = ui.paint_measured(
                BoxConstraints::tight(PANEL_COLUMNS as i32 * PX, CONTENT_BOTTOM as i32 * PX),
                next,
                source,
                extra,
                &mut paint,
            );
            let mut layout = ui.geometry();
            layout.set_page(snapshot.page.code());
            (damage, layout)
        };
        if screen.debug {
            self.draw_fps();
        }
        (damage, layout)
    }

    fn draw_fps(&mut self) {
        let mut digits_buf = [0u8; 6];
        let digits = iot_core::ui::text::format_u16(u16::from(self.screen.fps), &mut digits_buf);
        let mut run = [0u8; 10];
        run[..3].copy_from_slice(b"FPS");
        run[3] = b' ';
        let len = digits.len().min(run.len() - 4);
        run[4..4 + len].copy_from_slice(&digits[..len]);
        // The page's last node may have left a colour on the canvas; the footer is the panel's
        // own and always draws in the default.
        self.canvas.set_color(None);
        self.canvas
            .draw_text_right(&run[..4 + len], RIGHT as isize, CONTENT_BOTTOM);
    }
}
