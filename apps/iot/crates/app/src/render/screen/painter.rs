//! Paints the product's pages into a board-owned frame. Stateless: the snapshot arrives
//! with each call, so the panel never holds a product type.

use core::any::Any;

use crate::services::camera::CameraPage;
use crate::services::paint;
use crate::shared::kit::painter::Painter;
use crate::shared::snapshot::FrameSnapshot;
use iot_core::drivers::camera::CameraSnapshot;
use iot_core::drivers::surface::{PanelContext, PanelPainter, PanelTarget};
use iot_core::ui::Canvas;

const DEBUG_OVERLAY: bool = true;

pub struct AppPainter;

impl PanelPainter for AppPainter {
    fn paint(&self, target: PanelTarget<'_>, context: PanelContext, snapshot: &dyn Any) {
        let Some(frame) = snapshot.downcast_ref::<FrameSnapshot>() else {
            return;
        };
        let canvas = Canvas::new(target.frame, target.width, target.height, target.ground);
        let mut painter = Painter::new(
            canvas,
            context.instance,
            context.fps,
            DEBUG_OVERLAY,
            context.spl_offset_db,
        );
        if let Some(probe) = context.stack_probe {
            painter = painter.with_stack_probe(probe);
        }
        paint(frame.page, frame, &mut painter);
    }

    fn paint_camera(
        &self,
        target: PanelTarget<'_>,
        readout: CameraSnapshot,
        context: PanelContext,
        snapshot: &dyn Any,
    ) {
        let Some(frame) = snapshot.downcast_ref::<FrameSnapshot>() else {
            return;
        };
        let canvas = Canvas::new(target.frame, target.width, target.height, target.ground);
        let mut painter = Painter::new(
            canvas,
            context.instance,
            context.fps,
            DEBUG_OVERLAY,
            context.spl_offset_db,
        );
        CameraPage::paint_frame(frame, &mut painter, readout);
    }
}
