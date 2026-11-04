//! The screen: the panel, the one frame buffer the board gives it, and the sources that
//! fill those bytes.

use alloc::boxed::Box;

use esp_hal::time::Duration;
use iot_core::drivers::camera::FrameOwner;
use iot_core::drivers::surface::RgbLight;
use iot_core::paint::Rgb;

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

pub use self::panel::{PanelBus, panel_service};

const REPAINT_STEP: u8 = 12;

/// A frame the last paint left in the buffer, waiting for the bus.
pub(super) struct Pending {
    /// Whether the camera drew it: its failures are logged apart and do not force a repaint.
    pub(super) camera: bool,
    pub(super) render: Duration,
}

/// ST7789 panel carrying the pages, the backlight and the camera's frame buffer.
pub struct Screen {
    instance: u8,
    panel: Panel,
    frame: &'static mut [u8],
    owner: FrameOwner,
    camera: CameraFeed,
    metrics: Metrics,
    pending: Option<Pending>,
    /// A fill landed after the last page was drawn, so the next `repaint` has work even
    /// when the product's own gate says nothing moved.
    page_dirty: bool,
}

impl Screen {
    /// Takes the backlight dark and keeps it there until the first frame reaches the panel.
    ///
    /// The bus comes back out because it belongs on the other core: see [`PanelBus`].
    pub fn new(
        instance: u8,
        driver: St7789,
        backlight: Backlight,
        frame: &'static mut [u8],
    ) -> (Self, PanelBus) {
        log::info!(
            "[DISPLAY] backlight dark until the first frame; main stack {} B",
            main_stack_bytes()
        );
        let (panel, bus) = Panel::new(driver, backlight);
        (
            Self {
                instance,
                panel,
                frame,
                owner: FrameOwner::NeedsPaint,
                camera: CameraFeed::new(),
                metrics: Metrics::new(),
                pending: None,
                page_dirty: true,
            },
            bus,
        )
    }

    /// This surface's whole buffer, for a board wiring a camera onto the same bytes.
    pub fn frame(&mut self) -> &mut [u8] {
        let bytes = self.panel.frame_bytes();
        &mut self.frame[..bytes]
    }
}

impl RgbLight for Screen {
    fn set_rgb(&mut self, color: Rgb) {
        if !self.owner.accepts_color(color) {
            return;
        }
        self.fill(color);
        self.page_dirty = true;
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
