//! The lamp: one surface and the verb it is asked for. The instance is routing, not part of the
//! verb — the record's own `apply` only ever sees the action.

use crate::shared::intent::BusinessIntent;
use crate::shared::model::{Command, DeviceState, Effects};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LightService;

impl super::Service for LightService {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        state: &mut DeviceState,
        _layout: &iot_core::ui::Layout,
    ) -> (Option<Command>, Effects) {
        if let BusinessIntent::Light { instance, action } = intent {
            let index = usize::from(DeviceState::light_index(*instance));
            if let Some(next) = state.lights[index].apply(*action) {
                state.lights[index] = next;
            }
        }
        (None, Effects::default())
    }
}
