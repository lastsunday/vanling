//! What one gesture means on each page. Translation reads the whole device state, so it lives
//! outside `model`: the model owns the record and the page table is a leaf it depends on, and
//! the arrow must not point back.
//!
//! Each page's own gestures live with the translation that reads them (`audio`, `playback`,
//! `network`, `light`); this module only answers the two things that are not a page's: the
//! setup-mode hold and the triple click, then dispatches.

use crate::shared::intent::{ActionIntent, InputIntent, PageIntent};
use crate::state::{DeviceState, Page};
use iot_core::drivers::input::{ButtonEvent, InputEvent};

use crate::pages::audio::controller as audio_controller;
use crate::pages::network::controller as network_controller;
use crate::pages::speaker::controller as speaker_controller;
use crate::shared::light;

/// What one gesture means on the page the device is showing.
pub fn translate(op: &InputIntent, current: &DeviceState) -> ActionIntent {
    if let Some(intent) = network_controller::shortcut(op, current) {
        return intent;
    }
    if current.page() != Page::Splash
        && matches!(op.event, InputEvent::Button(ButtonEvent::TripleClick))
    {
        return ActionIntent::Page(PageIntent::NextPage);
    }
    match current.page() {
        Page::Ambient | Page::Attitude | Page::Camera => light::translate(op, current),
        Page::Audio => audio_controller::translate(op, current),
        Page::Speaker => speaker_controller::translate(op, current),
        Page::Network => network_controller::translate(op, current),
        Page::Splash => match op.event {
            InputEvent::Button(ButtonEvent::LongPress) => {
                ActionIntent::Page(PageIntent::RequestSetup)
            }
            _ => ActionIntent::Invalid,
        },
    }
}
