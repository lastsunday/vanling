//! What one gesture means on the Network page: the keyboard, the list, the two dialogs, and
//! the setup-mode hold that answers from any page but the splash.

use crate::drivers::input::{ButtonEvent, GestureEvent, InputEvent, SwipeDirection};
use crate::drivers::network::AccessPoint;
use crate::intent::{ActionIntent, InputIntent, NetworkPageIntent};
use crate::keyboard;
use crate::state::{Band, Confirm, DeviceState, DisplayPage, controls};

/// The setup-mode hold is a link decision, not a page one: switching the host is offered from
/// anywhere but the splash, so it is answered before the page sees the gesture.
pub(in crate::state) fn shortcut(op: &InputIntent, current: &DeviceState) -> Option<ActionIntent> {
    (current.page() != DisplayPage::Splash
        && current.network.link.mode.is_setup()
        && matches!(op.event, InputEvent::Button(ButtonEvent::LongPress)))
    .then_some(ActionIntent::NetworkPage(NetworkPageIntent::SwitchHost))
}

pub(in crate::state) fn translate(op: &InputIntent, current: &DeviceState) -> ActionIntent {
    match op.event {
        InputEvent::Gesture(GestureEvent::Tap { x, y, at_ms, .. }) => tap(current, x, y, at_ms),
        InputEvent::Gesture(GestureEvent::LongPress { x, y, .. }) => hold(current, x, y),
        InputEvent::Gesture(GestureEvent::Swipe { direction, .. })
            if !current.network_page.keyboard =>
        {
            if current.network_page.confirm.is_some() {
                return ActionIntent::NetworkPage(NetworkPageIntent::Dismiss);
            }
            if !Band::Content.showing(current.network_page, current.network.link) {
                return ActionIntent::Invalid;
            }
            match direction {
                SwipeDirection::Up => {
                    ActionIntent::NetworkPage(NetworkPageIntent::Scroll { rows: 1 })
                }
                SwipeDirection::Down => {
                    ActionIntent::NetworkPage(NetworkPageIntent::Scroll { rows: -1 })
                }
                _ => ActionIntent::Invalid,
            }
        }
        InputEvent::Button(ButtonEvent::Click) => {
            controls::strip(current.network_page, current.network.link)
                .command(keyboard::ToolbarCell::Right)
        }
        _ => ActionIntent::Invalid,
    }
}

fn tap(current: &DeviceState, x: u16, y: u16, at_ms: u32) -> ActionIntent {
    if current.network_page.keyboard {
        let Some(entry) = current.network_page.entry else {
            return ActionIntent::Invalid;
        };
        return keyboard::key_at(entry.layer(), x, y).map_or(ActionIntent::Invalid, |key| {
            ActionIntent::NetworkPage(NetworkPageIntent::Key { key, at_ms })
        });
    }
    if let Some(answer) = answer_at(current, x, y) {
        return answer;
    }
    if let Some(cell) = keyboard::toolbar_cell_at(x, y) {
        return controls::strip(current.network_page, current.network.link).command(cell);
    }
    if !Band::Content.showing(current.network_page, current.network.link) {
        return ActionIntent::Invalid;
    }
    match keyboard::list_band_at(y) {
        Some(band) => ap_at(current, band).map_or(ActionIntent::Invalid, |ap| {
            ActionIntent::NetworkPage(NetworkPageIntent::Choose(ap))
        }),
        None => ActionIntent::Invalid,
    }
}

fn hold(current: &DeviceState, x: u16, y: u16) -> ActionIntent {
    if current.network_page.keyboard {
        return ActionIntent::Invalid;
    }
    if let Some(answer) = answer_at(current, x, y) {
        return answer;
    }
    if !Band::Content.showing(current.network_page, current.network.link) {
        return ActionIntent::Invalid;
    }
    match keyboard::list_band_at(y) {
        Some(band) => ap_at(current, band).map_or(ActionIntent::Invalid, |ap| {
            ActionIntent::NetworkPage(NetworkPageIntent::AskForget(ap))
        }),
        None => ActionIntent::Invalid,
    }
}

fn answer_at(current: &DeviceState, x: u16, y: u16) -> Option<ActionIntent> {
    if current.network.link.shows_setup_box() {
        let answers = controls::answers(current.network.link);
        return match keyboard::dialog_choice_at(x, y) {
            Some(keyboard::DialogChoice::Keep) => Some(answers.keep.command),
            Some(keyboard::DialogChoice::Act) => Some(answers.act.command),
            None => None,
        };
    }
    let confirm: Confirm = current.network_page.confirm?;
    Some(match keyboard::dialog_choice_at(x, y) {
        Some(keyboard::DialogChoice::Act) => {
            ActionIntent::NetworkPage(NetworkPageIntent::Answer(confirm))
        }
        Some(keyboard::DialogChoice::Keep) | None => {
            ActionIntent::NetworkPage(NetworkPageIntent::Dismiss)
        }
    })
}

fn ap_at(current: &DeviceState, band: usize) -> Option<AccessPoint> {
    let index = usize::from(current.network_page.list_top) + band;
    current.network.aps.and_then(|list| list.get(index))
}
