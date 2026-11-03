//! What one gesture means on each page. Translation reads the whole device state, so it
//! lives beside that state rather than in `page`: the page table is a leaf the state
//! depends on, and the arrow must not point back.

use crate::drivers::audio::AudioPhase;
use crate::drivers::input::{ButtonEvent, GestureEvent, InputEvent, SwipeDirection};
use crate::drivers::network::AccessPoint;
use crate::intent::{
    ActionIntent, AudioIntent, InputIntent, NetworkPageIntent, PageIntent, PlaybackAction,
};
use crate::keyboard;

use super::{Band, Confirm, DeviceState, DisplayPage, controls, light_control};

/// What one gesture means on the page the device is showing.
pub fn translate(op: &InputIntent, current: &DeviceState) -> ActionIntent {
    if current.page != DisplayPage::Splash
        && current.network.link.mode.is_setup()
        && matches!(op.event, InputEvent::Button(ButtonEvent::LongPress))
    {
        return ActionIntent::NetworkPage(NetworkPageIntent::SwitchHost);
    }
    if current.page != DisplayPage::Splash
        && matches!(op.event, InputEvent::Button(ButtonEvent::TripleClick))
    {
        return ActionIntent::Page(PageIntent::NextPage);
    }
    match current.page {
        DisplayPage::Ambient | DisplayPage::Attitude | DisplayPage::Camera => {
            light_control::translate(op, current)
        }
        DisplayPage::Audio => audio(op, current),
        DisplayPage::Speaker => speaker(op, current),
        DisplayPage::Network => network(op, current),
        DisplayPage::Splash => splash(op, current),
    }
}

fn audio(op: &InputIntent, current: &DeviceState) -> ActionIntent {
    match op.event {
        InputEvent::Button(ButtonEvent::Click) | InputEvent::Gesture(GestureEvent::Tap { .. }) => {
            ActionIntent::Audio(capture_target(current.audio.phase))
        }
        InputEvent::Button(ButtonEvent::DoubleClick) => light_control::step(current, op.source),
        InputEvent::Button(ButtonEvent::LongPress) => light_control::cycle(current, op.source),
        InputEvent::Gesture(GestureEvent::LongPress { .. }) => {
            light_control::cycle_panel(current, op.source)
        }
        InputEvent::Gesture(GestureEvent::Swipe { direction, .. }) => {
            light_control::swipe(current, op.source, direction)
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

fn speaker(op: &InputIntent, current: &DeviceState) -> ActionIntent {
    match op.event {
        InputEvent::Button(ButtonEvent::Click) | InputEvent::Gesture(GestureEvent::Tap { .. }) => {
            ActionIntent::Playback(PlaybackAction::Next)
        }
        InputEvent::Button(ButtonEvent::LongPress)
        | InputEvent::Gesture(GestureEvent::LongPress { .. }) => {
            ActionIntent::Playback(PlaybackAction::SetMuted(!current.playback.muted))
        }
        InputEvent::Button(ButtonEvent::DoubleClick) => light_control::step(current, op.source),
        InputEvent::Gesture(GestureEvent::Swipe { direction, .. }) => {
            light_control::swipe(current, op.source, direction)
        }
        _ => ActionIntent::Invalid,
    }
}

fn splash(op: &InputIntent, _current: &DeviceState) -> ActionIntent {
    match op.event {
        InputEvent::Button(ButtonEvent::LongPress) => ActionIntent::Page(PageIntent::RequestSetup),
        _ => ActionIntent::Invalid,
    }
}

fn network(op: &InputIntent, current: &DeviceState) -> ActionIntent {
    match op.event {
        InputEvent::Gesture(GestureEvent::Tap { x, y, at_ms, .. }) => {
            network_tap(current, x, y, at_ms)
        }
        InputEvent::Gesture(GestureEvent::LongPress { x, y, .. }) => network_hold(current, x, y),
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

fn network_tap(current: &DeviceState, x: u16, y: u16, at_ms: u32) -> ActionIntent {
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

fn network_hold(current: &DeviceState, x: u16, y: u16) -> ActionIntent {
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
