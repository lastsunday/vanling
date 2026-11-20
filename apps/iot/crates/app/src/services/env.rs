//! The Env page: one line of text for now, and no gesture of its own.

use crate::shared::intent::{BusinessIntent, InputIntent};
use crate::shared::kit::chrome;
use crate::shared::model::DeviceState;
use crate::shared::snapshot::FrameSnapshot;
use iot_core::ui::Ui;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvPage;

impl super::Service for EnvPage {}

impl super::LocalPage for EnvPage {
    fn bind(&self, _op: &InputIntent, _state: &DeviceState) -> Option<BusinessIntent> {
        None
    }

    fn view(_snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        chrome::page(ui, |ui| chrome::title(ui, b"Env"));
    }

    fn descriptor() -> super::Descriptor {
        super::Descriptor::new(Self::view).with_repaints(super::Repaint::Overlay)
    }
}
