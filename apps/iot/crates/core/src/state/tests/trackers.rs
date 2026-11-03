use super::*;
use crate::drivers::input::{FINGER_SWIPE, FINGER_TAP};

#[test]
fn live_points_follow_down_contact_release() {
    let mut manager = DeviceManager::new();
    manager.apply_input(op(InputEvent::Touch(TouchEvent {
        points: [TouchPoint {
            id: 0,
            x: 10,
            y: 20,
            status: TouchStatus::Down,
        }; MAX_TOUCH_POINTS],
        len: 1,
    })));
    manager.apply_input(op(InputEvent::Touch(TouchEvent {
        points: [TouchPoint {
            id: 1,
            x: 150,
            y: 40,
            status: TouchStatus::Contact,
        }; MAX_TOUCH_POINTS],
        len: 1,
    })));
    assert_eq!(
        manager.state().touch.points,
        [
            Some(LivePoint {
                id: 0,
                x: 10,
                y: 20
            }),
            Some(LivePoint {
                id: 1,
                x: 150,
                y: 40
            })
        ],
        "each tracker upserts into its own slot"
    );
    assert_eq!(
        manager.state().touch.two_finger_runs,
        1,
        "two live slots in one transition counts one run"
    );
    manager.apply_input(op(InputEvent::Touch(TouchEvent {
        points: [TouchPoint {
            id: 0,
            x: 0,
            y: 0,
            status: TouchStatus::Release,
        }; MAX_TOUCH_POINTS],
        len: 1,
    })));
    assert_eq!(
        manager.state().touch.points,
        [
            None,
            Some(LivePoint {
                id: 1,
                x: 150,
                y: 40
            })
        ],
        "a release frees only the tracker's own slot"
    );
    manager.apply_input(op(InputEvent::Touch(TouchEvent {
        points: [TouchPoint {
            id: 1,
            x: 0,
            y: 0,
            status: TouchStatus::Release,
        }; MAX_TOUCH_POINTS],
        len: 1,
    })));
    assert_eq!(manager.state().touch.points, [None, None]);
    assert_eq!(
        manager.state().touch.two_finger_runs,
        1,
        "releasing a finger never grows the run counter"
    );
}

#[test]
fn live_dir_tracks_moves_and_keeps_axis_under_deadband() {
    let mut manager = DeviceManager::new();
    manager.apply_input(op(InputEvent::Gesture(GestureEvent::Press {
        id: 0,
        x: 100,
        y: 200,
    })));
    assert_eq!(
        manager.state().touch.live_dir,
        [0, 0],
        "a fresh landing reads as still until the finger moves"
    );
    manager.apply_input(touch_at(0, 100, 150, TouchStatus::Contact));
    assert_eq!(
        manager.state().touch.live_dir[0],
        SwipeDirection::Up.code(),
        "climbing the panel is an up-move in framebuffer space"
    );
    manager.apply_input(touch_at(0, 60, 150, TouchStatus::Contact));
    assert_eq!(
        manager.state().touch.live_dir[0],
        SwipeDirection::Left.code()
    );
    manager.apply_input(touch_at(0, 61, 151, TouchStatus::Contact));
    assert_eq!(
        manager.state().touch.live_dir[0],
        SwipeDirection::Left.code(),
        "sub-deadband jitter keeps the last axis instead of flickering to 0"
    );
}

#[test]
fn live_dir_reads_each_finger_independently() {
    let mut manager = DeviceManager::new();
    manager.apply_input(touch_at(0, 100, 200, TouchStatus::Down));
    manager.apply_input(touch_at(1, 200, 200, TouchStatus::Down));
    manager.apply_input(touch_at(1, 150, 200, TouchStatus::Contact));
    assert_eq!(
        manager.state().touch.live_dir,
        [0, SwipeDirection::Left.code()],
        "each slot carries only its own finger's axis"
    );
    manager.apply_input(touch_at(1, 0, 0, TouchStatus::Release));
    assert_eq!(
        manager.state().touch.live_dir,
        [0, 0],
        "releasing a finger frees its slot back to still"
    );
}

#[test]
fn simultaneous_presses_upsert_slots_and_count_runs() {
    let mut manager = DeviceManager::new();
    manager.apply_input(press(0, 10, 20));
    assert_eq!(
        manager.state().touch.points,
        [
            Some(LivePoint {
                id: 0,
                x: 10,
                y: 20
            }),
            None
        ],
        "the first press lands in the lowest free slot"
    );
    manager.apply_input(press(1, 50, 60));
    assert_eq!(
        manager.state().touch.two_finger_runs,
        1,
        "two live slots count one two-finger run"
    );
    manager.apply_input(press(0, 11, 21));
    assert_eq!(
        manager.state().touch.two_finger_runs,
        1,
        "an upsert over a live slot is not a new run"
    );
}

#[test]
fn two_finger_runs_saturates_at_255() {
    let mut manager = DeviceManager::new();
    let down = [
        TouchPoint {
            id: 1,
            x: 10,
            y: 10,
            status: TouchStatus::Down,
        },
        TouchPoint {
            id: 2,
            x: 150,
            y: 90,
            status: TouchStatus::Down,
        },
    ];
    let up = [
        TouchPoint {
            id: 1,
            x: 0,
            y: 0,
            status: TouchStatus::Release,
        },
        TouchPoint {
            id: 2,
            x: 0,
            y: 0,
            status: TouchStatus::Release,
        },
    ];
    let mut down_snapshot = [down[0]; MAX_TOUCH_POINTS];
    down_snapshot[1] = down[1];
    let mut up_snapshot = [up[0]; MAX_TOUCH_POINTS];
    up_snapshot[1] = up[1];
    for _ in 0..(u8::MAX as u32).saturating_add(1) {
        manager.apply_input(op(InputEvent::Touch(TouchEvent {
            points: down_snapshot,
            len: 2,
        })));
        manager.apply_input(op(InputEvent::Touch(TouchEvent {
            points: up_snapshot,
            len: 2,
        })));
    }
    assert_eq!(
        manager.state().touch.two_finger_runs,
        u8::MAX,
        "the run counter saturates instead of wrapping, preserving the record"
    );
}

#[test]
fn gestures_stamp_their_own_slot_and_free_the_point() {
    let mut manager = DeviceManager::new();
    manager.apply_input(press(0, 40, 50));
    manager.apply_input(press(1, 120, 30));
    manager.apply_input(op(InputEvent::Gesture(GestureEvent::Swipe {
        id: 0,
        x: 10,
        y: 200,
        end_x: 10,
        end_y: 50,
        direction: SwipeDirection::Up,
        held_ms: 30,
        distance_px: 77,
    })));
    manager.apply_input(op(InputEvent::Gesture(GestureEvent::Tap {
        id: 1,
        x: 120,
        y: 40,
        held_ms: 90,
        at_ms: 90,
    })));
    assert_eq!(
        manager.state().touch.points,
        [None, None],
        "each resolved gesture frees only its own slot"
    );
    assert_eq!(
        manager.state().touch.view.finger,
        [
            FingerLast {
                kind: FINGER_SWIPE,
                value: 77,
            },
            FingerLast {
                kind: FINGER_TAP,
                value: 90,
            }
        ],
        "concurrent fingers record their own last gestures, slot-aligned"
    );
}

#[test]
fn diagonal_swipe_frees_the_live_slot_and_arrow() {
    let mut manager = DeviceManager::new();
    manager.apply_input(touch_at(0, 60, 60, TouchStatus::Down));
    manager.apply_input(touch_at(0, 80, 80, TouchStatus::Contact));
    let light = manager.state().lights[0];
    assert_ne!(
        manager.state().touch.live_dir[0],
        0,
        "the slide mints a diagonal arrow"
    );
    manager.apply_input(op(InputEvent::Gesture(GestureEvent::Swipe {
        id: 0,
        x: 60,
        y: 60,
        end_x: 80,
        end_y: 80,
        direction: SwipeDirection::DownRight,
        held_ms: 40,
        distance_px: 28,
    })));
    let state = manager.state();
    assert_eq!(
        state.lights[0], light,
        "an operation never moves the light; the identity resolves in translate"
    );
    assert_eq!(
        state.touch.view.swipe_count, 1,
        "the diagonal still tallies"
    );
    assert_eq!(
        state.touch.view.last_swipe,
        Some((SwipeDirection::DownRight, 28))
    );
    assert_eq!(
        state.touch.points[0], None,
        "resolution frees the live slot"
    );
    assert_eq!(
        state.touch.live_dir[0], 0,
        "resolution clears the movement arrow"
    );
}

#[test]
fn last_gesture_points_follow_resolved_gestures() {
    let mut manager = DeviceManager::new();
    manager.apply_input(swipe());
    assert_eq!(
        manager.state().touch.view.last_gesture_origin,
        Some((80, 30)),
        "a swipe records its origin"
    );
    assert_eq!(manager.state().touch.view.last_gesture_end, Some((80, 60)));

    manager.apply_input(op(InputEvent::Gesture(GestureEvent::Tap {
        id: 0,
        x: 11,
        y: 12,
        held_ms: 0,
        at_ms: 0,
    })));
    assert_eq!(
        manager.state().touch.view.last_gesture_origin,
        Some((11, 12)),
        "a tap stamps its own point as the origin"
    );
    assert_eq!(
        manager.state().touch.view.last_gesture_end,
        None,
        "a single-point tap clears the previous trailing point"
    );

    manager.apply_input(swipe());
    manager.apply_input(op(InputEvent::Gesture(GestureEvent::LongPress {
        id: 0,
        x: 13,
        y: 14,
        held_ms: 0,
    })));
    assert_eq!(
        manager.state().touch.view.last_gesture_origin,
        Some((13, 14))
    );
    assert_eq!(
        manager.state().touch.view.last_gesture_end,
        None,
        "a single-point long-press clears the previous trailing point"
    );

    manager.apply_input(swipe());
    manager.apply_input(op(InputEvent::Gesture(GestureEvent::Swipe {
        id: 0,
        x: 21,
        y: 22,
        end_x: 23,
        end_y: 24,
        direction: SwipeDirection::DownRight,
        held_ms: 0,
        distance_px: 3,
    })));
    assert_eq!(
        manager.state().touch.view.last_gesture_origin,
        Some((21, 22)),
        "a slide shows where it started"
    );
    assert_eq!(
        manager.state().touch.view.last_gesture_end,
        Some((23, 24)),
        "a slide shows where it ended"
    );
}

fn touch_at(id: u8, x: u16, y: u16, status: TouchStatus) -> InputIntent {
    op(InputEvent::Touch(TouchEvent {
        points: [TouchPoint { id, x, y, status }; MAX_TOUCH_POINTS],
        len: 1,
    }))
}
