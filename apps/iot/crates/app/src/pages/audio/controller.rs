//! The Audio page's gestures: a tap starts or stops the capture, and everything else is the
//! lamp's, as on every page without a verb of its own for it.

use crate::shared::intent::{ActionIntent, AudioIntent, InputIntent};
use crate::shared::model::AudioPhase;
use crate::shared::model::DeviceState;
use iot_core::drivers::input::{ButtonEvent, GestureEvent, InputEvent};

use crate::shared::gesture::light;

pub(crate) fn bind(op: &InputIntent, current: &DeviceState) -> ActionIntent {
    match op.event {
        InputEvent::Button(ButtonEvent::Click) | InputEvent::Gesture(GestureEvent::Tap { .. }) => {
            ActionIntent::Audio(capture_target(current.audio.phase))
        }
        InputEvent::Button(ButtonEvent::DoubleClick) => light::step(current, op.source),
        InputEvent::Button(ButtonEvent::LongPress) => light::cycle(current, op.source),
        InputEvent::Gesture(GestureEvent::LongPress { .. }) => {
            light::cycle_panel(current, op.source)
        }
        InputEvent::Gesture(GestureEvent::Swipe { direction, .. }) => {
            light::swipe(current, op.source, direction)
        }
        _ => ActionIntent::Invalid,
    }
}

fn capture_target(phase: AudioPhase) -> AudioIntent {
    if phase == AudioPhase::Recording {
        AudioIntent::Stop
    } else {
        AudioIntent::Start
    }
}
