//! What one gesture means on each page. Translation reads the whole device state, so it
//! lives beside that state rather than in `page`: the page table is a leaf the state
//! depends on, and the arrow must not point back.
//!
//! Each page's own gestures live with the state they read (`audio`, `playback`,
//! `network_page`, `light_control`); this module only answers the two things that are not a
//! page's: the setup-mode hold and the triple click, then dispatches.

use crate::model::intent::{ActionIntent, InputIntent, PageIntent};
use iot_core::drivers::input::{ButtonEvent, InputEvent};

use super::{DeviceState, DisplayPage, audio, light_control, network_page, playback};

/// What one gesture means on the page the device is showing.
pub fn translate(op: &InputIntent, current: &DeviceState) -> ActionIntent {
    if let Some(intent) = network_page::translate::shortcut(op, current) {
        return intent;
    }
    if current.page() != DisplayPage::Splash
        && matches!(op.event, InputEvent::Button(ButtonEvent::TripleClick))
    {
        return ActionIntent::Page(PageIntent::NextPage);
    }
    match current.page() {
        DisplayPage::Ambient | DisplayPage::Attitude | DisplayPage::Camera => {
            light_control::translate(op, current)
        }
        DisplayPage::Audio => audio::translate(op, current),
        DisplayPage::Speaker => playback::translate(op, current),
        DisplayPage::Network => network_page::translate::translate(op, current),
        DisplayPage::Splash => match op.event {
            InputEvent::Button(ButtonEvent::LongPress) => {
                ActionIntent::Page(PageIntent::RequestSetup)
            }
            _ => ActionIntent::Invalid,
        },
    }
}
