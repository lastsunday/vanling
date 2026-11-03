//! The surfaces a board hands the render layer, split by what they can show.

use alloc::boxed::Box;
use core::future::Future;
use core::pin::Pin;

use crate::diagnostics::DiagnosticsSink;
use crate::drivers::camera::CameraTarget;
use crate::paint::Rgb;

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
pub trait PageSurface: RgbLight + DiagnosticsSink + CameraTarget {
    fn set_backlight(&mut self, _level_pct: u8) {}

    fn set_backlight_ceiling(&mut self, _level_pct: u8) {}

    fn repaint_step(&self) -> u8 {
        0
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
