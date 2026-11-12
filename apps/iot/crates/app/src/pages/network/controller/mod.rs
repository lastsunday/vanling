//! What one gesture means on the Network page: the keyboard, the list, and the two dialogs.
//!
//! The bands and the strip are read by the view but are not state the model owns.

pub mod bands;
pub mod verbs;

pub use self::bands::{Band, each_band, joining};
pub use self::verbs::{Answers, Control, ControlKind, Strip, answers, confirm_verb, strip};

use self::verbs as page_controls;
use crate::pages::network::layout;
use crate::shared::intent::Confirm;
use crate::shared::intent::{BusinessIntent, InputIntent, NetworkPageIntent};
use crate::shared::model::DeviceState;
use iot_core::drivers::input::{ButtonEvent, GestureEvent, InputEvent, SwipeDirection};
use iot_core::drivers::network::AccessPoint;

pub(crate) fn bind(op: &InputIntent, current: &DeviceState) -> BusinessIntent {
    match op.event {
        InputEvent::Gesture(GestureEvent::Tap { x, y, at_ms, .. }) => tap(current, x, y, at_ms),
        InputEvent::Gesture(GestureEvent::LongPress { x, y, .. }) => hold(current, x, y),
        InputEvent::Gesture(GestureEvent::Swipe { direction, .. })
            if !current.network_page.keyboard =>
        {
            if current.network_page.confirm.is_some() {
                return BusinessIntent::NetworkPage(NetworkPageIntent::Dismiss);
            }
            if !Band::Content.showing(current.network_page, current.network.link) {
                return BusinessIntent::Invalid;
            }
            match direction {
                SwipeDirection::Up => {
                    BusinessIntent::NetworkPage(NetworkPageIntent::Scroll { rows: 1 })
                }
                SwipeDirection::Down => {
                    BusinessIntent::NetworkPage(NetworkPageIntent::Scroll { rows: -1 })
                }
                _ => BusinessIntent::Invalid,
            }
        }
        InputEvent::Button(ButtonEvent::Click) => {
            page_controls::strip(current.network_page, current.network.link)
                .command(layout::ToolbarCell::Right)
        }
        _ => BusinessIntent::Invalid,
    }
}

fn tap(current: &DeviceState, x: u16, y: u16, at_ms: u32) -> BusinessIntent {
    if current.network_page.keyboard {
        let Some(entry) = current.network_page.entry else {
            return BusinessIntent::Invalid;
        };
        return layout::key_at(entry.layer(), x, y).map_or(BusinessIntent::Invalid, |key| {
            BusinessIntent::NetworkPage(NetworkPageIntent::Key { key, at_ms })
        });
    }
    if let Some(answer) = answer_at(current, x, y) {
        return answer;
    }
    if let Some(cell) = layout::toolbar_cell_at(x, y) {
        return page_controls::strip(current.network_page, current.network.link).command(cell);
    }
    if !Band::Content.showing(current.network_page, current.network.link) {
        return BusinessIntent::Invalid;
    }
    match layout::list_band_at(y) {
        Some(band) => ap_at(current, band).map_or(BusinessIntent::Invalid, |ap| {
            BusinessIntent::NetworkPage(NetworkPageIntent::Choose(ap))
        }),
        None => BusinessIntent::Invalid,
    }
}

fn hold(current: &DeviceState, x: u16, y: u16) -> BusinessIntent {
    if current.network_page.keyboard {
        return BusinessIntent::Invalid;
    }
    if let Some(answer) = answer_at(current, x, y) {
        return answer;
    }
    if !Band::Content.showing(current.network_page, current.network.link) {
        return BusinessIntent::Invalid;
    }
    match layout::list_band_at(y) {
        Some(band) => ap_at(current, band).map_or(BusinessIntent::Invalid, |ap| {
            BusinessIntent::NetworkPage(NetworkPageIntent::AskForget(ap))
        }),
        None => BusinessIntent::Invalid,
    }
}

fn answer_at(current: &DeviceState, x: u16, y: u16) -> Option<BusinessIntent> {
    if current.network.link.shows_setup_box() {
        let answers = page_controls::answers(current.network.link);
        return match layout::dialog_choice_at(x, y) {
            Some(layout::DialogChoice::Keep) => Some(answers.keep.command),
            Some(layout::DialogChoice::Act) => Some(answers.act.command),
            None => None,
        };
    }
    let confirm: Confirm = current.network_page.confirm?;
    Some(match layout::dialog_choice_at(x, y) {
        Some(layout::DialogChoice::Act) => {
            BusinessIntent::NetworkPage(NetworkPageIntent::Answer(confirm))
        }
        Some(layout::DialogChoice::Keep) | None => {
            BusinessIntent::NetworkPage(NetworkPageIntent::Dismiss)
        }
    })
}

fn ap_at(current: &DeviceState, band: usize) -> Option<AccessPoint> {
    let index = usize::from(current.network_page.list_top) + band;
    current.network.aps.and_then(|list| list.get(index))
}
