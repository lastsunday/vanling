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
    assert!(!DisplayPage::Attitude.is_backed(bare));

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
    assert!(DisplayPage::Attitude.is_backed(wired));
    assert!(DisplayPage::Audio.is_backed(wired));
    assert!(!DisplayPage::Speaker.is_backed(wired));
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
