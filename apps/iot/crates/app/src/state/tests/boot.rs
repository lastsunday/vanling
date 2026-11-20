use super::*;

#[test]
fn state_is_clean() {
    let manager = DeviceManager::new();
    assert_eq!(manager.light_state(), BREATH, "boot wakes to breathing");
    assert_eq!(DeviceState::default().lights[0], BREATH);
    assert_eq!(DeviceManager::default().light_state(), BREATH);
    let state = manager.state();
    assert_eq!(manager.touch_state(), None);
    assert_eq!(state.touch.view.tap_count, 0);
    assert_eq!(state.touch.view.press_count, 0);
    assert_eq!(state.touch.view.long_press_count, 0);
    assert_eq!(state.touch.view.ghost_count, 0);
    assert_eq!(state.touch.view.swipe_count, 0);
    assert_eq!(state.touch.two_finger_runs, 0);
    assert_eq!(state.touch.frames, 0);
    assert_eq!(state.touch.view.chip_gesture_id, 0);
    assert_eq!(state.touch.view.last_swipe, None);
    assert_eq!(state.touch.view.last_gesture_origin, None);
    assert_eq!(state.touch.view.last_gesture_end, None);
}

#[test]
fn the_wiring_record_is_what_the_page_table_asks() {
    let bare = DeviceManager::new().state().backed;
    assert_eq!(bare, Backed::NONE);
    assert_eq!(
        bare.motion_caps(),
        MotionCapabilities::EMPTY,
        "a board with no part declares nothing, whatever the board would report"
    );
    assert!(!PageId::Attitude.is_backed(bare));

    let wired = DeviceManager::boot(Boot {
        backed: Backed {
            motion: Some(MotionCapabilities::TAP),
            audio: true,
            ..Backed::NONE
        },
        ..Default::default()
    })
    .state()
    .backed;
    assert_eq!(wired.motion, Some(MotionCapabilities::TAP));
    assert_eq!(wired.motion_caps(), MotionCapabilities::TAP);
    assert!(PageId::Attitude.is_backed(wired));
    assert!(PageId::Audio.is_backed(wired));
    assert!(!PageId::Speaker.is_backed(wired));
}

#[test]
fn every_present_device_claims_its_key_in_a_fixed_order() {
    let all = Backed {
        motion: Some(MotionCapabilities::EMPTY),
        audio: true,
        playback: true,
        camera: true,
        ..Backed::NONE
    };
    assert_eq!(
        all.demand_keys(),
        alloc::vec![
            DemandKey::DISPLAY,
            DemandKey::MOTION,
            DemandKey::AUDIO,
            DemandKey::CAMERA
        ],
        "a page claims exactly the devices the board declared; playback owns no demand key"
    );
}

#[test]
fn an_absent_device_never_enters_the_demand() {
    assert_eq!(
        Backed::NONE.demand_keys(),
        alloc::vec![DemandKey::DISPLAY],
        "only the panel is claimed when the board declared nothing else"
    );
}

#[test]
fn the_boot_page_joins_the_cycle_it_was_asked_for() {
    let mut manager = landed(DeviceManager::boot(Boot {
        backed: Backed {
            motion: Some(MotionCapabilities::EMPTY),
            audio: true,
            playback: true,
            camera: true,
            ..Backed::NONE
        },
        boot_page: PageId::Camera,
        ..Default::default()
    }));
    assert_eq!(manager.state().page(), PageId::Camera);
    for expected in [PageId::Env, PageId::Text, PageId::Ambient, PageId::Attitude] {
        feed_input(&mut manager, triple_click());
        assert_eq!(manager.state().page(), expected);
    }
}

#[test]
fn the_splash_is_never_where_the_device_lands() {
    let landed_on = landed(DeviceManager::boot(Boot {
        boot_page: PageId::Splash,
        ..Default::default()
    }))
    .state()
    .page();
    assert_eq!(landed_on, PageId::Ambient);
}

#[test]
fn the_boot_page_falls_back_for_every_capability_not_wired() {
    let bare = landed(DeviceManager::new());
    let default_page = bare.state().page();
    for page in cycled() {
        let landed_on = landed(DeviceManager::boot(Boot {
            boot_page: page,
            ..Default::default()
        }))
        .state()
        .page();
        let expected = if page.is_backed(Backed::NONE) {
            page
        } else {
            default_page
        };
        assert_eq!(
            landed_on, expected,
            "{page:?} lands where the bare board can show it, and on the default otherwise"
        );
    }
    assert_eq!(
        landed(DeviceManager::boot(Boot {
            backed: Backed {
                motion: Some(MotionCapabilities::EMPTY),
                ..Backed::NONE
            },
            boot_page: PageId::Attitude,
            ..Default::default()
        }))
        .state()
        .page(),
        PageId::Attitude,
        "the one capability it does have"
    );
    assert_eq!(
        landed(DeviceManager::boot(Boot {
            backed: Backed {
                motion: Some(MotionCapabilities::EMPTY),
                ..Backed::NONE
            },
            boot_page: PageId::Camera,
            ..Default::default()
        }))
        .state()
        .page(),
        PageId::Ambient,
        "and the one it does not"
    );
}
