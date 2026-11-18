//! The boot page: the product's name over the hold that opens setup mode.

use iot_core::overlay::{FONT_H, GLYPH_PITCH, OVERLAY_GAP, PANEL_COLUMNS};

use crate::shared::intent::{BusinessIntent, InputIntent, NavigationIntent};
use crate::shared::kit::painter::Painter;
use crate::shared::model::DeviceState;
use crate::shared::snapshot::FrameSnapshot;
use iot_core::drivers::input::{ButtonEvent, InputEvent};

/// The splash holds one question — a boot hold asks for setup — and the product's name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SplashPage;

impl super::Service for SplashPage {}

impl super::LocalPage for SplashPage {
    fn bind(&self, op: &InputIntent, _state: &DeviceState) -> Option<BusinessIntent> {
        match op.event {
            InputEvent::Button(ButtonEvent::LongPress) => {
                Some(BusinessIntent::Navigation(NavigationIntent::RequestSetup))
            }
            _ => None,
        }
    }

    fn paint(_snapshot: &FrameSnapshot, painter: &mut Painter<'_>) {
        let lines: [&[u8]; SPLASH_LINES] = [b"VANLING", b"HOLD BOOT", b"SETUP"];
        let block = (SPLASH_LINES - 1) * SPLASH_PITCH + FONT_H;
        let top = painter.height().saturating_sub(block) / 2;
        for (line, text) in lines.iter().enumerate() {
            let ink = text.len() * GLYPH_PITCH - OVERLAY_GAP;
            let left = painter.width().saturating_sub(ink) / 2;
            painter.draw_text(text, left, top + line * SPLASH_PITCH);
        }
    }

    fn descriptor() -> super::Descriptor {
        super::Descriptor::new(Self::paint)
    }
}

const SPLASH_LINES: usize = 3;

const SPLASH_PITCH: usize = 3 * FONT_H;

const _: () = assert!(
    b"HOLD BOOT".len() * GLYPH_PITCH - OVERLAY_GAP <= PANEL_COLUMNS,
    "the widest splash line has to fit the panel"
);
