//! The Speaker page's gestures: a tap walks the catalogue, a hold mutes, and the button and
//! swipes are the lamp's, as on every page without a verb of its own for them.

use crate::shared::intent::{BusinessIntent, InputIntent, LightAction, PlaybackAction};
use crate::shared::model::DeviceState;
use iot_core::drivers::input::{ButtonEvent, GestureEvent, InputEvent};

pub(crate) fn bind(op: &InputIntent, current: &DeviceState) -> BusinessIntent {
    match op.event {
        InputEvent::Button(ButtonEvent::Click) | InputEvent::Gesture(GestureEvent::Tap { .. }) => {
            BusinessIntent::Playback(PlaybackAction::Next)
        }
        InputEvent::Button(ButtonEvent::LongPress)
        | InputEvent::Gesture(GestureEvent::LongPress { .. }) => {
            BusinessIntent::Playback(PlaybackAction::SetMuted(!current.playback.muted))
        }
        InputEvent::Button(ButtonEvent::DoubleClick) => current.light_intent(op, LightAction::Step),
        InputEvent::Gesture(GestureEvent::Swipe { direction, .. }) => {
            current.light_swipe(op, direction)
        }
        _ => BusinessIntent::Invalid,
    }
}
