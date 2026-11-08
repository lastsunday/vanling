use super::*;
use iot_core::drivers::motion::{MotionBatch, MotionEvent, MotionEvents, TiltDir};

const ALL_CAPS: MotionCapabilities = MotionCapabilities::TAP
    .union(MotionCapabilities::STEP)
    .union(MotionCapabilities::STILL)
    .union(MotionCapabilities::MOVING)
    .union(MotionCapabilities::ACTIVITY)
    .union(MotionCapabilities::TILT)
    .union(MotionCapabilities::SHAKE)
    .union(MotionCapabilities::LIFT_PLACE)
    .union(MotionCapabilities::POSTURE);

fn sample() -> MotionSample {
    MotionSample::from_raw([1, 2, 3], [4, 5, 6], TEST_SCALE, 3, 0)
}

fn poll(manager: &mut DeviceManager, events: &[MotionEvent]) {
    let mut batch = MotionEvents::new();
    for &event in events {
        batch.push(event);
    }
    manager.apply_input(op(InputEvent::Motion(MotionBatch {
        sample: sample(),
        events: batch,
    })));
}

type Bucket = fn(&MotionCounts) -> u16;

#[test]
fn the_page_switch_skips_pages_the_board_lacks() {
    let mut disabled = landed(DeviceManager::new());
    apply_page(&mut disabled, triple_click());
    assert_eq!(disabled.state().page(), Page::Ambient);

    let mut enabled = landed(DeviceManager::boot(Boot {
        backed: Backed {
            motion: Some(ALL_CAPS),
            ..Backed::NONE
        },
        ..Default::default()
    }));
    apply_page(&mut enabled, triple_click());
    assert_eq!(enabled.state().page(), Page::Attitude);
    apply_page(&mut enabled, triple_click());
    assert_eq!(enabled.state().page(), Page::Ambient);
}

#[test]
fn motion_samples_are_kept_only_when_motion_is_enabled() {
    let mut disabled = DeviceManager::new();
    disabled.apply_input(op(InputEvent::Motion(MotionBatch::new(sample()))));
    assert_eq!(disabled.motion_state(), None);
    assert_eq!(disabled.motion_counts_state(), MotionCounts::default());

    let mut enabled = DeviceManager::boot(Boot {
        backed: Backed {
            motion: Some(ALL_CAPS),
            ..Backed::NONE
        },
        ..Default::default()
    });
    enabled.apply_input(op(InputEvent::Motion(MotionBatch::new(sample()))));
    assert_eq!(enabled.motion_state(), Some(sample()));
    assert_eq!(
        enabled.motion_counts_state(),
        MotionCounts::default(),
        "a silent poll records no semantic"
    );
}

#[test]
fn every_semantic_files_under_its_own_bucket() {
    let cases: [(MotionEvent, Bucket); 14] = [
        (MotionEvent::Tap { count: 1 }, |c| c.taps),
        (MotionEvent::Tap { count: 2 }, |c| c.double_taps),
        (MotionEvent::Tap { count: 3 }, |c| c.triple_taps),
        (MotionEvent::Still, |c| c.still),
        (MotionEvent::Moving, |c| c.moving),
        (MotionEvent::Activity, |c| c.activity),
        (MotionEvent::Step, |c| c.steps),
        (MotionEvent::TiltEnter(TiltDir::Left), |c| c.tilt_enters),
        (MotionEvent::TiltExit(TiltDir::Left), |c| c.tilt_exits),
        (MotionEvent::Shake, |c| c.shakes),
        (MotionEvent::Lift, |c| c.lifts),
        (MotionEvent::Place, |c| c.places),
        (MotionEvent::Portrait, |c| c.portraits),
        (MotionEvent::Landscape, |c| c.landscapes),
    ];
    for (event, bucket) in cases {
        let mut manager = DeviceManager::boot(Boot {
            backed: Backed {
                motion: Some(ALL_CAPS),
                ..Backed::NONE
            },
            ..Default::default()
        });
        poll(&mut manager, &[event]);
        let counts = manager.motion_counts_state();
        assert_eq!(bucket(&counts), 1, "{event:?} counted once");
        let others: u16 = cases
            .iter()
            .filter(|(other, _)| *other != event)
            .map(|(_, bucket)| bucket(&counts))
            .sum();
        assert_eq!(others, 0, "{event:?} touched no other bucket");
    }
}

#[test]
fn one_poll_counts_every_semantic_it_carried() {
    let mut manager = DeviceManager::boot(Boot {
        backed: Backed {
            motion: Some(ALL_CAPS),
            ..Backed::NONE
        },
        ..Default::default()
    });
    poll(
        &mut manager,
        &[
            MotionEvent::Shake,
            MotionEvent::TiltEnter(TiltDir::Left),
            MotionEvent::Still,
        ],
    );
    let counts = manager.motion_counts_state();
    assert_eq!(counts.shakes, 1);
    assert_eq!(counts.tilt_enters, 1);
    assert_eq!(counts.still, 1);
}

#[test]
fn repeated_polls_accumulate_and_then_saturate() {
    let mut manager = DeviceManager::boot(Boot {
        backed: Backed {
            motion: Some(ALL_CAPS),
            ..Backed::NONE
        },
        ..Default::default()
    });
    poll(&mut manager, &[MotionEvent::Place]);
    poll(&mut manager, &[MotionEvent::Place]);
    poll(&mut manager, &[MotionEvent::Place]);
    assert_eq!(manager.motion_counts_state().places, 3);

    let mut saturated = MotionCounts {
        places: u16::MAX,
        ..MotionCounts::default()
    };
    saturated.bump(MotionEvent::Place);
    assert_eq!(
        saturated.places,
        u16::MAX,
        "a counter that wrapped would under-report a long session"
    );
}

#[test]
fn a_board_without_a_motion_source_counts_nothing() {
    let mut manager = DeviceManager::new();
    poll(
        &mut manager,
        &[MotionEvent::Shake, MotionEvent::Tap { count: 1 }],
    );
    assert_eq!(manager.motion_counts_state(), MotionCounts::default());
    assert_eq!(
        manager.state().backed.motion_caps(),
        MotionCapabilities::EMPTY
    );
}

#[test]
fn a_knock_and_a_finger_tap_are_recorded_as_separate_acts() {
    let mut manager = DeviceManager::boot(Boot {
        backed: Backed {
            motion: Some(ALL_CAPS),
            ..Backed::NONE
        },
        ..Default::default()
    });
    manager.apply_input(op(InputEvent::Gesture(GestureEvent::Tap {
        id: 0,
        x: 20,
        y: 30,
        held_ms: 40,
        at_ms: 40,
    })));
    poll(&mut manager, &[MotionEvent::Tap { count: 1 }]);
    assert_eq!(
        manager.state().touch.view.tap_count,
        1,
        "the finger tap still tallies"
    );
    assert_eq!(
        manager.motion_counts_state().taps,
        1,
        "the knock is not folded into the finger tap"
    );
}
