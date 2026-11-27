//! The surfaces a board hands the render layer, split by what they can show.

use alloc::boxed::Box;
use core::any::Any;
use core::future::Future;
use core::pin::Pin;

use crate::drivers::camera::{CameraSnapshot, CameraTarget};
use crate::light::Rgb;

/// The frame a board hands to the product's painter: the bytes it owns, and the colour the
/// light already put under them. Nothing here names a page or a snapshot type.
pub struct ScreenTarget<'a> {
    pub frame: &'a mut [u8],
    pub width: usize,
    pub height: usize,
    /// The colour the light painted under the page, when the buffer still holds one.
    pub ground: Option<Rgb>,
}

/// What the screen knows about itself when it asks for one frame.
#[derive(Clone, Copy)]
pub struct ScreenContext {
    pub instance: u8,
    pub fps: u8,
    /// The codec's calibration, owned by the board that wired it.
    pub spl_offset_db: i16,
    /// Sampled while the paint path's deepest working set is live, when the board has one.
    pub stack_probe: Option<fn()>,
}

/// One rectangle of a frame, in panel pixels. Ends are exclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DamageRect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl DamageRect {
    pub const fn new(x: u16, y: u16, w: u16, h: u16) -> Self {
        Self { x, y, w, h }
    }

    pub const fn right(&self) -> u16 {
        self.x + self.w
    }

    pub const fn bottom(&self) -> u16 {
        self.y + self.h
    }

    pub const fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }
}

/// The pixels one paint wrote: a few disjoint rectangles, or the whole frame.
///
/// The surface ships only these, so a scroll of two text regions costs a few kilobytes on the
/// wire instead of the frame's 153 600. `FULL` is the safe answer a painter that does not
/// describe its pixels gets; `NONE` says nothing was drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameDamage {
    rects: [DamageRect; MAX_DAMAGE_RECTS],
    len: u8,
    full: bool,
}

/// Rects one paint may describe. The text page's scroll frame needs seven: two boxes, two
/// headers, two thumbs and the debug badge's footer band.
pub const MAX_DAMAGE_RECTS: usize = 7;

impl FrameDamage {
    pub const NONE: Self = Self {
        rects: [DamageRect::new(0, 0, 0, 0); MAX_DAMAGE_RECTS],
        len: 0,
        full: false,
    };

    pub const FULL: Self = Self {
        rects: [DamageRect::new(0, 0, 0, 0); MAX_DAMAGE_RECTS],
        len: 0,
        full: true,
    };

    pub const fn is_full(&self) -> bool {
        self.full
    }

    pub fn rects(&self) -> &[DamageRect] {
        &self.rects[..usize::from(self.len)]
    }

    /// Adds one rectangle. More than [`MAX_DAMAGE_RECTS`] degrades to the whole frame, which is
    /// slower but never wrong.
    pub fn add(&mut self, rect: DamageRect) {
        if self.full || rect.w == 0 || rect.h == 0 {
            return;
        }
        let Some(slot) = self.rects.get_mut(usize::from(self.len)) else {
            *self = Self::FULL;
            return;
        };
        *slot = rect;
        self.len += 1;
    }
}

impl Default for FrameDamage {
    fn default() -> Self {
        Self::NONE
    }
}

/// Product-side drawing for a screen. The board owns the buffer; the app decides what is drawn.
pub trait ScreenPainter {
    fn paint(&mut self, target: ScreenTarget<'_>, context: ScreenContext, snapshot: &dyn Any);

    fn paint_camera(
        &mut self,
        target: ScreenTarget<'_>,
        readout: CameraSnapshot,
        context: ScreenContext,
        snapshot: &dyn Any,
    );

    /// The pixels the panel has to receive for this frame, which can be more than the ones the
    /// last `paint` drew: a blit moves rows the page never draws again. Defaults to the whole
    /// frame, which every surface can ship.
    fn damage(&self) -> FrameDamage {
        FrameDamage::FULL
    }

    /// Whether the next `paint` may start from the pixels the last one left instead of a fresh
    /// fill. Only a painter that knows the frame still holds its own last picture says yes.
    fn keeps_frame(&mut self, _snapshot: &dyn Any) -> bool {
        false
    }

    /// The panel's rectangles from this painter's last frame, for the hit test that runs before
    /// the next one. A painter that does not keep them answers `None`.
    fn layout(&self) -> Option<crate::ui::Layout> {
        None
    }

    /// Tells the painter the buffer no longer holds its last picture, so the next `paint` has
    /// to draw every pixel it needs instead of scrolling the ones already there.
    fn reset(&mut self) {}
}

/// Surface slots one device can carry, dimensioning the state's lights and the render slots.
pub const MAX_LIGHTS: usize = 2;

/// What one wired surface is for: a screen, ambient light, or nothing at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SurfaceRole {
    /// Draws the frame the product paints.
    #[default]
    Screen,
    /// Shows light only; the link's news lands here when there is no page to say it.
    Lamp,
    /// This slot carries no surface.
    Absent,
}

/// A surface that accepts one colour.
pub trait RgbLight {
    fn set_rgb(&mut self, color: Rgb);
}

/// A colour surface that also draws frames: it owns a backlight, throttles its own repaints,
/// draws the frame over the colour, and is the only kind that can take the camera.
pub trait ScreenSurface: RgbLight + CameraTarget {
    fn set_backlight(&mut self, _level_pct: u8) {}

    fn set_backlight_ceiling(&mut self, _level_pct: u8) {}

    fn repaint_step(&self) -> u8 {
        0
    }

    /// Draws the frame the snapshot describes over the colour the last fill left. The board
    /// supplies its buffer and the app's painter decides what is drawn. `force` redraws even when no
    /// colour moved — the product's repaint gate asks for that. Answers whether a frame is now
    /// pending for [`Self::flush`].
    ///
    /// Required rather than defaulted: a screen surface that cannot draw a frame is a wiring
    /// mistake, and a `false` default would hide it behind a screen that never fills.
    fn repaint(&mut self, painter: &mut dyn ScreenPainter, snapshot: &dyn Any, force: bool)
    -> bool;

    /// Ships whatever the last paint left in the buffer, if anything. Separate from painting
    /// because the screen's transfer is a DMA operation: awaiting it lets the executor run
    /// between chunks instead of holding it for the whole frame. Surfaces without a bus, and
    /// surfaces with nothing to send, answer `None` rather than a future that does nothing.
    fn flush<'a>(&'a mut self) -> Option<Pin<Box<dyn Future<Output = ()> + 'a>>> {
        None
    }
}

/// One surface as the board hands it over, with the role the wiring gave it.
///
/// A lamp is only asked for colour; nothing here can be mistaken for a screen, so an LED
/// never has to answer for a backlight or a camera it does not have.
pub enum WiredSurface {
    Screen(Box<dyn ScreenSurface>),
    Lamp(Box<dyn RgbLight>),
}

impl WiredSurface {
    pub const fn role(&self) -> SurfaceRole {
        match self {
            Self::Screen(_) => SurfaceRole::Screen,
            Self::Lamp(_) => SurfaceRole::Lamp,
        }
    }
}
