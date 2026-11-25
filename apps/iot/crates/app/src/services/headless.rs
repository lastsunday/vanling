//! The board with no panel: the button and the lamp are the whole interface.

use iot_core::drivers::input::{ButtonEvent, GestureEvent, InputEvent};
use iot_core::ui::Ui;

use crate::shared::intent::{BusinessIntent, InputIntent, LightAction};
use crate::shared::model::DeviceState;
use crate::shared::snapshot::FrameSnapshot;

use super::Descriptor;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeadlessPage;

impl super::Service for HeadlessPage {}

impl super::LocalPage for HeadlessPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> Option<BusinessIntent> {
        match op.event {
            InputEvent::Button(ButtonEvent::Click)
            | InputEvent::Gesture(GestureEvent::Tap { .. }) => {
                state.light_intent(op, LightAction::NextColor { forward: true })
            }
            InputEvent::Button(ButtonEvent::DoubleClick) => {
                state.light_intent(op, LightAction::Step)
            }
            InputEvent::Button(ButtonEvent::LongPress)
            | InputEvent::Gesture(GestureEvent::LongPress { .. }) => {
                state.light_intent(op, LightAction::Cycle)
            }
            InputEvent::Gesture(GestureEvent::Swipe { direction, .. }) => {
                state.light_swipe(op, direction)
            }
            _ => None,
        }
    }

    fn view(_: &FrameSnapshot, _: &mut Ui<'_, '_>) {}

    fn descriptor() -> Descriptor {
        Descriptor::new(Self::view)
    }
}
