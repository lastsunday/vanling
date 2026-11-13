//! Where the splash lands and what it asks the link for on the way.

use super::*;

#[test]
fn a_splash_long_press_asks_for_setup_mode_on_every_board() {
    for kinds in [
        [SurfaceRole::Pages, SurfaceRole::Absent],
        [SurfaceRole::Lamp, SurfaceRole::Absent],
    ] {
        let mut manager = DeviceManager::boot(Boot {
            backed: Backed {
                network: true,
                ..Backed::NONE
            },
            surface_roles: kinds,
            ..Default::default()
        });
        let asked = long_press_the_button(&mut manager);
        assert!(
            manager.setup_requested(),
            "{kinds:?}: the request is latched"
        );
        assert_eq!(
            manager.state().network_page.confirm,
            None,
            "{kinds:?}: the splash carries no question"
        );
        assert_eq!(asked, None, "{kinds:?}: the request is not a leave");
        assert_eq!(
            manager.state().page(),
            Page::Splash,
            "{kinds:?}: the page keeps its window"
        );
    }
}

#[test]
fn the_splash_lands_with_the_setup_request_the_state_holds() {
    let mut manager = DeviceManager::boot(Boot {
        backed: Backed {
            network: true,
            ..Backed::NONE
        },
        ..Default::default()
    });
    long_press_the_button(&mut manager);
    assert!(
        manager.state().setup_requested,
        "the request lives in the state the landing reads"
    );

    let command = manager.apply_business(BusinessIntent::Navigation(NavigationIntent::SplashOver));
    assert_eq!(
        command,
        Some(Command::Network(NetworkCommand::SplashOver { setup: true })),
        "the landing reads the request from state, not from the manager"
    );
}

#[test]
fn the_splash_is_stacked_over_the_landing_page_and_pops_off_it() {
    let mut manager = DeviceManager::boot(Boot {
        backed: Backed {
            network: true,
            camera: true,
            ..Backed::NONE
        },
        boot_page: Page::Camera,
        ..Default::default()
    });
    assert_eq!(
        manager.state().nav.entries(),
        &[Page::Camera, Page::Splash],
        "the landing page sits under the transient"
    );
    manager.apply_business(BusinessIntent::Navigation(NavigationIntent::SplashOver));
    assert_eq!(
        manager.state().nav.entries(),
        &[Page::Camera],
        "the transient is gone and the root is what remains"
    );
    assert_eq!(manager.boot_page(), Page::Camera);
}

#[test]
fn the_splash_lands_on_the_page_that_offers_setup_when_it_was_asked_for() {
    let mut manager = DeviceManager::boot(Boot {
        backed: Backed {
            network: true,
            camera: true,
            ..Backed::NONE
        },
        boot_page: Page::Camera,
        ..Default::default()
    });
    long_press_the_button(&mut manager);
    manager.apply_business(BusinessIntent::Navigation(NavigationIntent::SplashOver));
    assert_eq!(manager.state().page(), Page::Network);
    assert_eq!(
        manager.state().nav.entries(),
        &[Page::Network],
        "the setup landing replaces the boot page rather than stacking on it"
    );
}

#[test]
fn the_splash_lands_on_the_build_page_when_nothing_was_asked() {
    let manager = landed(DeviceManager::boot(Boot {
        backed: Backed {
            network: true,
            camera: true,
            ..Backed::NONE
        },
        boot_page: Page::Camera,
        ..Default::default()
    }));
    assert_eq!(manager.state().page(), Page::Camera);
    assert!(!manager.setup_requested());
}

#[test]
fn a_board_with_no_radio_lands_on_its_page_even_after_the_hold() {
    let mut manager = DeviceManager::new();
    long_press_the_button(&mut manager);
    manager.apply_business(BusinessIntent::Navigation(NavigationIntent::SplashOver));
    assert_eq!(manager.state().page(), Page::Ambient);
}

#[test]
fn a_long_press_after_the_splash_is_the_pages_own_verb() {
    let mut manager = landed(DeviceManager::boot(Boot {
        backed: Backed {
            network: true,
            ..Backed::NONE
        },
        surface_roles: [SurfaceRole::Lamp, SurfaceRole::Absent],
        ..Default::default()
    }));
    link_up(&mut manager, b"neighbour");
    let before = manager.state().lights[0];

    let asked = long_press_the_button(&mut manager);
    assert_eq!(
        manager.state().network_page.confirm,
        None,
        "the press never raises the question the strip raises"
    );
    assert_eq!(asked, None, "and it takes nothing down");
    assert_ne!(
        manager.state().lights[0],
        before,
        "the press is still the page's own verb"
    );
}

#[test]
fn the_splash_lands_once_and_says_whether_setup_was_asked_for() {
    let mut manager = on_network_page();
    manager.state.nav.push(Page::Splash, manager.state.backed);
    long_press_the_button(&mut manager);
    assert_eq!(
        manager.apply_business(BusinessIntent::Navigation(NavigationIntent::SplashOver)),
        Some(Command::Network(NetworkCommand::SplashOver { setup: true })),
        "one answer: the page ended, and setup mode was asked for on it"
    );
    assert_eq!(
        manager.apply_business(BusinessIntent::Navigation(NavigationIntent::SplashOver)),
        None,
        "and the landing is news once"
    );
}

#[test]
fn a_boot_through_the_splash_tells_the_link_that_setup_was_not_asked_for() {
    let mut booting = on_network_page();
    booting.state.nav.push(Page::Splash, booting.state.backed);
    assert_eq!(
        booting.apply_business(BusinessIntent::Navigation(NavigationIntent::SplashOver)),
        Some(Command::Network(NetworkCommand::SplashOver {
            setup: false
        })),
        "the boot's own landing is news once"
    );
}
