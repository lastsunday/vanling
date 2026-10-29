//! The screen: the panel, the one frame buffer the board gives it, and the sources that
//! fill those bytes.

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

const REPAINT_STEP: u8 = 12;

const DEBUG_DIAGNOSTICS: bool = true;

/// ST7789 panel framed as an `RgbLight` surface.
pub struct Screen {
    instance: u8,
    panel: Panel,
    frame: &'static mut [u8],
    owner: FrameOwner,
    camera: CameraFeed,
    diagnostics: Diagnostics,
    metrics: Metrics,
}

impl Screen {
    /// Takes the backlight dark and keeps it there until the first frame reaches the panel.
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
        let bytes = self.panel.frame_bytes();
        &mut self.frame[..bytes]
    }

    /// Binds the camera that fills this panel's buffer.
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
