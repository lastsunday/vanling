//! Paints the product's pages into a board-owned frame. Stateless: the snapshot arrives
//! with each call, so the screen never holds a product type.

use core::any::Any;

use crate::services::camera::CameraPage;
use crate::services::paint;
use crate::shared::kit::panel::{Panel, no_custom};
use crate::shared::snapshot::FrameSnapshot;
use iot_core::drivers::camera::CameraSnapshot;
use iot_core::drivers::surface::{ScreenContext, ScreenPainter, ScreenTarget};
use iot_core::ui::{Canvas, Screen};

pub struct AppPainter {
    overlay: bool,
}

impl AppPainter {
    pub const fn new(overlay: bool) -> Self {
        Self { overlay }
    }

    fn panel<'a>(&self, target: ScreenTarget<'a>, context: ScreenContext) -> Panel<'a> {
        let canvas = Canvas::new(target.frame, target.width, target.height, target.ground);
        Panel::new(
            canvas,
            Screen {
                instance: context.instance,
                fps: context.fps,
                debug: self.overlay,
                spl_offset_db: context.spl_offset_db,
                probe: context.stack_probe,
            },
        )
    }
}

impl ScreenPainter for AppPainter {
    fn paint(&self, target: ScreenTarget<'_>, context: ScreenContext, snapshot: &dyn Any) {
        let Some(frame) = snapshot.downcast_ref::<FrameSnapshot>() else {
            debug_assert!(false, "the screen's snapshot is the frame it was handed");
            return;
        };
        paint(frame.page, frame, &mut self.panel(target, context));
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
        let mut panel = self.panel(target, context);
        panel.declare(frame, no_custom, |snapshot, ui| {
            CameraPage::view_frame(snapshot, ui, readout)
        });
    }
}
