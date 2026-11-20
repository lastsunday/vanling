//! The boot page: the product's name over the hold that opens setup mode.

use crate::shared::intent::{BusinessIntent, InputIntent, NavigationIntent};
use crate::shared::model::DeviceState;
use crate::shared::snapshot::FrameSnapshot;
use iot_core::drivers::input::{ButtonEvent, InputEvent};
use iot_core::ui::panel::{CONTENT_BOTTOM, PANEL_COLUMNS};
use iot_core::ui::{Align, Style, Ui};

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

    fn view(_snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        ui.column(
            Style::new()
                .size(PANEL_COLUMNS as i16, CONTENT_BOTTOM as i16)
                .center()
                .gap(24),
            |ui| {
                for line in [b"VANLING".as_slice(), b"HOLD BOOT", b"SETUP"] {
                    ui.text(line, Style::new().nowrap().text(Align::Center));
                }
            },
        );
    }

    fn descriptor() -> super::Descriptor {
        super::Descriptor::new(Self::view)
    }
}
