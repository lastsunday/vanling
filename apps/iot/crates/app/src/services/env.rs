//! The Env page: one line of text for now, and no gesture of its own.

use crate::shared::i18n::Msg;
use crate::shared::intent::{BusinessIntent, InputIntent};
use crate::shared::model::DeviceState;
use crate::shared::page;
use crate::shared::snapshot::FrameSnapshot;
use iot_core::ui::Ui;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvPage;

impl super::Service for EnvPage {}

impl super::LocalPage for EnvPage {
    fn bind(
        &self,
        _op: &InputIntent,
        _state: &DeviceState,
        _layout: &iot_core::ui::Layout,
    ) -> Option<BusinessIntent> {
        None
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        page::body(ui, |ui| page::title(ui, Msg::EnvTitle.bytes(snapshot.lang)));
    }

    fn descriptor() -> super::Descriptor {
        super::Descriptor::new(Self::view).with_repaints(super::Repaint::Overlay)
    }
}
