use super::{MOTION_BACKGROUND, ON_SCREEN};
use crate::demand::{Claim, DemandKind};
use crate::diagnostics::Diagnostics;
use crate::drivers::input::{ButtonEvent, GestureEvent, InputEvent};
use crate::intent::{BusinessIntent, OperationIntent, PlaybackAction};
use crate::page::light;
use crate::state::DeviceState;

pub(super) const fn claim(kind: DemandKind) -> Claim {
    match kind {
        DemandKind::Motion => MOTION_BACKGROUND,
        DemandKind::Display => ON_SCREEN,
        DemandKind::Interaction | DemandKind::Audio | DemandKind::Camera => Claim::off(),
    }
}

pub(super) fn translate(op: &OperationIntent, current: &DeviceState) -> BusinessIntent {
    match op.event {
        InputEvent::Button(ButtonEvent::Click) | InputEvent::Gesture(GestureEvent::Tap { .. }) => {
            BusinessIntent::Playback(PlaybackAction::Next)
        }
        InputEvent::Button(ButtonEvent::LongPress)
        | InputEvent::Gesture(GestureEvent::LongPress { .. }) => {
            BusinessIntent::Playback(PlaybackAction::SetMuted(!current.playback.muted))
        }
        InputEvent::Button(ButtonEvent::DoubleClick) => light::step(current, op.source),
        InputEvent::Gesture(GestureEvent::Swipe { direction, .. }) => {
            light::swipe(current, op.source, direction)
        }
        _ => BusinessIntent::Invalid,
    }
}

pub(super) fn repaint(prev: &Diagnostics, next: &Diagnostics, _overlay: bool) -> bool {
    prev.playback != next.playback
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::input::SwipeDirection;
    use crate::intent::{PageIntent, translate};
    use crate::page::light::COLOR_GROUPS;
    use crate::page::test_support::{
        BREATH_BASE, DEFAULT_BREATH, long_press, op, state_on, swipe_event, tap_gesture, target,
        target_in,
    };
    use crate::state::{DisplayPage, LightState};

    mod speaker_page {
        use super::*;

        fn on_speaker() -> DeviceState {
            state_on(DisplayPage::Speaker, DEFAULT_BREATH)
        }

        fn long_press_gesture() -> InputEvent {
            InputEvent::Gesture(GestureEvent::LongPress {
                id: 0,
                x: 1,
                y: 2,
                held_ms: 800,
            })
        }

        #[test]
        fn a_tap_plays_and_a_long_press_mutes() {
            assert_eq!(
                translate(&op(tap_gesture(0)), &on_speaker()),
                BusinessIntent::Playback(PlaybackAction::Next)
            );
            assert_eq!(
                translate(&op(long_press_gesture()), &on_speaker()),
                BusinessIntent::Playback(PlaybackAction::SetMuted(true))
            );
            assert_eq!(
                translate(&op(InputEvent::Button(ButtonEvent::Click)), &on_speaker()),
                BusinessIntent::Playback(PlaybackAction::Next)
            );
            assert_eq!(
                translate(&op(long_press()), &on_speaker()),
                BusinessIntent::Playback(PlaybackAction::SetMuted(true))
            );
        }

        #[test]
        fn the_page_never_moves_a_light() {
            assert_eq!(
                target_in(InputEvent::Button(ButtonEvent::DoubleClick), &on_speaker()),
                target(InputEvent::Button(ButtonEvent::DoubleClick), DEFAULT_BREATH),
                "the double-click still steps the table"
            );
            for direction in [
                SwipeDirection::Up,
                SwipeDirection::Down,
                SwipeDirection::Left,
                SwipeDirection::Right,
            ] {
                assert_eq!(
                    target_in(swipe_event(direction), &on_speaker()),
                    target(swipe_event(direction), DEFAULT_BREATH),
                    "{direction:?} still moves the light"
                );
            }
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::TripleClick)),
                    &on_speaker()
                ),
                BusinessIntent::Page(PageIntent::NextPage)
            );
        }

        #[test]
        fn a_tap_off_the_speaker_page_never_plays() {
            assert_eq!(
                target(tap_gesture(0), DEFAULT_BREATH),
                Some(LightState::Breath(COLOR_GROUPS[1].into_breath(BREATH_BASE)))
            );
        }

        #[test]
        fn a_finished_play_is_never_recognized_from_an_event() {
            for event in [
                InputEvent::Button(ButtonEvent::Click),
                long_press(),
                tap_gesture(0),
                long_press_gesture(),
            ] {
                assert_ne!(
                    translate(&op(event), &on_speaker()),
                    BusinessIntent::Playback(PlaybackAction::Finished),
                    "{event:?} must not fake a driver report"
                );
            }
        }
    }
}
