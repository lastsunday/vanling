use super::*;

#[test]
fn tap_tallies_stamps_origin_and_accumulates() {
    let mut manager = DeviceManager::new();
    for _ in 0..3 {
        manager.apply_input(tap());
    }
    let state = manager.state();
    assert_eq!(state.touch.view.tap_count, 3);
    assert_eq!(
        state.touch.view.last_gesture_origin,
        Some((1, 2)),
        "the tap records its touch point as the gesture origin"
    );
    assert_eq!(
        manager.light_state(),
        LightState::default(),
        "the tally never moves the light; only the translated action intent does"
    );
}

#[test]
fn long_press_tallies_on_its_own_row() {
    let mut manager = DeviceManager::new();
    manager.apply_input(long_press());
    assert_eq!(manager.state().touch.view.long_press_count, 1);
    assert_eq!(
        manager.state().touch.view.press_count,
        0,
        "the hold's down-edge press and the long-press are separate tallies"
    );
}

#[test]
fn tap_count_wraps_at_255() {
    let mut manager = DeviceManager::new();
    for _ in 0..(u8::MAX as u32).saturating_add(1) {
        manager.apply_input(tap());
    }
    assert_eq!(
        manager.state().touch.view.tap_count,
        0,
        "255 taps wrap to 0, keeping the diagnostic digit bounded"
    );
}

#[test]
fn press_pulse_tallies_upserts_and_wraps() {
    let mut manager = DeviceManager::new();
    manager.apply_input(press(0, 12, 34));
    manager.apply_input(press(0, 12, 34));
    assert_eq!(manager.state().touch.view.press_count, 2);
    assert_eq!(
        manager.state().touch.points,
        [
            Some(LivePoint {
                id: 0,
                x: 12,
                y: 34
            }),
            None
        ]
    );
    let mut wraps = DeviceManager::new();
    for _ in 0..(u8::MAX as u32).saturating_add(1) {
        wraps.apply_input(press(0, 12, 34));
    }
    assert_eq!(
        wraps.state().touch.view.press_count,
        0,
        "255 presses wrap to 0, keeping the diagnostic digit bounded"
    );
}

#[test]
fn ghost_tallies_and_saturates() {
    let mut manager = DeviceManager::new();
    manager.apply_input(op(InputEvent::Gesture(GestureEvent::Ghost)));
    manager.apply_input(op(InputEvent::Gesture(GestureEvent::Ghost)));
    assert_eq!(manager.state().touch.view.ghost_count, 2);
    for _ in 0..(u8::MAX as u32).saturating_add(1) {
        manager.apply_input(op(InputEvent::Gesture(GestureEvent::Ghost)));
    }
    assert_eq!(
        manager.state().touch.view.ghost_count,
        u8::MAX,
        "the ghost tally saturates instead of wrapping, preserving the record"
    );
}

#[test]
fn swipe_tallies_records_travel_and_wraps() {
    let mut manager = DeviceManager::new();
    manager.apply_input(swipe());
    let state = manager.state();
    assert_eq!(state.touch.view.swipe_count, 1);
    assert_eq!(
        state.touch.view.last_swipe,
        Some((SwipeDirection::Down, 73))
    );
    assert_eq!(state.touch.view.last_gesture_origin, Some((80, 30)));
    assert_eq!(state.touch.view.last_gesture_end, Some((80, 60)));
    assert_eq!(state.touch.view.held_ms, 42);
    assert_eq!(
        state.lights[0],
        LightState::default(),
        "the swipe op tallies; the light move is the translated action intent"
    );
    assert_eq!(
        state.touch.points,
        [None, None],
        "resolution frees the live slot"
    );
    assert_eq!(
        state.touch.live_dir[0], 0,
        "resolution clears the movement arrow"
    );
    let mut wraps = DeviceManager::new();
    for _ in 0..(u8::MAX as u32).saturating_add(1) {
        wraps.apply_input(op(InputEvent::Gesture(GestureEvent::Swipe {
            id: 0,
            x: 3,
            y: 9,
            end_x: 9,
            end_y: 3,
            direction: SwipeDirection::Up,
            held_ms: 0,
            distance_px: 60,
        })));
    }
    assert_eq!(
        wraps.state().touch.view.swipe_count,
        0,
        "255 swipes wrap to 0, keeping the diagnostic digit bounded"
    );
}

#[test]
fn chip_gesture_stores_verbatim_without_touching_light() {
    let mut manager = DeviceManager::new();
    manager.apply_input(op(InputEvent::ChipGesture(0x10)));
    assert_eq!(manager.state().touch.view.chip_gesture_id, 0x10);
    assert_eq!(manager.light_state(), LightState::default());
}
