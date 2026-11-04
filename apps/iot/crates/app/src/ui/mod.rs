//! The product's ink: the pages, their shell, and what the board's panel asks them to draw.

mod layout;
mod network_text;
mod pages;
mod surface;
mod text;

pub use surface::Surface;

use core::any::Any;

use crate::diagnostics::Diagnostics;
use iot_core::drivers::camera::CameraSnapshot;
use iot_core::drivers::surface::{PanelContext, PanelPainter, PanelTarget};
use iot_ui::Canvas;

const DEBUG_DIAGNOSTICS: bool = true;

/// Paints the product's pages into a board-owned frame. Stateless: the snapshot arrives
/// with each call, so the panel never holds a product type.
pub struct AppPainter;

impl PanelPainter for AppPainter {
    fn paint(&self, target: PanelTarget<'_>, context: PanelContext, snapshot: &dyn Any) {
        let Some(diagnostics) = snapshot.downcast_ref::<Diagnostics>() else {
            return;
        };
        let canvas = Canvas::new(target.frame, target.width, target.height, target.ground);
        let mut surface = Surface::new(
            canvas,
            diagnostics,
            context.instance,
            context.fps,
            DEBUG_DIAGNOSTICS,
            context.spl_offset_db,
        );
        if let Some(probe) = context.stack_probe {
            surface = surface.with_stack_probe(probe);
        }
        surface.render(diagnostics.page);
    }

    fn paint_camera(
        &self,
        target: PanelTarget<'_>,
        readout: CameraSnapshot,
        context: PanelContext,
        snapshot: &dyn Any,
    ) {
        let Some(diagnostics) = snapshot.downcast_ref::<Diagnostics>() else {
            return;
        };
        let canvas = Canvas::new(target.frame, target.width, target.height, target.ground);
        let mut surface = Surface::new(
            canvas,
            diagnostics,
            context.instance,
            context.fps,
            DEBUG_DIAGNOSTICS,
            context.spl_offset_db,
        );
        surface.stamp_camera(readout);
    }
}
