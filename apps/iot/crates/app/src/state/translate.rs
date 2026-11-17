//! The input boundary: the bindings that answer from anywhere are answered here, before the
//! page is asked. Each page's own bindings live beside it, in [`Page::bind`].
//!
//! It reads the whole device state, so it sits outside `model`: the model must not point back.

use crate::pages::PageSet;
use crate::shared::intent::{BusinessIntent, InputIntent, NavigationIntent, NetworkPageIntent};
use crate::state::{DeviceState, PageId};
use iot_core::drivers::input::{ButtonEvent, InputEvent};

/// What one gesture means on the page the device is showing.
pub fn translate(op: &InputIntent, current: &DeviceState, pages: &PageSet) -> BusinessIntent {
    if let Some(intent) = shortcut(op, current) {
        return intent;
    }
    if current.page() != PageId::Splash
        && matches!(op.event, InputEvent::Button(ButtonEvent::TripleClick))
    {
        return BusinessIntent::Navigation(NavigationIntent::NextPage);
    }
    pages.get(current.page()).bind(op, current)
}

/// The setup-mode hold is a link decision, not a page one: switching the host is offered from
/// anywhere but the splash, so it is answered before the page sees the gesture.
fn shortcut(op: &InputIntent, current: &DeviceState) -> Option<BusinessIntent> {
    (current.page() != PageId::Splash
        && current.network.link.mode.is_setup()
        && matches!(op.event, InputEvent::Button(ButtonEvent::LongPress)))
    .then_some(BusinessIntent::NetworkPage(NetworkPageIntent::SwitchHost))
}
