//! What one gesture means on each page. Translation reads the whole device state, so it lives
//! outside `model`: the model owns the record and the page table is a leaf it depends on, and
//! the arrow must not point back.
//!
//! Each page's own gestures live with the translation that reads them (`audio`, `playback`,
//! `network`, `light`); this module only answers the two things that are not a page's: the
//! setup-mode hold and the triple click, then dispatches.

use crate::model::intent::{ActionIntent, InputIntent, PageIntent};
use crate::model::state::{DeviceState, DisplayPage};
use iot_core::drivers::input::{ButtonEvent, InputEvent};

use crate::controller::audio;
use crate::controller::light;
use crate::controller::network;
use crate::controller::playback;

/// What one gesture means on the page the device is showing.
pub fn translate(op: &InputIntent, current: &DeviceState) -> ActionIntent {
    if let Some(intent) = network::shortcut(op, current) {
        return intent;
    }
    if current.page() != DisplayPage::Splash
        && matches!(op.event, InputEvent::Button(ButtonEvent::TripleClick))
    {
        return ActionIntent::Page(PageIntent::NextPage);
    }
    match current.page() {
        DisplayPage::Ambient | DisplayPage::Attitude | DisplayPage::Camera => {
            light::translate(op, current)
        }
        DisplayPage::Audio => audio::translate(op, current),
        DisplayPage::Speaker => playback::translate(op, current),
        DisplayPage::Network => network::translate(op, current),
        DisplayPage::Splash => match op.event {
            InputEvent::Button(ButtonEvent::LongPress) => {
                ActionIntent::Page(PageIntent::RequestSetup)
            }
            _ => ActionIntent::Invalid,
        },
    }
}
