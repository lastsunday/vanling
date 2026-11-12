pub(crate) use super::*;
pub(crate) use crate::shared::intent::{BusinessIntent, InputIntent, Intent, NavigationIntent};
pub(crate) use crate::shared::route::CYCLE;
pub(crate) use crate::state::translate::translate;
pub(crate) use iot_core::drivers::input::{
    ButtonEvent, GestureEvent, InputEvent, MAX_TOUCH_POINTS, SwipeDirection, TouchEvent,
    TouchPoint, TouchStatus,
};
pub(crate) use iot_core::drivers::motion::MotionScale;
pub(crate) use iot_core::drivers::network::{Ipv4, SavedNetworks};

pub(crate) const TEST_SCALE: MotionScale = MotionScale::from_ranges(8_192, 64);

pub(crate) const BREATH: LightState = LightState::Breath(Breath {
    period_ms: 3_000,
    hue_period_ms: 60_000,
    hue_span: 255,
    min_brightness: 24,
    max_brightness: 80,
    saturation: 200,
    hue: 0,
    group: [16, 32, 3, 0, 0, 0, 0],
    group_len: 3,
});

pub(crate) fn op(event: InputEvent) -> InputIntent {
    InputIntent { source: 0, event }
}

pub(crate) fn tap() -> InputIntent {
    op(InputEvent::Gesture(GestureEvent::Tap {
        id: 0,
        x: 1,
        y: 2,
        held_ms: 0,
        at_ms: 0,
    }))
}

pub(crate) fn triple_click() -> InputIntent {
    op(InputEvent::Button(ButtonEvent::TripleClick))
}

pub(crate) fn long_press() -> InputIntent {
    op(InputEvent::Gesture(GestureEvent::LongPress {
        id: 0,
        x: 3,
        y: 4,
        held_ms: 0,
    }))
}

pub(crate) fn swipe() -> InputIntent {
    op(InputEvent::Gesture(GestureEvent::Swipe {
        id: 0,
        x: 80,
        y: 30,
        end_x: 80,
        end_y: 60,
        direction: SwipeDirection::Down,
        held_ms: 42,
        distance_px: 73,
    }))
}

pub(crate) fn touch_event(status: TouchStatus) -> InputIntent {
    op(InputEvent::Touch(TouchEvent {
        points: [TouchPoint {
            id: 0,
            x: 1,
            y: 2,
            status,
        }; MAX_TOUCH_POINTS],
        len: 1,
    }))
}

pub(crate) fn press(id: u8, x: u16, y: u16) -> InputIntent {
    op(InputEvent::Gesture(GestureEvent::Press { id, x, y }))
}

pub(crate) fn feed_input(manager: &mut DeviceManager, operation: InputIntent) {
    manager.apply_intent(Intent::Input(operation));
}

pub(crate) fn landed(mut manager: DeviceManager) -> DeviceManager {
    manager.apply_business(BusinessIntent::Navigation(NavigationIntent::SplashOver));
    assert_ne!(manager.state().page(), Page::Splash);
    manager
}
