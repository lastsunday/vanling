//! The boot page: the product's name over the hold that opens setup mode.

use iot_core::overlay::{FONT_H, GLYPH_PITCH, OVERLAY_GAP, PANEL_COLUMNS};

use crate::ui::surface::Surface;

impl Surface<'_> {
    pub(crate) fn stamp_splash(&mut self) {
        let lines: [&[u8]; SPLASH_LINES] = [b"VANLING", b"HOLD BOOT", b"SETUP"];
        let block = (SPLASH_LINES - 1) * SPLASH_PITCH + FONT_H;
        let top = self.height().saturating_sub(block) / 2;
        for (line, text) in lines.iter().enumerate() {
            let ink = text.len() * GLYPH_PITCH - OVERLAY_GAP;
            let left = self.width().saturating_sub(ink) / 2;
            self.stamp_text(text, left, top + line * SPLASH_PITCH);
        }
    }
}

const SPLASH_LINES: usize = 3;

const SPLASH_PITCH: usize = 3 * FONT_H;

const _: () = assert!(
    b"HOLD BOOT".len() * GLYPH_PITCH - OVERLAY_GAP <= PANEL_COLUMNS,
    "the widest splash line has to fit the panel"
);
