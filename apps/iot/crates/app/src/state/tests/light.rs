//! The lamp's input table: which verb each event asks for, and which surface answers it.

use super::*;
use crate::shared::intent::{LightAction, recognize};
use crate::shared::model::PALETTE;

fn two_lights() -> DeviceState {
    DeviceState {
        lights: [
            LightState::Off,
            LightState::Solid {
                color: PALETTE[0],
                brightness: 140,
            },
        ],
        ..state_on(PageId::Headless, BREATH)
    }
}

fn lamp_intent(event: InputEvent, current: &DeviceState) -> Option<BusinessIntent> {
    translate(&recognize(event, 0), current, &Services::default())
}

#[test]
fn a_button_asks_for_one_verb_per_click() {
    let lamp = state_on(PageId::Headless, BREATH);
    for (event, action) in [
        (
            InputEvent::Button(ButtonEvent::Click),
            LightAction::NextEffect,
        ),
        (
            InputEvent::Button(ButtonEvent::DoubleClick),
            LightAction::NextColor { forward: true },
        ),
        (
            InputEvent::Button(ButtonEvent::TripleClick),
            LightAction::Brightness { up: true },
        ),
        (
            InputEvent::Button(ButtonEvent::LongPress),
            LightAction::Brightness { up: true },
        ),
    ] {
        assert_eq!(
            lamp_intent(event, &lamp),
            Some(BusinessIntent::Light {
                instance: 0,
                action,
            }),
            "{event:?}"
        );
    }
}

#[test]
fn a_touch_asks_the_panel_surface() {
    let lamp = state_on(PageId::Headless, BREATH);
    assert_eq!(
        lamp_intent(tap_gesture(0), &lamp),
        Some(BusinessIntent::Light {
            instance: 0,
            action: LightAction::NextEffect,
        })
    );
    assert_eq!(
        translate(&long_press(), &lamp, &Services::default()),
        Some(BusinessIntent::Light {
            instance: 0,
            action: LightAction::Brightness { up: true },
        })
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
            lamp_intent(swipe_event(direction), &lamp),
            Some(BusinessIntent::Light {
                instance: 0,
                action,
            }),
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
            lamp_intent(swipe_event(direction), &state_on(PageId::Headless, BREATH)),
            None,
            "{direction:?} has no axis"
        );
    }
}

#[test]
fn off_answers_only_the_effects_that_can_light_it() {
    let off = state_on(PageId::Headless, LightState::Off);
    for event in [InputEvent::Button(ButtonEvent::Click), tap_gesture(0)] {
        assert_eq!(
            lamp_intent(event, &off),
            Some(BusinessIntent::Light {
                instance: 0,
                action: LightAction::NextEffect,
            }),
            "{event:?} turns the lamp back on"
        );
    }
    for event in [
        InputEvent::Button(ButtonEvent::DoubleClick),
        swipe_event(SwipeDirection::Down),
        swipe_event(SwipeDirection::Right),
    ] {
        assert_eq!(
            lamp_intent(event, &off),
            None,
            "{event:?} has nothing to move"
        );
    }
    for event in [
        InputEvent::Button(ButtonEvent::TripleClick),
        swipe_event(SwipeDirection::Up),
    ] {
        assert_eq!(
            lamp_intent(event, &off),
            Some(BusinessIntent::Light {
                instance: 0,
                action: LightAction::Brightness { up: true },
            }),
            "{event:?} lights a dark lamp at the dial's foot"
        );
    }
    assert_eq!(
        translate(&long_press(), &off, &Services::default()),
        Some(BusinessIntent::Light {
            instance: 0,
            action: LightAction::Brightness { up: true },
        }),
        "the dial lights a dark lamp at its foot"
    );
}

#[test]
fn a_button_drives_its_own_surface_and_clamps() {
    assert_eq!(
        translate(
            &recognize(InputEvent::Button(ButtonEvent::Click), 1),
            &two_lights(),
            &Services::default()
        ),
        Some(BusinessIntent::Light {
            instance: 1,
            action: LightAction::NextEffect,
        }),
        "each button drives the surface its wiring order names"
    );
    assert_eq!(
        translate(
            &recognize(InputEvent::Button(ButtonEvent::DoubleClick), 7),
            &two_lights(),
            &Services::default()
        ),
        Some(BusinessIntent::Light {
            instance: 1,
            action: LightAction::NextColor { forward: true },
        }),
        "out-of-range sources clamp to the last surface"
    );
}

#[test]
fn a_tap_only_touches_the_first_surface() {
    assert_eq!(
        translate(&tap(), &two_lights(), &Services::default()),
        Some(BusinessIntent::Light {
            instance: 0,
            action: LightAction::NextEffect,
        }),
        "the panel's surface answers even while dark, and never the second one"
    );
}
