//! Walking the cycle: the button reaches a page only when the board backs it, and a full lap
//! comes home.

use super::*;

fn board(backed: Backed) -> DeviceManager {
    landed(DeviceManager::boot(Boot {
        backed,
        ..Default::default()
    }))
}

#[test]
fn the_page_switch_reaches_the_audio_page_and_back() {
    let mut manager = board(Backed {
        audio: true,
        ..Backed::NONE
    });
    feed_input(&mut manager, triple_click());
    assert_eq!(manager.state().page(), Page::Audio);
    feed_input(&mut manager, triple_click());
    assert_eq!(manager.state().page(), Page::Ambient);
}

#[test]
fn the_camera_page_cycles_from_the_board_and_back() {
    assert!(
        !DeviceManager::new().state().backed.camera,
        "not declared until a board declares it"
    );
    let mut manager = board(Backed {
        camera: true,
        ..Backed::NONE
    });
    assert!(manager.state().backed.camera);
    feed_input(&mut manager, triple_click());
    assert_eq!(manager.state().page(), Page::Camera);
    feed_input(&mut manager, triple_click());
    assert_eq!(manager.state().page(), Page::Ambient);
}

#[test]
fn a_board_without_a_camera_never_lands_on_its_page() {
    let mut manager = board(Backed {
        audio: true,
        playback: true,
        ..Backed::NONE
    });
    for _ in 0..8 {
        feed_input(&mut manager, triple_click());
        assert_ne!(
            manager.state().page(),
            Page::Camera,
            "the camera page is gated on the wiring, not on how long you press"
        );
    }
}

#[test]
fn a_full_lap_walks_every_page_and_returns_home() {
    let mut manager = board(Backed {
        motion: Some(MotionCapabilities::EMPTY),
        audio: true,
        playback: true,
        ..Backed::NONE
    });
    for expected in [Page::Attitude, Page::Audio, Page::Speaker, Page::Ambient] {
        feed_input(&mut manager, triple_click());
        assert_eq!(manager.state().page(), expected);
    }
}
