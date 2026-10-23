//! The glass: one controller, one backlight, and the write that reaches them.
//!
//! Nothing here knows about pages, the frame buffer or a camera: the buffer is the screen's
//! and travels in on every write, so this type never owns bytes another engine may fill.

use crate::components::backlight::Backlight;
use crate::components::st7789::St7789;

/// One mounted panel, with the backlight policy that keeps its first visible frame honest.
pub(super) struct Panel {
    driver: St7789,
    backlight: Backlight,
    /// Ceiling every `set_backlight` is clamped to, so a demand can hold the panel dim
    /// without the animation loop having to know it.
    ceiling: u8,
    /// The backlight level the renderer last asked for. Kept aside until the first
    /// frame reaches the panel: applying it earlier lights the previous run's GRAM.
    requested: u8,
    /// Whether a frame has actually been written since boot, which is what allows
    /// [`Self::requested`] to reach the LEDC channel.
    lit: bool,
}

impl Panel {
    pub(super) fn new(driver: St7789, backlight: Backlight) -> Self {
        Self {
            driver,
            backlight,
            ceiling: 100,
            requested: 0,
            lit: false,
        }
    }

    pub(super) fn width(&self) -> u16 {
        self.driver.width()
    }

    pub(super) fn height(&self) -> u16 {
        self.driver.height()
    }

    pub(super) fn frame_bytes(&self) -> usize {
        usize::from(self.width()) * usize::from(self.height()) * 2
    }

    pub(super) fn write(&mut self, frame: &[u8]) -> Result<(), esp_hal::spi::Error> {
        self.driver.write_frame(frame)
    }

    pub(super) fn set_backlight(&mut self, level_pct: u8) {
        self.requested = level_pct.min(100);
        if self.lit {
            self.backlight
                .set_level_pct(self.requested.min(self.ceiling));
        }
    }

    pub(super) fn set_ceiling(&mut self, level_pct: u8) {
        self.ceiling = level_pct.min(100);
        if self.lit {
            self.backlight
                .set_level_pct(self.requested.min(self.ceiling));
        }
    }

    /// Raises the backlight once a frame has actually reached the panel.
    ///
    /// Until then the controller is still scanning whatever its own GRAM held
    /// before this boot, so lighting it early would present that picture as this
    /// run's first frame.
    pub(super) fn note_frame_shipped(&mut self) {
        if !self.lit {
            self.lit = true;
            self.backlight
                .set_level_pct(self.requested.min(self.ceiling));
        }
    }
}
