//! The Audio page's gestures: a tap starts or stops the capture, and everything else is the
//! lamp's, as on every page without a verb of its own for it.

use crate::shared::intent::{AudioIntent, BusinessIntent, InputIntent, LightAction};
use crate::shared::model::AudioPhase;
use crate::shared::model::DeviceState;
use iot_core::drivers::input::{ButtonEvent, GestureEvent, InputEvent};

pub(crate) fn bind(op: &InputIntent, current: &DeviceState) -> BusinessIntent {
    match op.event {
        InputEvent::Button(ButtonEvent::Click) | InputEvent::Gesture(GestureEvent::Tap { .. }) => {
            BusinessIntent::Audio(capture_target(current.audio.phase))
        }
        InputEvent::Button(ButtonEvent::DoubleClick) => current.light_intent(op, LightAction::Step),
        InputEvent::Button(ButtonEvent::LongPress)
        | InputEvent::Gesture(GestureEvent::LongPress { .. }) => {
            current.light_intent(op, LightAction::Cycle)
        }
        InputEvent::Gesture(GestureEvent::Swipe { direction, .. }) => {
            current.light_swipe(op, direction)
        }
        _ => BusinessIntent::Invalid,
    }
}

fn capture_target(phase: AudioPhase) -> AudioIntent {
    if phase == AudioPhase::Recording {
        AudioIntent::Stop
    } else {
        AudioIntent::Start
    }
}
