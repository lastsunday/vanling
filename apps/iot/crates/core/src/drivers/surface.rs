//! The surfaces a board hands the render layer, split by what they can show.

use alloc::boxed::Box;

use crate::diagnostics::DiagnosticsSink;
use crate::drivers::camera::CameraTarget;
use crate::paint::Rgb;
use crate::state::SurfaceRole;

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
