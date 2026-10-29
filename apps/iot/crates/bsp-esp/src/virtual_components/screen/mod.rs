//! The screen: the panel, the one frame buffer the board gives it, and the sources that fill
//! those bytes.
//!
//! Two writers share the buffer and never run at once: the CPU paints a colour fill and stamps
//! the page over it, or the camera fills it and the portable surface stamps its readout on top.
//! That is why the buffer is the screen's and the sensor is bound to the screen rather than to
//! the glass — [`Panel`] is only the controller and its backlight.
//!
//! Pages are drawn by `iot-ui`; this crate decides when a frame is worth shipping and how much
//! of the shared executor a frame may hold.

use alloc::boxed::Box;

use iot_core::diagnostics::Diagnostics;
use iot_core::drivers::camera::{FrameOwner, FrameSource};
use iot_core::drivers::light::{Fill, Rgb, RgbLight};
use iot_core::state::DisplayPage;

use crate::components::backlight::Backlight;
use crate::components::st7789::St7789;
use crate::stack::main_stack_bytes;

mod camera;
mod metrics;
mod paint;
mod panel;

use self::camera::CameraFeed;
use self::metrics::Metrics;
use self::panel::Panel;

/// Minimum per-channel color delta that warrants a full-frame repaint.
const REPAINT_STEP: u8 = 12;

/// Debug overlay toggle. A compile-time switch (not a Cargo feature): the
/// ambient diagnostics rows and last-touch coordinates are a field/development
/// aid, so production builds keep them dark by setting this to `false`. The
/// attitude page remains visible in either mode.
const DEBUG_DIAGNOSTICS: bool = true;

/// ST7789 panel framed as an `RgbLight` surface.
///
/// The frame buffer is the board's, not this type's: it is a static because it has to outlive
/// the DMA transfer the peripheral keeps running against it, and because its size is part of
/// the board's memory budget rather than of what one image happens to ask the heap for. A
/// camera wired to the same screen fills those bytes, which is why the buffer is handed in
/// here rather than allocated.
pub struct Screen {
    /// Which light surface this panel renders (wiring order); the diagnostics
    /// snapshot carries every surface, so the panel reads its own row.
    instance: u8,
    /// The glass and its backlight: no buffer, no pages, no camera.
    panel: Panel,
    /// The board's frame buffer, shown by the panel and filled by whichever engine owns it.
    frame: &'static mut [u8],
    /// Which engine owns the frame buffer, and what the CPU last drew while it held it.
    /// Unconditional because the colour half is also how a panel with no camera decides
    /// whether a fill is worth shipping, so gating it would leave one half of the decision
    /// outside the value that holds it.
    owner: FrameOwner,
    /// The camera behind the Camera page, if this board mounted one.
    camera: CameraFeed,
    /// Diagnostic overlay payload: touch counters, light mode, and motion (see
    /// [`Diagnostics`]). A bump repaints via `paint`, which the
    /// [`FrameOwner::painted_color`] guard would otherwise skip.
    diagnostics: Diagnostics,
    /// Repaint rate and per-frame cost, so a log can say whether the panel is actually cheap
    /// to keep live.
    metrics: Metrics,
}

impl Screen {
    /// Takes the backlight dark and keeps it there until the first frame reaches
    /// the panel. The caller turns the LEDC channel off before the fallible part of
    /// bring-up; this type raises it only after a write succeeds.
    pub fn new(
        instance: u8,
        driver: St7789,
        backlight: Backlight,
        frame: &'static mut [u8],
    ) -> Self {
        log::info!(
            "[DISPLAY] backlight dark until the first frame; main stack {} B",
            main_stack_bytes()
        );
        Self {
            instance,
            panel: Panel::new(driver, backlight),
            frame,
            owner: FrameOwner::NeedsPaint,
            camera: CameraFeed::new(),
            diagnostics: Diagnostics::default(),
            metrics: Metrics::new(),
        }
    }

    /// This surface's whole buffer, for a board wiring a camera onto the same bytes.
    pub fn frame(&mut self) -> &mut [u8] {
        // The length is taken first: computing it inside the index would borrow `self` to read
        // the panel while the same borrow is handing out the buffer.
        let bytes = self.panel.frame_bytes();
        &mut self.frame[..bytes]
    }

    /// Binds the camera that fills this panel's buffer.
    ///
    /// Takes it by value and erases it to the trait, because the surface is its only
    /// consumer — a frame has nowhere else to be seen — and so that neither this signature
    /// nor the renderer's names a sensor.
    pub fn with_camera(mut self, camera: Box<dyn FrameSource>) -> Self {
        self.camera.attach(camera);
        self
    }
}

impl RgbLight for Screen {
    fn repaint_step(&self) -> u8 {
        REPAINT_STEP
    }

    fn set_backlight(&mut self, level_pct: u8) {
        self.panel.set_backlight(level_pct);
    }

    fn set_backlight_ceiling(&mut self, level_pct: u8) {
        self.panel.set_ceiling(level_pct);
    }

    fn set_fill(&mut self, fill: Fill, color: Rgb) {
        if !self.owner.accepts_fill(fill, color) {
            return;
        }
        self.paint(fill, color);
    }

    fn page_ground(&self, page: DisplayPage) -> Option<Rgb> {
        matches!(page, DisplayPage::Network | DisplayPage::Camera).then_some(Rgb(0, 0, 0))
    }

    fn page_draws_frames(&self, page: DisplayPage) -> bool {
        page == DisplayPage::Camera
    }
}

impl crate::hardware::SelfCheck for Screen {
    fn name(&self) -> &'static str {
        self.panel.name()
    }

    fn id(&self) -> Option<crate::hardware::Address> {
        self.panel.id()
    }

    fn check(&mut self) -> crate::hardware::Status {
        self.panel.check()
    }
}
