//! Samples to taps, holds and slides, and the phantom echoes a panel can add.

use super::*;

fn sample(points: &[(TouchStatus, u8, u16, u16)]) -> InputEvent {
    let mut pts = [TouchPoint {
        status: TouchStatus::Release,
        id: 0,
        x: 0,
        y: 0,
    }; MAX_TOUCH_POINTS];
    for (i, &(status, id, x, y)) in points.iter().take(MAX_TOUCH_POINTS).enumerate() {
        pts[i] = TouchPoint { status, id, x, y };
    }
    InputEvent::Touch(TouchEvent {
        points: pts,
        len: points.len().min(MAX_TOUCH_POINTS) as u8,
    })
}

fn touch(status: TouchStatus, x: u16, y: u16) -> InputEvent {
    sample(&[(status, 0, x, y)])
}

#[test]
fn press_fires_once_on_the_rise_edge() {
    let mut agg = TouchGestures::new();
    assert_eq!(
        agg.feed(touch(TouchStatus::Down, 10, 10), 0),
        Some(InputEvent::Gesture(GestureEvent::Press {
            id: 0,
            x: 10,
            y: 10
        }))
    );
    assert_eq!(
        agg.feed(touch(TouchStatus::Contact, 10, 10), 5),
        Some(touch(TouchStatus::Contact, 10, 10))
    );
    assert_eq!(
        agg.feed(touch(TouchStatus::Contact, 10, 10), 10),
        Some(touch(TouchStatus::Contact, 10, 10))
    );
    assert_eq!(
        agg.feed(touch(TouchStatus::Release, 10, 10), 15),
        Some(InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x: 10,
            y: 10,
            held_ms: 15,
            at_ms: 15,
        })),
        "the lift resolves the tap on its own tick"
    );
    let mut agg = TouchGestures::new();
    assert_eq!(
        agg.feed(touch(TouchStatus::Contact, 10, 10), 5),
        Some(InputEvent::Gesture(GestureEvent::Press {
            id: 0,
            x: 10,
            y: 10
        }))
    );
    assert_eq!(
        agg.feed(touch(TouchStatus::Release, 10, 10), 10),
        Some(InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x: 10,
            y: 10,
            held_ms: 5,
            at_ms: 10,
        }))
    );
    let mut agg = TouchGestures::new();
    assert_eq!(
        agg.feed(touch(TouchStatus::Release, 10, 10), 10),
        Some(touch(TouchStatus::Release, 10, 10)),
        "an unpaired lift is forwarded as a bare snapshot"
    );
    assert!(
        agg.tick(400).is_none(),
        "an unpaired lift is not a tap, then or later"
    );
}

#[test]
fn a_tap_reports_its_down_edge_on_the_tick_it_lifts() {
    let mut agg = TouchGestures::new();
    assert_eq!(
        agg.feed(touch(TouchStatus::Down, 100, 100), 0),
        Some(InputEvent::Gesture(GestureEvent::Press {
            id: 0,
            x: 100,
            y: 100
        }))
    );
    // The release drifts 20 px, which is the stale coordinate the slide measure
    // refuses to trust: the tap is where the finger landed, not where it left.
    assert_eq!(
        agg.feed(touch(TouchStatus::Release, 120, 105), 10),
        Some(InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x: 100,
            y: 100,
            held_ms: 10,
            at_ms: 10,
        })),
        "no window stands between the lift and the tap"
    );
}

#[test]
fn hold_gap_monotonic_to_the_long_press_threshold() {
    for gap in [300, 400, 500, 599] {
        let mut agg = TouchGestures::new();
        assert!(
            agg.feed(touch(TouchStatus::Down, 100, 100), 0).is_some(),
            "gap {gap}: down edge pulses Press"
        );
        assert_eq!(
            agg.feed(touch(TouchStatus::Release, 100, 100), gap),
            Some(InputEvent::Gesture(GestureEvent::Tap {
                id: 0,
                x: 100,
                y: 100,
                held_ms: gap as u16,
                at_ms: gap as u32,
            })),
            "gap {gap}: a short lift taps at the lift"
        );
    }
    for gap in [600, 610, 800] {
        let mut agg = TouchGestures::new();
        assert!(agg.feed(touch(TouchStatus::Down, 100, 100), 0).is_some());
        assert_eq!(
            agg.feed(touch(TouchStatus::Release, 100, 100), gap),
            Some(InputEvent::Gesture(GestureEvent::LongPress {
                id: 0,
                x: 100,
                y: 100,
                held_ms: gap as u16
            })),
            "gap {gap}: a hold to the threshold is a long-press on release"
        );
    }
}

#[test]
fn any_press_that_ever_traveled_the_slide_threshold_is_a_swipe() {
    let mut agg = TouchGestures::new();
    assert!(
        agg.feed(touch(TouchStatus::Down, 20, 120), 0).is_some(),
        "down edge pulses Press"
    );
    assert_eq!(
        agg.feed(touch(TouchStatus::Release, 140, 125), 90),
        Some(InputEvent::Gesture(GestureEvent::Swipe {
            id: 0,
            direction: SwipeDirection::Right,
            x: 20,
            y: 120,
            end_x: 140,
            end_y: 125,
            held_ms: 90,
            distance_px: 120,
        }))
    );
    assert!(agg.tick(400).is_none(), "no tap is buffered after a swipe");
    let mut agg = TouchGestures::new();
    assert!(agg.feed(touch(TouchStatus::Down, 10, 10), 0).is_some());
    assert_eq!(
        agg.feed(touch(TouchStatus::Release, 110, 10), LONG_PRESS_MS + 1),
        Some(InputEvent::Gesture(GestureEvent::Swipe {
            id: 0,
            direction: SwipeDirection::Right,
            x: 10,
            y: 10,
            end_x: 110,
            end_y: 10,
            held_ms: LONG_PRESS_MS as u16 + 1,
            distance_px: 100,
        }))
    );
    let mut agg = TouchGestures::new();
    assert!(agg.feed(touch(TouchStatus::Down, 20, 120), 0).is_some());
    let mid = agg.feed(touch(TouchStatus::Contact, 140, 125), 400);
    assert!(
        !matches!(mid, Some(InputEvent::Gesture(_))),
        "a mid-hold position is a silent Contact: {mid:?}"
    );
    assert_eq!(
        agg.feed(touch(TouchStatus::Release, 20, 120), LONG_PRESS_MS + 100),
        Some(InputEvent::Gesture(GestureEvent::Swipe {
            id: 0,
            direction: SwipeDirection::Right,
            x: 20,
            y: 120,
            end_x: 140,
            end_y: 125,
            held_ms: LONG_PRESS_MS as u16 + 100,
            distance_px: 120,
        }))
    );
}

#[test]
fn swipe_classify_reads_axes_bands_and_none_below_threshold() {
    for (x0, y0, x1, y1, direction, px) in [
        (10, 100, 90, 100, SwipeDirection::Right, 80),
        (90, 100, 10, 100, SwipeDirection::Left, 80),
        (100, 200, 100, 120, SwipeDirection::Up, 80),
        (100, 120, 100, 200, SwipeDirection::Down, 80),
    ] {
        assert_eq!(swipe_classify(x0, y0, x1, y1), Some((direction, px)));
    }
    for (x0, y0, x1, y1, direction) in [
        (0, 0, 60, 60, SwipeDirection::DownRight),
        (0, 60, 60, 0, SwipeDirection::UpRight),
        (60, 0, 0, 60, SwipeDirection::DownLeft),
        (60, 60, 0, 0, SwipeDirection::UpLeft),
    ] {
        assert_eq!(swipe_classify(x0, y0, x1, y1), Some((direction, 84)));
    }
    let (direction, _) = swipe_classify(0, 0, 20, 60).unwrap();
    assert_eq!(direction, SwipeDirection::Down);
    let (direction, distance) = swipe_classify(10, 10, 80, 80).unwrap();
    assert_eq!(direction, SwipeDirection::DownRight);
    assert_eq!(distance, 98, "sqrt(70²+70²) floors to 98");
    assert_eq!(swipe_classify(0, 0, 50, 0), None);
    assert_eq!(swipe_classify(0, 0, 0, 59), None);
}

#[test]
fn swipe_threshold_straddles_slop_monotonically() {
    // On the 240-px-wide panel a quarter is 60 px: the threshold never
    // exceeds it so a deliberate flick always crosses.
    const QUARTER_PANEL: u16 = 240 / 4;
    const _: () = assert!(SWIPE_MIN_DISTANCE_PX <= QUARTER_PANEL);
    for (label, (x0, y0), (x_below, y_below), (x_at, y_at), (x_far, y_far), direction) in [
        (
            "right",
            (0, 100),
            (59, 100),
            (60, 100),
            (80, 100),
            SwipeDirection::Right,
        ),
        (
            "left",
            (80, 100),
            (21, 100),
            (20, 100),
            (0, 100),
            SwipeDirection::Left,
        ),
        (
            "down",
            (100, 0),
            (100, 59),
            (100, 60),
            (100, 80),
            SwipeDirection::Down,
        ),
        (
            "up",
            (100, 80),
            (100, 21),
            (100, 20),
            (100, 0),
            SwipeDirection::Up,
        ),
        (
            "down-right",
            (0, 0),
            (50, 32),
            (50, 34),
            (80, 80),
            SwipeDirection::DownRight,
        ),
        (
            "up-right",
            (0, 80),
            (50, 48),
            (50, 46),
            (80, 0),
            SwipeDirection::UpRight,
        ),
        (
            "down-left",
            (80, 0),
            (30, 32),
            (30, 34),
            (0, 80),
            SwipeDirection::DownLeft,
        ),
        (
            "up-left",
            (80, 80),
            (30, 48),
            (30, 46),
            (0, 0),
            SwipeDirection::UpLeft,
        ),
    ] {
        assert_eq!(
            swipe_classify(x0, y0, x_below, y_below),
            None,
            "{label}: 59 px of travel is still a tap"
        );
        let (dir, px) =
            swipe_classify(x0, y0, x_at, y_at).expect("{label}: 60 px crosses the slop threshold");
        assert_eq!(dir, direction);
        assert_eq!(px, 60);
        let (dir, px) = swipe_classify(x0, y0, x_far, y_far).unwrap();
        assert_eq!(dir, direction, "{label}: the far slide keeps its direction");
        assert!(px > 60, "{label}: travel keeps growing past the threshold");
    }
}

#[test]
fn direction_between_reads_axes_and_respects_the_deadband() {
    assert_eq!(
        direction_between(100, 200, 100, 120, MOVE_DEADBAND_PX),
        Some(SwipeDirection::Up)
    );
    assert_eq!(
        direction_between(100, 120, 100, 200, MOVE_DEADBAND_PX),
        Some(SwipeDirection::Down)
    );
    assert_eq!(
        direction_between(120, 100, 30, 100, MOVE_DEADBAND_PX),
        Some(SwipeDirection::Left)
    );
    assert_eq!(
        direction_between(30, 100, 90, 100, MOVE_DEADBAND_PX),
        Some(SwipeDirection::Right)
    );
    assert_eq!(
        direction_between(100, 100, 50, 150, MOVE_DEADBAND_PX),
        Some(SwipeDirection::DownLeft)
    );
    assert_eq!(
        direction_between(100, 150, 160, 60, MOVE_DEADBAND_PX),
        Some(SwipeDirection::UpRight)
    );
    assert_eq!(
        direction_between(10, 10, 80, 80, MOVE_DEADBAND_PX),
        Some(SwipeDirection::DownRight)
    );
    assert_eq!(direction_between(10, 10, 11, 10, MOVE_DEADBAND_PX), None);
    assert_eq!(direction_between(10, 10, 10, 11, MOVE_DEADBAND_PX), None);
    assert_eq!(direction_between(10, 10, 11, 11, MOVE_DEADBAND_PX), None);
}

#[test]
fn two_quick_lifts_are_two_taps() {
    let mut agg = TouchGestures::new();
    assert_eq!(
        agg.feed(sample(&[(TouchStatus::Down, 12, 40, 60)]), 0),
        Some(InputEvent::Gesture(GestureEvent::Press {
            id: 12,
            x: 40,
            y: 60
        }))
    );
    assert_eq!(
        agg.feed(sample(&[(TouchStatus::Release, 12, 40, 60)]), 10),
        Some(InputEvent::Gesture(GestureEvent::Tap {
            id: 12,
            x: 40,
            y: 60,
            held_ms: 10,
            at_ms: 10,
        })),
        "the first lift reports its own tap, not a pending first half"
    );
    assert_eq!(
        agg.feed(sample(&[(TouchStatus::Down, 7, 45, 65)]), 90),
        Some(InputEvent::Gesture(GestureEvent::Press {
            id: 7,
            x: 45,
            y: 65
        })),
        "the second physical tap is a fresh tracker, never the old id"
    );
    assert_eq!(
        agg.feed(sample(&[(TouchStatus::Release, 7, 45, 65)]), 100),
        Some(InputEvent::Gesture(GestureEvent::Tap {
            id: 7,
            x: 45,
            y: 65,
            held_ms: 10,
            at_ms: 100,
        })),
        "the pair is two taps on two ids, with no window between them"
    );
    assert!(agg.tick(2_000).is_none(), "and nothing lapses later");
}

#[test]
fn separated_lifts_release_as_independent_taps() {
    let mut agg = TouchGestures::new();
    assert!(agg.tick(100).is_none(), "a quiet clock reports nothing");
    assert!(agg.feed(touch(TouchStatus::Down, 10, 10), 0).is_some());
    assert_eq!(
        agg.feed(touch(TouchStatus::Release, 10, 10), 10),
        Some(InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x: 10,
            y: 10,
            held_ms: 10,
            at_ms: 10,
        }))
    );
    assert!(agg.feed(touch(TouchStatus::Down, 20, 20), 400).is_some());
    assert_eq!(
        agg.feed(touch(TouchStatus::Release, 20, 20), 410),
        Some(InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x: 20,
            y: 20,
            held_ms: 10,
            at_ms: 410,
        }))
    );
}

#[test]
fn an_end_gesture_after_a_tap_is_reported_behind_it() {
    let mut agg = TouchGestures::new();
    assert!(agg.feed(touch(TouchStatus::Down, 10, 10), 0).is_some());
    assert_eq!(
        agg.feed(touch(TouchStatus::Release, 10, 10), 10),
        Some(InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x: 10,
            y: 10,
            held_ms: 10,
            at_ms: 10,
        }))
    );
    assert!(agg.feed(touch(TouchStatus::Down, 10, 200), 400).is_some());
    assert_eq!(
        agg.feed(touch(TouchStatus::Release, 10, 100), 480),
        Some(InputEvent::Gesture(GestureEvent::Swipe {
            id: 0,
            direction: SwipeDirection::Up,
            x: 10,
            y: 200,
            end_x: 10,
            end_y: 100,
            held_ms: 80,
            distance_px: 100,
        })),
        "the slide's own sample reports the slide"
    );

    let mut agg = TouchGestures::new();
    assert!(agg.feed(touch(TouchStatus::Down, 10, 10), 0).is_some());
    assert_eq!(
        agg.feed(touch(TouchStatus::Release, 10, 10), 10),
        Some(InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x: 10,
            y: 10,
            held_ms: 10,
            at_ms: 10,
        }))
    );
    assert!(agg.feed(touch(TouchStatus::Down, 20, 20), 400).is_some());
    assert_eq!(
        agg.feed(touch(TouchStatus::Release, 20, 20), 1000),
        Some(InputEvent::Gesture(GestureEvent::LongPress {
            id: 0,
            x: 20,
            y: 20,
            held_ms: 600
        }))
    );
    assert!(agg.tick(1300).is_none());
}

#[test]
fn phantom_echoes_pulse_once_and_leave_the_finger_armed() {
    let mut agg = TouchGestures::new();
    assert_eq!(
        agg.feed(sample(&[(TouchStatus::Down, 0, 100, 100)]), 0),
        Some(InputEvent::Gesture(GestureEvent::Press {
            id: 0,
            x: 100,
            y: 100
        }))
    );
    assert_eq!(
        agg.feed(
            sample(&[
                (TouchStatus::Contact, 0, 100, 100),
                (TouchStatus::Down, 1, 110, 105)
            ]),
            10,
        ),
        Some(InputEvent::Gesture(GestureEvent::Ghost))
    );
    assert_eq!(
        agg.feed(sample(&[(TouchStatus::Release, 1, 110, 105)]), 20),
        Some(sample(&[(TouchStatus::Release, 1, 110, 105)])),
        "a lift with no tracked press is a bare snapshot, not a tap"
    );
    assert_eq!(
        agg.feed(sample(&[(TouchStatus::Release, 0, 100, 100)]), 30),
        Some(InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x: 100,
            y: 100,
            held_ms: 30,
            at_ms: 30,
        })),
        "the real finger's lift is its own tap, unmoved by the echo"
    );
    let mut agg = TouchGestures::new();
    assert!(
        agg.feed(sample(&[(TouchStatus::Down, 0, 30, 30)]), 0)
            .is_some()
    );
    assert_eq!(
        agg.feed(
            sample(&[
                (TouchStatus::Contact, 0, 30, 30),
                (TouchStatus::Down, 1, 35, 35)
            ]),
            5
        ),
        Some(InputEvent::Gesture(GestureEvent::Ghost))
    );
    assert_eq!(
        agg.feed(
            sample(&[
                (TouchStatus::Contact, 0, 30, 30),
                (TouchStatus::Down, 2, 36, 36)
            ]),
            9
        ),
        Some(sample(&[
            (TouchStatus::Contact, 0, 30, 30),
            (TouchStatus::Down, 2, 36, 36)
        ])),
    );
    assert_eq!(
        agg.feed(sample(&[(TouchStatus::Contact, 0, 30, 30)]), 50),
        Some(sample(&[(TouchStatus::Contact, 0, 30, 30)])),
        "a clean one-contact sample closes the run"
    );
    assert_eq!(
        agg.feed(sample(&[(TouchStatus::Down, 3, 200, 200)]), 60),
        Some(InputEvent::Gesture(GestureEvent::Press {
            id: 3,
            x: 200,
            y: 200
        }))
    );
    assert_eq!(
        agg.feed(
            sample(&[
                (TouchStatus::Contact, 3, 200, 200),
                (TouchStatus::Down, 4, 205, 205)
            ]),
            65
        ),
        Some(InputEvent::Gesture(GestureEvent::Ghost)),
        "a genuinely new finger's echo is a fresh run, not a continuation"
    );
}

#[test]
fn concurrent_fingers_never_pair_into_one_gesture() {
    let mut agg = TouchGestures::new();
    assert_eq!(
        agg.feed(sample(&[(TouchStatus::Down, 0, 20, 120)]), 0),
        Some(InputEvent::Gesture(GestureEvent::Press {
            id: 0,
            x: 20,
            y: 120
        }))
    );
    assert_eq!(
        agg.feed(sample(&[(TouchStatus::Down, 1, 150, 200)]), 0),
        Some(InputEvent::Gesture(GestureEvent::Press {
            id: 1,
            x: 150,
            y: 200
        })),
        "a second faraway finger is genuine concurrency, not a phantom"
    );
    assert_eq!(
        agg.feed(sample(&[(TouchStatus::Release, 0, 140, 125)]), 90),
        Some(InputEvent::Gesture(GestureEvent::Swipe {
            id: 0,
            direction: SwipeDirection::Right,
            x: 20,
            y: 120,
            end_x: 140,
            end_y: 125,
            held_ms: 90,
            distance_px: 120,
        }))
    );
    assert_eq!(
        agg.feed(sample(&[(TouchStatus::Contact, 1, 150, 200)]), 95),
        Some(sample(&[(TouchStatus::Contact, 1, 150, 200)])),
        "finger 1's hold is silent; only its snapshot passes through"
    );
    assert_eq!(
        agg.feed(sample(&[(TouchStatus::Release, 1, 150, 200)]), 100),
        Some(InputEvent::Gesture(GestureEvent::Tap {
            id: 1,
            x: 150,
            y: 200,
            held_ms: 100,
            at_ms: 100,
        })),
        "finger 1's tap did not pair with finger 0's swipe"
    );
    assert!(agg.tick(101).is_none());

    let mut agg = TouchGestures::new();
    assert!(
        agg.feed(sample(&[(TouchStatus::Down, 0, 10, 90)]), 0)
            .is_some()
    );
    assert!(
        agg.feed(sample(&[(TouchStatus::Down, 1, 150, 90)]), 0)
            .is_some()
    );
    assert_eq!(
        agg.feed(sample(&[(TouchStatus::Release, 0, 10, 90)]), 10),
        Some(InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x: 10,
            y: 90,
            held_ms: 10,
            at_ms: 10,
        }))
    );
    assert_eq!(
        agg.feed(sample(&[(TouchStatus::Release, 1, 150, 90)]), 20),
        Some(InputEvent::Gesture(GestureEvent::Tap {
            id: 1,
            x: 150,
            y: 90,
            held_ms: 20,
            at_ms: 20,
        })),
        "the second finger's lift taps on its own tick, not the first's window"
    );
    assert!(agg.tick(21).is_none());
}

#[test]
fn same_sample_lifts_buffer_the_second_in_fifo() {
    let mut agg = TouchGestures::new();
    assert!(
        agg.feed(sample(&[(TouchStatus::Down, 0, 20, 120)]), 0)
            .is_some()
    );
    assert!(
        agg.feed(sample(&[(TouchStatus::Down, 1, 150, 200)]), 0)
            .is_some()
    );
    let both_lift = sample(&[
        (TouchStatus::Release, 0, 140, 125),
        (TouchStatus::Release, 1, 80, 170),
    ]);
    assert_eq!(
        agg.feed(both_lift, 90),
        Some(InputEvent::Gesture(GestureEvent::Swipe {
            id: 0,
            direction: SwipeDirection::Right,
            x: 20,
            y: 120,
            end_x: 140,
            end_y: 125,
            held_ms: 90,
            distance_px: 120,
        }))
    );
    assert_eq!(
        agg.tick(100),
        Some(InputEvent::Gesture(GestureEvent::Swipe {
            id: 1,
            direction: SwipeDirection::UpLeft,
            x: 150,
            y: 200,
            end_x: 80,
            end_y: 170,
            held_ms: 90,
            distance_px: 76,
        })),
        "the second event waited in FIFO, not dropped"
    );
    assert!(
        agg.tick(110).is_none(),
        "and nothing else was reported for that frame"
    );
}

#[test]
fn companion_lift_never_cycles_a_clean_hold_still_does() {
    let mut agg = TouchGestures::new();
    assert!(
        agg.feed(sample(&[(TouchStatus::Down, 0, 20, 120)]), 0)
            .is_some()
    );
    assert!(
        agg.feed(sample(&[(TouchStatus::Down, 1, 150, 150)]), 100)
            .is_some(),
        "a far second contact is genuine concurrency, not a phantom"
    );
    let ghost_lift = agg.feed(sample(&[(TouchStatus::Release, 1, 150, 150)]), 900);
    assert!(
        !matches!(
            ghost_lift,
            Some(InputEvent::Gesture(GestureEvent::LongPress { .. }))
        ),
        "the parked companion's 900 ms lift never reads as a long-press: {ghost_lift:?}"
    );
    assert_eq!(
        agg.feed(sample(&[(TouchStatus::Release, 0, 20, 120)]), 1100),
        Some(InputEvent::Gesture(GestureEvent::LongPress {
            id: 0,
            x: 20,
            y: 120,
            held_ms: 1100,
        })),
        "the real finger's clean hold, ended alone, is the deliberate long-press"
    );
}

#[test]
fn isqrt_u32_floors_perfect_and_imperfect_squares() {
    assert_eq!(isqrt_u32(0), 0);
    assert_eq!(isqrt_u32(1), 1);
    assert_eq!(isqrt_u32(4), 2);
    assert_eq!(isqrt_u32(65536), 256);
    assert_eq!(isqrt_u32(9800), 98);
    assert_eq!(isqrt_u32(u32::MAX), 65535);
}

#[test]
fn chip_gestures_pass_through_unmodified() {
    let mut agg = TouchGestures::new();
    assert_eq!(
        agg.feed(InputEvent::ChipGesture(0x10), 0),
        Some(InputEvent::ChipGesture(0x10))
    );
}
