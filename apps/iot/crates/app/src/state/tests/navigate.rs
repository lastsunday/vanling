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
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Audio);
    next_page(&mut manager);
    assert_eq!(
        manager.state().page(),
        PageId::Env,
        "the page that needs nothing"
    );
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Text);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Widgets);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Advanced);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Feedback);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Extended);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Data);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Planner);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Records);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::List);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Lighting);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Menu);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Input);
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
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Camera);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Env);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Text);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Widgets);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Advanced);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Feedback);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Extended);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Data);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Planner);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Records);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::List);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Lighting);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Menu);
    next_page(&mut manager);
    assert_eq!(manager.state().page(), PageId::Input);
}

#[test]
fn a_board_without_a_camera_never_lands_on_its_page() {
    let mut manager = board(Backed {
        audio: true,
        playback: true,
        ..Backed::NONE
    });
    for _ in 0..8 {
        next_page(&mut manager);
        assert_ne!(
            manager.state().page(),
            PageId::Camera,
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
    for expected in [
        PageId::Attitude,
        PageId::Audio,
        PageId::Speaker,
        PageId::Env,
        PageId::Text,
        PageId::Widgets,
        PageId::Advanced,
        PageId::Feedback,
        PageId::Extended,
        PageId::Data,
        PageId::Planner,
        PageId::Records,
        PageId::List,
        PageId::Lighting,
        PageId::Menu,
        PageId::Input,
    ] {
        next_page(&mut manager);
        assert_eq!(manager.state().page(), expected);
    }
}

#[test]
fn a_menu_button_opens_its_page_from_a_tap() {
    let mut manager = landed(DeviceManager::boot(Boot {
        backed: Backed {
            motion: Some(MotionCapabilities::EMPTY),
            audio: true,
            playback: true,
            camera: true,
            network: true,
        },
        surface_roles: [SurfaceRole::Screen, SurfaceRole::Absent],
        boot_page: PageId::Menu,
        ..Default::default()
    }));
    assert_eq!(manager.state().page(), PageId::Menu);

    // Ask the page where its button is, so a change to the head or the grid cannot strand
    // this test on a coordinate that no longer holds a button. `Attitude` sits in the first
    // band, so it is on screen at offset 0.
    let menu = crate::services::menu::MenuPage::default();
    let layout = crate::services::layout_of(&menu, PageId::Menu);
    let (x, y) = menu
        .button_centre(&layout, PageId::Attitude)
        .expect("`Attitude` is listed");
    feed_input(
        &mut manager,
        op(InputEvent::Gesture(GestureEvent::Press { id: 0, x, y })),
    );
    feed_input(
        &mut manager,
        op(InputEvent::Gesture(GestureEvent::Tap {
            id: 0,
            x,
            y,
            held_ms: 40,
            at_ms: 40,
        })),
    );
    assert_eq!(manager.state().page(), PageId::Attitude);
}
