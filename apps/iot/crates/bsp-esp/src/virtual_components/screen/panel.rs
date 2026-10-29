//! The glass: one controller, one backlight, and the write that reaches them.

use crate::components::backlight::Backlight;
use crate::components::st7789::St7789;

pub(super) struct Panel {
    driver: St7789,
    backlight: Backlight,
    ceiling: u8,
    requested: u8,
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

    pub(super) fn note_frame_shipped(&mut self) {
        if !self.lit {
            self.lit = true;
            self.backlight
                .set_level_pct(self.requested.min(self.ceiling));
        }
    }
}

impl crate::hardware::SelfCheck for Panel {
    fn name(&self) -> &'static str {
        self.driver.name()
    }

    fn id(&self) -> Option<crate::hardware::Address> {
        self.driver.id()
    }

    fn check(&mut self) -> crate::hardware::Status {
        self.driver.check()
    }
}
