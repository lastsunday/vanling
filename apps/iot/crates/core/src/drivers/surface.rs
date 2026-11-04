//! The surfaces a board hands the render layer, split by what they can show.

use alloc::boxed::Box;
use core::any::Any;
use core::future::Future;
use core::pin::Pin;

use crate::drivers::camera::{CameraSnapshot, CameraTarget};
use crate::paint::Rgb;

/// The frame a board hands to the product's painter: the bytes it owns, and the colour the
/// light already put under them. Nothing here names a page or a snapshot type.
pub struct PanelTarget<'a> {
    pub frame: &'a mut [u8],
    pub width: usize,
    pub height: usize,
    /// The colour the light painted under the page, when the buffer still holds one.
    pub ground: Option<Rgb>,
}

/// What the panel knows about itself when it asks for one frame.
#[derive(Clone, Copy)]
pub struct PanelContext {
    pub instance: u8,
    pub fps: u8,
    /// The codec's calibration, owned by the board that wired it.
    pub spl_offset_db: i16,
    /// Sampled while the paint path's deepest working set is live, when the board has one.
    pub stack_probe: Option<fn()>,
}

/// Product-side drawing for a panel. The board owns the buffer; the app decides the ink.
pub trait PanelPainter {
    fn paint(&self, target: PanelTarget<'_>, context: PanelContext, snapshot: &dyn Any);

    fn paint_camera(
        &self,
        target: PanelTarget<'_>,
        readout: CameraSnapshot,
        context: PanelContext,
        snapshot: &dyn Any,
    );
}

/// Surface slots one device can carry, dimensioning the state's lights and the render slots.
pub const MAX_LIGHTS: usize = 2;

/// What one wired surface is for: pages, ambient light, or nothing at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SurfaceRole {
    /// Draws the pages and puts the questions on them.
    #[default]
    Pages,
    /// Shows light only; the link's news lands here when there is no page to say it.
    Lamp,
    /// This slot carries no surface.
    Absent,
}

/// A surface that accepts one colour.
pub trait RgbLight {
    fn set_rgb(&mut self, color: Rgb);
}

/// A colour surface that also carries pages: it owns a backlight, throttles its own
/// repaints, draws the page over the colour, and is the only kind that can take the camera.
pub trait PageSurface: RgbLight + CameraTarget {
    fn set_backlight(&mut self, _level_pct: u8) {}

    fn set_backlight_ceiling(&mut self, _level_pct: u8) {}

    fn repaint_step(&self) -> u8 {
        0
    }

    /// Draws the page the snapshot describes over the colour the last fill left. The board
    /// supplies its own frame and the app's painter decides the ink. `force` redraws even
    /// when no colour moved; the product's repaint gate is what asks for that. Answers
    /// whether a frame is now pending for [`Self::flush`].
    fn repaint(&mut self, _painter: &dyn PanelPainter, _snapshot: &dyn Any, _force: bool) -> bool {
        false
    }

    /// Ships whatever the last paint left in the buffer, if anything. Separate from painting
    /// because the panel's transfer is a DMA operation: awaiting it lets the executor run
    /// between chunks instead of holding it for the whole frame. Surfaces without a bus, and
    /// surfaces with nothing to send, answer `None` rather than a future that does nothing.
    fn flush<'a>(&'a mut self) -> Option<Pin<Box<dyn Future<Output = ()> + 'a>>> {
        None
    }
}

/// One surface as the board hands it over, with the role the wiring gave it.
///
/// A lamp is only asked for colour; nothing here can be mistaken for a panel, so an LED
/// never has to answer for a backlight or a camera it does not have.
pub enum WiredSurface {
    Pages(Box<dyn PageSurface>),
    Lamp(Box<dyn RgbLight>),
}

impl WiredSurface {
    pub const fn role(&self) -> SurfaceRole {
        match self {
            Self::Pages(_) => SurfaceRole::Pages,
            Self::Lamp(_) => SurfaceRole::Lamp,
        }
    }
}
