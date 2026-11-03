use super::*;

#[test]
fn snapshots_store_and_replace_the_last_frame() {
    let mut manager = DeviceManager::new();
    let first = TouchEvent {
        points: [TouchPoint {
            id: 0,
            x: 1,
            y: 2,
            status: TouchStatus::Down,
        }; MAX_TOUCH_POINTS],
        len: 1,
    };
    manager.apply_input(op(InputEvent::Touch(first)));
    assert_eq!(manager.touch_state(), Some(first));
    assert_eq!(
        manager.state().touch.view.press_count,
        0,
        "a raw frame is a snapshot, not the classifier press pulse"
    );
    assert_eq!(
        manager.light_state(),
        LightState::default(),
        "an operation snapshot never moves a light"
    );
    let second = TouchEvent {
        points: [TouchPoint {
            id: 1,
            x: 3,
            y: 4,
            status: TouchStatus::Contact,
        }; MAX_TOUCH_POINTS],
        len: 1,
    };
    manager.apply_input(op(InputEvent::Touch(second)));
    assert_eq!(
        manager.touch_state(),
        Some(second),
        "the latest frame replaces the previous snapshot"
    );
}

#[test]
fn heartbeat_counts_only_applied_frames() {
    let mut manager = DeviceManager::new();
    assert_eq!(manager.state().touch.frames, 0);
    manager.apply_input(op(InputEvent::Gesture(GestureEvent::Press {
        id: 0,
        x: 10,
        y: 20,
    })));
    assert_eq!(
        manager.state().touch.frames,
        0,
        "a classifier press pulse is not a raw frame"
    );
    manager.apply_input(touch_event(TouchStatus::Down));
    manager.apply_input(touch_event(TouchStatus::Contact));
    assert_eq!(
        manager.state().touch.frames,
        2,
        "every stored snapshot ticks the heartbeat once"
    );
    manager.apply_input(tap());
    assert_eq!(
        manager.state().touch.frames,
        2,
        "resolved gestures never tick the raw-frame heartbeat"
    );
}
