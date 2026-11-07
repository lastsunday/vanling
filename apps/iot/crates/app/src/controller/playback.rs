//! The Speaker page's gestures: a tap walks the catalogue, a hold mutes, and the button and
//! swipes are the lamp's, as on every page without a verb of its own for them.

use crate::model::intent::{ActionIntent, InputIntent, PlaybackAction};
use crate::model::state::DeviceState;
use iot_core::drivers::input::{ButtonEvent, GestureEvent, InputEvent};

use super::light;

pub(super) fn translate(op: &InputIntent, current: &DeviceState) -> ActionIntent {
    match op.event {
        InputEvent::Button(ButtonEvent::Click) | InputEvent::Gesture(GestureEvent::Tap { .. }) => {
            ActionIntent::Playback(PlaybackAction::Next)
        }
        InputEvent::Button(ButtonEvent::LongPress)
        | InputEvent::Gesture(GestureEvent::LongPress { .. }) => {
            ActionIntent::Playback(PlaybackAction::SetMuted(!current.playback.muted))
        }
        InputEvent::Button(ButtonEvent::DoubleClick) => light::step(current, op.source),
        InputEvent::Gesture(GestureEvent::Swipe { direction, .. }) => {
            light::swipe(current, op.source, direction)
        }
        _ => ActionIntent::Invalid,
    }
}
