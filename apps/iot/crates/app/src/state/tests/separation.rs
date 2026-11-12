use super::*;

#[test]
fn input_alone_never_moves_the_light() {
    let mut manager = DeviceManager::new();
    manager.apply_input(tap());
    manager.apply_input(op(InputEvent::Gesture(GestureEvent::Ghost)));
    manager.apply_input(press(0, 12, 34));
    manager.apply_input(touch_event(TouchStatus::Contact));
    manager.apply_input(touch_event(TouchStatus::Release));
    assert_eq!(manager.light_state(), LightState::default());
    assert_eq!(manager.state().touch.view.tap_count, 1);
    assert_eq!(manager.state().touch.view.ghost_count, 1);
    assert_eq!(
        manager.state().touch.view.press_count,
        1,
        "only a classifier press pulse earns a press tally, never a raw frame"
    );
    let target = LightState::Solid {
        color: Rgb(1, 2, 3),
        brightness: LightState::SOLID_DEFAULT_BRIGHTNESS,
    };
    manager.apply_to(0, target);
    assert_eq!(manager.light_state(), target);
    manager.apply_business(BusinessIntent::Invalid);
    assert_eq!(
        manager.light_state(),
        target,
        "an invalid intent is a no-op"
    );
    assert_eq!(
        manager.state().touch.view.tap_count,
        1,
        "the business move never inflates the operation tally"
    );
}

#[test]
fn off_tap_full_path_records_diagnostics_and_keeps_off() {
    let mut manager = DeviceManager::new();
    manager.apply_to(0, LightState::Off);
    let op = tap();
    manager.apply_input(op);
    let business = translate(&op, &manager.state());
    manager.apply_business(business);
    assert_eq!(business, BusinessIntent::Invalid);
    assert_eq!(manager.light_state(), LightState::Off);
    assert_eq!(manager.state().touch.view.tap_count, 1);
    assert_eq!(manager.state().touch.view.last_gesture_origin, Some((1, 2)));
}

#[test]
fn off_swipe_full_path_records_diagnostics_and_keeps_off() {
    let mut manager = DeviceManager::new();
    manager.apply_to(0, LightState::Off);
    let op = swipe();
    manager.apply_input(op);
    let business = translate(&op, &manager.state());
    manager.apply_business(business);
    assert_eq!(business, BusinessIntent::Invalid);
    assert_eq!(manager.light_state(), LightState::Off);
    assert_eq!(manager.state().touch.view.swipe_count, 1);
    assert_eq!(
        manager.state().touch.view.last_swipe,
        Some((SwipeDirection::Down, 73))
    );
}
