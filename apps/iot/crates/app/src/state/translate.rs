//! The input boundary: the bindings that answer from anywhere are answered here, before the
//! page is asked; each page's own bindings live beside its controller (`audio`, `speaker`,
//! `network`).
//!
//! It reads the whole device state, so it sits outside `model`: the model must not point back.

use crate::shared::intent::{
    BusinessIntent, InputIntent, LightAction, NavigationIntent, NetworkPageIntent,
};
use crate::state::{DeviceState, Page};
use iot_core::drivers::input::{ButtonEvent, GestureEvent, InputEvent};

use crate::pages::audio::controller as audio_controller;
use crate::pages::network::controller as network_controller;
use crate::pages::speaker::controller as speaker_controller;

/// What one gesture means on the page the device is showing.
pub fn translate(op: &InputIntent, current: &DeviceState) -> BusinessIntent {
    if let Some(intent) = shortcut(op, current) {
        return intent;
    }
    if current.page() != Page::Splash
        && matches!(op.event, InputEvent::Button(ButtonEvent::TripleClick))
    {
        return BusinessIntent::Navigation(NavigationIntent::NextPage);
    }
    match current.page() {
        Page::Ambient | Page::Attitude | Page::Camera => lamp(op, current),
        Page::Audio => audio_controller::bind(op, current),
        Page::Speaker => speaker_controller::bind(op, current),
        Page::Network => network_controller::bind(op, current),
        Page::Splash => match op.event {
            InputEvent::Button(ButtonEvent::LongPress) => {
                BusinessIntent::Navigation(NavigationIntent::RequestSetup)
            }
            _ => BusinessIntent::Invalid,
        },
    }
}

/// The lamp's table, for the pages whose only verb is the light.
fn lamp(op: &InputIntent, current: &DeviceState) -> BusinessIntent {
    match op.event {
        InputEvent::Button(ButtonEvent::Click) | InputEvent::Gesture(GestureEvent::Tap { .. }) => {
            current.light_intent(op, LightAction::NextColor { forward: true })
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

/// The setup-mode hold is a link decision, not a page one: switching the host is offered from
/// anywhere but the splash, so it is answered before the page sees the gesture.
fn shortcut(op: &InputIntent, current: &DeviceState) -> Option<BusinessIntent> {
    (current.page() != Page::Splash
        && current.network.link.mode.is_setup()
        && matches!(op.event, InputEvent::Button(ButtonEvent::LongPress)))
    .then_some(BusinessIntent::NetworkPage(NetworkPageIntent::SwitchHost))
}
