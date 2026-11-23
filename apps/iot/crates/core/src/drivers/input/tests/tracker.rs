//! Touch ids across index shuffles, a release that has to be confirmed, and the controller's
//! own slot number when it reports one.

use super::*;

fn p(x: u16, y: u16) -> RawPoint {
    RawPoint::new(x, y, RawPoint::NO_ID)
}

fn tagged(x: u16, y: u16, hw_id: u8) -> RawPoint {
    RawPoint::new(x, y, hw_id)
}

#[test]
fn tracked_points_upsert_move_and_stay_quiet() {
    let mut c = TouchContinuity::new();
    let ev = c.update(&[p(100, 200)]).unwrap();
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
    let ev = c.update(&[p(120, 210)]).unwrap();
    assert_eq!(
        ev.points[0],
        TouchPoint {
            id: 0,
            x: 120,
            y: 210,
            status: TouchStatus::Contact,
        }
    );
    assert!(c.update(&[p(120, 210)]).is_none());
}

#[test]
fn release_confirms_only_after_a_dropout_run_and_recycles_ids() {
    let mut c = TouchContinuity::new();
    c.update(&[p(100, 200)]);
    assert!(c.update(&[]).is_none(), "one dropout is not a lift");
    assert!(c.update(&[]).is_none());
    let ev = c.update(&[p(101, 201)]).unwrap();
    assert_eq!(ev.points[0].status, TouchStatus::Contact);
    assert_eq!(ev.points[0].id, 0);
    let mut c = TouchContinuity::new();
    c.update(&[p(100, 200)]);
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
    c.update(&[p(5, 5)]);
    for _ in 0..RELEASE_CONFIRM_SAMPLES {
        c.update(&[]);
    }
    let ev = c.update(&[p(7, 7)]).unwrap();
    assert_eq!(ev.points[0].status, TouchStatus::Down);
}

#[test]
fn ids_follow_fingers_across_index_shuffles() {
    let mut c = TouchContinuity::new();
    let first = c.update(&[p(10, 10), p(300, 300)]).unwrap();
    assert_eq!(first.points[0].id, 0);
    assert_eq!(first.points[1].id, 1);
    let ev = c.update(&[p(300, 301), p(11, 12)]).unwrap();
    assert_eq!(ev.points[0].id, 1, "far point keeps its id at index 0");
    assert_eq!(ev.points[0].status, TouchStatus::Contact);
    assert_eq!(ev.points[1].id, 0, "near point keeps its id at index 1");
    assert_eq!(ev.points[1].status, TouchStatus::Contact);
}

/// A second finger landing near a tracked one is reported first by this panel; the finger
/// that stayed put must keep its id, and the newcomer gets a fresh one.
#[test]
fn a_second_finger_reported_first_does_not_steal_the_tracker() {
    let mut c = TouchContinuity::new();
    let first = c.update(&[tagged(100, 200, 0)]).unwrap();
    assert_eq!(first.points[0].id, 0);
    let ev = c
        .update(&[tagged(140, 200, 1), tagged(100, 200, 0)])
        .unwrap();
    assert_eq!(
        (ev.points[0].id, ev.points[0].status),
        (1, TouchStatus::Down),
        "the newcomer is a new contact at its own position"
    );
    assert_eq!(
        (ev.points[1].id, ev.points[1].x, ev.points[1].status),
        (0, 100, TouchStatus::Contact),
        "the tracked finger keeps its id where it stayed"
    );
}

/// The panel swaps its two points between samples; nearest matching keeps both ids.
#[test]
fn swapped_report_order_keeps_both_fingers() {
    let mut c = TouchContinuity::new();
    let first = c
        .update(&[tagged(40, 100, 0), tagged(280, 100, 1)])
        .unwrap();
    assert_eq!((first.points[0].id, first.points[1].id), (0, 1));
    let ev = c
        .update(&[tagged(281, 110, 0), tagged(41, 110, 1)])
        .unwrap();
    assert_eq!((ev.points[0].id, ev.points[0].x), (1, 281));
    assert_eq!((ev.points[1].id, ev.points[1].x), (0, 41));
    assert!(
        ev.points[..2]
            .iter()
            .all(|p| p.status == TouchStatus::Contact)
    );
}

#[test]
fn points_without_a_usable_slot_pair_by_distance() {
    let mut c = TouchContinuity::new();
    c.update(&[p(40, 100), p(280, 100)]);
    let ev = c.update(&[p(281, 110), p(41, 110)]).unwrap();
    assert_eq!((ev.points[0].id, ev.points[0].x), (1, 281));
    assert_eq!((ev.points[1].id, ev.points[1].x), (0, 41));
}

#[test]
fn both_fingers_slide_ten_samples_without_a_release() {
    let mut c = TouchContinuity::new();
    c.update(&[p(40, 100), p(280, 100)]);
    for step in 1..=10u16 {
        let ev = c.update(&[p(40, 100 + step), p(280, 100 + step)]).unwrap();
        assert_eq!(ev.len, 2);
        assert!(
            ev.points[..2]
                .iter()
                .all(|p| p.status == TouchStatus::Contact)
        );
    }
}

/// Two trackers the same distance away: only then does the controller's slot decide.
#[test]
fn an_exact_distance_tie_falls_to_the_controllers_slot() {
    let mut c = TouchContinuity::new();
    c.update(&[tagged(100, 100, 0), tagged(200, 100, 1)]);
    let ev = c.update(&[tagged(150, 100, 1)]).unwrap();
    assert_eq!((ev.points[0].id, ev.points[0].x), (1, 150));
}

#[test]
fn a_slot_past_the_reach_gate_does_not_drag_the_old_contact() {
    let mut c = TouchContinuity::new();
    c.update(&[tagged(20, 20, 0)]);
    let ev = c.update(&[tagged(300, 300, 0)]).unwrap();
    assert_eq!(
        (ev.points[0].id, ev.points[0].status),
        (1, TouchStatus::Down)
    );
    let ev = c.update(&[tagged(30, 30, 0)]).unwrap();
    assert_eq!((ev.points[0].id, ev.points[0].x), (0, 30));
}

#[test]
fn multi_contact_samples_keep_releases() {
    let mut c = TouchContinuity::new();
    c.update(&[p(5, 5), p(6, 6)]);
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
    let ev = c.update(&[p(5, 5)]).unwrap();
    assert_eq!(ev.len, 1);
}

/// A point that reappears after a missed sample is judged by a tighter reach than one that never
/// left: with the full flick reach a fresh tap near the last position inherits the old contact,
/// arrives as `Contact`, and no press is ever reported for it.
#[test]
fn a_point_that_reappears_after_a_dropout_does_not_inherit_a_far_contact() {
    let mut c = TouchContinuity::new();
    c.update(&[p(40, 40)]);
    assert!(c.update(&[]).is_none(), "one dropout is not a lift");
    let ev = c.update(&[p(40, 100)]).unwrap();
    assert_eq!(
        (ev.points[0].id, ev.points[0].status),
        (1, TouchStatus::Down),
        "60 px after a missed sample is a new contact, not the old one"
    );

    let mut c = TouchContinuity::new();
    c.update(&[p(40, 40)]);
    let ev = c.update(&[p(40, 100)]).unwrap();
    assert_eq!(
        (ev.points[0].id, ev.points[0].status),
        (0, TouchStatus::Contact),
        "60 px between consecutive samples is still one fast flick"
    );
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
