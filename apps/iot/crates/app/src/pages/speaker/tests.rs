//! The Speaker page's input contract: a tap plays, a hold mutes, and no gesture here moves a
//! light.

use crate::shared::intent::PlaybackAction;
use crate::state::test_support::*;

fn on_speaker() -> DeviceState {
    state_on(PageId::Speaker, BREATH)
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
        translate(&op(tap_gesture(0)), &on_speaker(), &PageSet::default()),
        BusinessIntent::Playback(PlaybackAction::Next)
    );
    assert_eq!(
        translate(
            &op(long_press_gesture()),
            &on_speaker(),
            &PageSet::default()
        ),
        BusinessIntent::Playback(PlaybackAction::SetMuted(true))
    );
    assert_eq!(
        translate(
            &op(InputEvent::Button(ButtonEvent::Click)),
            &on_speaker(),
            &PageSet::default()
        ),
        BusinessIntent::Playback(PlaybackAction::Next)
    );
    assert_eq!(
        translate(&op(long_press_event()), &on_speaker(), &PageSet::default()),
        BusinessIntent::Playback(PlaybackAction::SetMuted(true))
    );
}

#[test]
fn the_page_never_moves_a_light() {
    assert_eq!(
        target_in(InputEvent::Button(ButtonEvent::DoubleClick), &on_speaker()),
        target(InputEvent::Button(ButtonEvent::DoubleClick), BREATH),
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
            target(swipe_event(direction), BREATH),
            "{direction:?} still moves the light"
        );
    }
    assert_eq!(
        translate(
            &op(InputEvent::Button(ButtonEvent::TripleClick)),
            &on_speaker(),
            &PageSet::default()
        ),
        BusinessIntent::Navigation(NavigationIntent::NextPage)
    );
}

#[test]
fn a_tap_off_the_speaker_page_never_plays() {
    assert_eq!(
        target(tap_gesture(0), BREATH),
        Some(LightAction::NextColor { forward: true })
    );
}

#[test]
fn a_finished_play_is_never_recognized_from_an_event() {
    for event in [
        InputEvent::Button(ButtonEvent::Click),
        long_press_event(),
        tap_gesture(0),
        long_press_gesture(),
    ] {
        assert_ne!(
            translate(&op(event), &on_speaker(), &PageSet::default()),
            BusinessIntent::Playback(PlaybackAction::Finished),
            "{event:?} must not fake a driver report"
        );
    }
}
