//! Paints the product's pages into a board-owned frame. Stateless: the snapshot arrives
//! with each call, so the screen never holds a product type.

use core::any::Any;

use crate::services::camera::CameraPage;
use crate::services::paint;
use crate::shared::kit::painter::Painter;
use crate::shared::snapshot::FrameSnapshot;
use iot_core::drivers::camera::CameraSnapshot;
use iot_core::drivers::surface::{ScreenContext, ScreenPainter, ScreenTarget};
use iot_core::ui::Canvas;

pub struct AppPainter {
    overlay: bool,
}

impl AppPainter {
    pub const fn new(overlay: bool) -> Self {
        Self { overlay }
    }

    fn painter<'a>(&self, target: ScreenTarget<'a>, context: ScreenContext) -> Painter<'a> {
        let canvas = Canvas::new(target.frame, target.width, target.height, target.ground);
        let painter = Painter::new(
            canvas,
            context.instance,
            context.fps,
            self.overlay,
            context.spl_offset_db,
        );
        match context.stack_probe {
            Some(probe) => painter.with_stack_probe(probe),
            None => painter,
        }
    }
}

impl ScreenPainter for AppPainter {
    fn paint(&self, target: ScreenTarget<'_>, context: ScreenContext, snapshot: &dyn Any) {
        let Some(frame) = snapshot.downcast_ref::<FrameSnapshot>() else {
            debug_assert!(false, "the screen's snapshot is the frame it was handed");
            return;
        };
        paint(frame.page, frame, &mut self.painter(target, context));
    }

    fn paint_camera(
        &self,
        target: ScreenTarget<'_>,
        readout: CameraSnapshot,
        context: ScreenContext,
        snapshot: &dyn Any,
    ) {
        let Some(frame) = snapshot.downcast_ref::<FrameSnapshot>() else {
            debug_assert!(false, "the screen's snapshot is the frame it was handed");
            return;
        };
        CameraPage::paint_frame(frame, &mut self.painter(target, context), readout);
    }
}
