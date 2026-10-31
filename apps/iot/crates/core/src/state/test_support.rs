pub(crate) use super::*;
pub(crate) use crate::drivers::input::{
    ButtonEvent, GestureEvent, InputEvent, MAX_TOUCH_POINTS, SwipeDirection, TouchEvent,
    TouchPoint, TouchStatus,
};
pub(crate) use crate::drivers::motion::MotionScale;
pub(crate) use crate::drivers::network::{Ipv4, SavedNetworks};
pub(crate) use crate::intent::{
    BusinessIntent, LightIntent, OperationIntent, PageIntent, translate,
};
pub(crate) use crate::page::CYCLE;

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

pub(crate) fn op(event: InputEvent) -> OperationIntent {
    OperationIntent { source: 0, event }
}

pub(crate) fn tap() -> OperationIntent {
    op(InputEvent::Gesture(GestureEvent::Tap {
        id: 0,
        x: 1,
        y: 2,
        held_ms: 0,
        at_ms: 0,
    }))
}

pub(crate) fn triple_click() -> OperationIntent {
    op(InputEvent::Button(ButtonEvent::TripleClick))
}

pub(crate) fn long_press() -> OperationIntent {
    op(InputEvent::Gesture(GestureEvent::LongPress {
        id: 0,
        x: 3,
        y: 4,
        held_ms: 0,
    }))
}

pub(crate) fn swipe() -> OperationIntent {
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

pub(crate) fn touch_event(status: TouchStatus) -> OperationIntent {
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

pub(crate) fn set_light(instance: u8, state: LightState) -> BusinessIntent {
    BusinessIntent::Light(LightIntent { instance, state })
}

pub(crate) fn press(id: u8, x: u16, y: u16) -> OperationIntent {
    op(InputEvent::Gesture(GestureEvent::Press { id, x, y }))
}

pub(crate) fn apply_page(manager: &mut DeviceManager, operation: OperationIntent) {
    manager.apply_operation(operation);
    let business = translate(&operation, &manager.state());
    manager.apply_business(business);
}

pub(crate) fn landed(mut manager: DeviceManager) -> DeviceManager {
    manager.apply_business(BusinessIntent::Page(PageIntent::SplashOver));
    assert_ne!(manager.state().page, DisplayPage::Splash);
    manager
}
