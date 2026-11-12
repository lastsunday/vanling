//! The lamp's input table: which verb each event asks for, and which surface answers it.

use super::*;
use crate::shared::intent::{LightAction, recognize};
use crate::shared::model::PALETTE;
use crate::shared::route::test_support::{DEFAULT_BREATH, state, swipe_event, tap_gesture};

fn two_lights() -> DeviceState {
    DeviceState {
        lights: [
            LightState::Off,
            LightState::Solid {
                color: PALETTE[0],
                brightness: 140,
            },
        ],
        ..state(DEFAULT_BREATH)
    }
}

fn lamp_intent(event: InputEvent, current: &DeviceState) -> BusinessIntent {
    translate(&recognize(event, 0), current)
}

#[test]
fn a_button_asks_for_one_verb_per_click() {
    let ambient = state(DEFAULT_BREATH);
    for (event, action) in [
        (
            InputEvent::Button(ButtonEvent::Click),
            LightAction::NextColor { forward: true },
        ),
        (
            InputEvent::Button(ButtonEvent::DoubleClick),
            LightAction::Step,
        ),
        (
            InputEvent::Button(ButtonEvent::LongPress),
            LightAction::Cycle,
        ),
    ] {
        assert_eq!(
            lamp_intent(event, &ambient),
            BusinessIntent::Light {
                instance: 0,
                action,
            },
            "{event:?}"
        );
    }
}

#[test]
fn a_touch_asks_the_panel_surface() {
    let ambient = state(DEFAULT_BREATH);
    assert_eq!(
        lamp_intent(tap_gesture(0), &ambient),
        BusinessIntent::Light {
            instance: 0,
            action: LightAction::NextColor { forward: true },
        }
    );
    assert_eq!(
        translate(&long_press(), &ambient),
        BusinessIntent::Light {
            instance: 0,
            action: LightAction::Cycle,
        }
    );
    for (direction, action) in [
        (
            SwipeDirection::Left,
            LightAction::NextColor { forward: false },
        ),
        (
            SwipeDirection::Right,
            LightAction::NextColor { forward: true },
        ),
        (SwipeDirection::Up, LightAction::Brightness { up: true }),
        (SwipeDirection::Down, LightAction::Brightness { up: false }),
    ] {
        assert_eq!(
            lamp_intent(swipe_event(direction), &ambient),
            BusinessIntent::Light {
                instance: 0,
                action,
            },
            "{direction:?}"
        );
    }
}

#[test]
fn a_diagonal_swipe_asks_for_nothing() {
    for direction in [
        SwipeDirection::UpLeft,
        SwipeDirection::UpRight,
        SwipeDirection::DownLeft,
        SwipeDirection::DownRight,
    ] {
        assert_eq!(
            lamp_intent(swipe_event(direction), &state(DEFAULT_BREATH)),
            BusinessIntent::Invalid,
            "{direction:?} has no axis"
        );
    }
}

#[test]
fn off_answers_only_the_cycle() {
    let off = state(LightState::Off);
    for event in [
        InputEvent::Button(ButtonEvent::Click),
        InputEvent::Button(ButtonEvent::DoubleClick),
        swipe_event(SwipeDirection::Up),
        swipe_event(SwipeDirection::Right),
        tap_gesture(0),
    ] {
        assert_eq!(
            lamp_intent(event, &off),
            BusinessIntent::Invalid,
            "{event:?} has nothing to move"
        );
    }
    assert_eq!(
        translate(&long_press(), &off),
        BusinessIntent::Light {
            instance: 0,
            action: LightAction::Cycle,
        }
    );
}

#[test]
fn a_button_drives_its_own_surface_and_clamps() {
    assert_eq!(
        translate(
            &recognize(InputEvent::Button(ButtonEvent::Click), 1),
            &two_lights()
        ),
        BusinessIntent::Light {
            instance: 1,
            action: LightAction::NextColor { forward: true },
        },
        "each button drives the surface its wiring order names"
    );
    assert_eq!(
        translate(
            &recognize(InputEvent::Button(ButtonEvent::DoubleClick), 7),
            &two_lights()
        ),
        BusinessIntent::Light {
            instance: 1,
            action: LightAction::Step,
        },
        "out-of-range sources clamp to the last surface"
    );
}

#[test]
fn a_tap_only_touches_the_first_surface() {
    assert_eq!(
        translate(&tap(), &two_lights()),
        BusinessIntent::Invalid,
        "the panel's surface is off, and a tap never reaches the second one"
    );
}
