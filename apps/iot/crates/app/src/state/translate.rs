//! The input boundary: the bindings that answer from anywhere are answered here, before the
//! page is asked; each page's own bindings live beside its controller (`audio`, `speaker`,
//! `network`, and the three pages that share `gesture::light`).
//!
//! It reads the whole device state, so it sits outside `model`: the model must not point back.

use crate::shared::intent::{ActionIntent, InputIntent, NetworkPageIntent, PageIntent};
use crate::state::{DeviceState, Page};
use iot_core::drivers::input::{ButtonEvent, InputEvent};

use crate::pages::audio::controller as audio_controller;
use crate::pages::network::controller as network_controller;
use crate::pages::speaker::controller as speaker_controller;
use crate::shared::gesture::light;

/// What one gesture means on the page the device is showing.
pub fn translate(op: &InputIntent, current: &DeviceState) -> ActionIntent {
    if let Some(intent) = shortcut(op, current) {
        return intent;
    }
    if current.page() != Page::Splash
        && matches!(op.event, InputEvent::Button(ButtonEvent::TripleClick))
    {
        return ActionIntent::Page(PageIntent::NextPage);
    }
    match current.page() {
        Page::Ambient | Page::Attitude | Page::Camera => light::bind(op, current),
        Page::Audio => audio_controller::bind(op, current),
        Page::Speaker => speaker_controller::bind(op, current),
        Page::Network => network_controller::bind(op, current),
        Page::Splash => match op.event {
            InputEvent::Button(ButtonEvent::LongPress) => {
                ActionIntent::Page(PageIntent::RequestSetup)
            }
            _ => ActionIntent::Invalid,
        },
    }
}

/// The setup-mode hold is a link decision, not a page one: switching the host is offered from
/// anywhere but the splash, so it is answered before the page sees the gesture.
fn shortcut(op: &InputIntent, current: &DeviceState) -> Option<ActionIntent> {
    (current.page() != Page::Splash
        && current.network.link.mode.is_setup()
        && matches!(op.event, InputEvent::Button(ButtonEvent::LongPress)))
    .then_some(ActionIntent::NetworkDraft(NetworkPageIntent::SwitchHost))
}
