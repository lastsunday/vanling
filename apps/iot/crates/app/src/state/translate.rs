//! The input boundary: the bindings that answer from anywhere are answered here, before the
//! page is asked. Each page's own bindings live beside it, in
//! [`LocalPage::bind`](crate::services::LocalPage::bind).
//!
//! It reads the whole device state, so it sits outside `model`: the model must not point back.

use crate::services::Services;
use crate::shared::intent::{BusinessIntent, InputIntent, NavigationIntent, NetworkIntent};
use crate::state::{DeviceState, PageId};
use iot_core::drivers::input::{ButtonEvent, InputEvent};

/// What one gesture means on the page the device is showing. `None` when it means nothing.
pub fn translate(
    op: &InputIntent,
    current: &DeviceState,
    services: &Services,
) -> Option<BusinessIntent> {
    if let Some(intent) = shortcut(op, current) {
        return Some(intent);
    }
    if current.page() != PageId::Splash
        && matches!(op.event, InputEvent::Button(ButtonEvent::TripleClick))
    {
        return Some(BusinessIntent::Navigation(NavigationIntent::NextPage));
    }
    services.get(current.page()).bind(op, current)
}

/// The setup-mode hold is a link decision, not a page one: switching the host is offered from
/// anywhere but the splash, so it is answered before the page sees the gesture.
fn shortcut(op: &InputIntent, current: &DeviceState) -> Option<BusinessIntent> {
    (current.page() != PageId::Splash
        && current.network.link.mode.is_setup()
        && matches!(op.event, InputEvent::Button(ButtonEvent::LongPress)))
    .then_some(BusinessIntent::Network(NetworkIntent::SwitchHost))
}
