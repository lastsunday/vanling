//! Touch ids across index shuffles, and a release that has to be confirmed.

use super::*;

#[test]
fn tracked_points_upsert_move_and_stay_quiet() {
    let mut c = TouchContinuity::new();
    let ev = c.update(&[(100, 200)]).unwrap();
    assert_eq!(ev.len, 1);
    assert_eq!(
        ev.points[0],
        TouchPoint {
            id: 0,
            x: 100,
            y: 200,
            status: TouchStatus::Down,
        }
    );
    let ev = c.update(&[(120, 210)]).unwrap();
    assert_eq!(
        ev.points[0],
        TouchPoint {
            id: 0,
            x: 120,
            y: 210,
            status: TouchStatus::Contact,
        }
    );
    assert!(c.update(&[(120, 210)]).is_none());
}

#[test]
fn release_confirms_only_after_a_dropout_run_and_recycles_ids() {
    let mut c = TouchContinuity::new();
    c.update(&[(100, 200)]);
    assert!(c.update(&[]).is_none(), "one dropout is not a lift");
    assert!(c.update(&[]).is_none());
    let ev = c.update(&[(101, 201)]).unwrap();
    assert_eq!(ev.points[0].status, TouchStatus::Contact);
    assert_eq!(ev.points[0].id, 0);
    let mut c = TouchContinuity::new();
    c.update(&[(100, 200)]);
    for _ in 0..RELEASE_CONFIRM_SAMPLES - 1 {
        assert!(c.update(&[]).is_none());
    }
    let ev = c.update(&[]).unwrap();
    assert_eq!(
        ev.points[0],
        TouchPoint {
            id: 0,
            x: 100,
            y: 200,
            status: TouchStatus::Release,
        }
    );
    assert!(c.update(&[]).is_none());
    let mut c = TouchContinuity::new();
    c.update(&[(5, 5)]);
    for _ in 0..RELEASE_CONFIRM_SAMPLES {
        c.update(&[]);
    }
    let ev = c.update(&[(7, 7)]).unwrap();
    assert_eq!(ev.points[0].status, TouchStatus::Down);
}

#[test]
fn ids_follow_fingers_across_index_shuffles() {
    let mut c = TouchContinuity::new();
    let first = c.update(&[(10, 10), (300, 300)]).unwrap();
    assert_eq!(first.points[0].id, 0);
    assert_eq!(first.points[1].id, 1);
    let ev = c.update(&[(300, 301), (11, 12)]).unwrap();
    assert_eq!(ev.points[0].id, 1, "far point keeps its id at index 0");
    assert_eq!(ev.points[0].status, TouchStatus::Contact);
    assert_eq!(ev.points[1].id, 0, "near point keeps its id at index 1");
    assert_eq!(ev.points[1].status, TouchStatus::Contact);
}

#[test]
fn multi_contact_samples_keep_releases() {
    let mut c = TouchContinuity::new();
    c.update(&[(5, 5), (6, 6)]);
    for _ in 0..RELEASE_CONFIRM_SAMPLES - 1 {
        assert!(c.update(&[]).is_none());
    }
    let ev = c.update(&[]).unwrap();
    assert_eq!(ev.len, 2);
    assert!(
        ev.points[..ev.len as usize]
            .iter()
            .all(|p| p.status == TouchStatus::Release)
    );
    let mut c = TouchContinuity::new();
    let ev = c.update(&[(5, 5)]).unwrap();
    assert_eq!(ev.len, 1);
}

#[test]
fn touch_map_identity_swap_and_mirror() {
    assert_eq!(TouchMap::IDENTITY.map((10, 20), 240, 320), (10, 20));
    let swap = TouchMap {
        swap_xy: true,
        mirror_x: false,
        mirror_y: false,
    };
    assert_eq!(swap.map((10, 20), 240, 320), (20, 10));
    let mirror = TouchMap {
        swap_xy: false,
        mirror_x: true,
        mirror_y: true,
    };
    assert_eq!(mirror.map((0, 0), 240, 320), (239, 319));
    let both = TouchMap {
        swap_xy: true,
        mirror_x: true,
        mirror_y: true,
    };
    assert_eq!(both.map((10, 20), 240, 320), (219, 309));
}

#[test]
fn tracker_and_gesture_constants_align_on_the_touch_grid() {
    const _: () = assert!(TOUCH_SCAN_MS.is_multiple_of(INPUT_BASE_MS));
    const _: () = assert!(DOUBLE_CLICK_WINDOW_MS.is_multiple_of(TOUCH_SCAN_MS));
    const _: () = assert!(LONG_PRESS_MS.is_multiple_of(TOUCH_SCAN_MS));
    const _: () = assert!(LONG_PRESS_MS > DOUBLE_CLICK_WINDOW_MS);
    assert_eq!(RELEASE_CONFIRM_SAMPLES, 3);
    const _: () = assert!(RELEASE_CONFIRM_SAMPLES as u64 * TOUCH_SCAN_MS == 30);
    const _: () =
        assert!((RELEASE_CONFIRM_SAMPLES as u64 * TOUCH_SCAN_MS) < DOUBLE_CLICK_WINDOW_MS);
    const _: () = assert!(MAX_TOUCH_POINTS == 16);
    assert!(ft6x06_point_is_live(0x00), "down edge");
    assert!(ft6x06_point_is_live(0x0F), "down with X high bits set");
    assert!(ft6x06_point_is_live(0x80), "contact with X=0");
    assert!(ft6x06_point_is_live(0x8F), "contact frame of a held finger");
    assert!(!ft6x06_point_is_live(0x40), "up/lift frame");
    assert!(!ft6x06_point_is_live(0x4F), "up with X high bits set");
    assert!(!ft6x06_point_is_live(0xC0), "reserved event");
    assert!(
        !ft6x06_point_is_live(0xFF),
        "reserved event with X high bits set"
    );
}
