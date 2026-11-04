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
        assert!(manager.setup_wanted(), "{kinds:?}: the request is latched");
        assert_eq!(
            manager.state().network_page.confirm,
            None,
            "{kinds:?}: the splash carries no question"
        );
        assert_eq!(asked, None, "{kinds:?}: the request is not a leave");
        assert_eq!(
            manager.state().page(),
            DisplayPage::Splash,
            "{kinds:?}: the page keeps its window"
        );
    }
}

#[test]
fn the_splash_is_stacked_over_the_landing_page_and_pops_off_it() {
    let mut manager = DeviceManager::boot(Boot {
        backed: Backed {
            network: true,
            camera: true,
            ..Backed::NONE
        },
        boot_page: DisplayPage::Camera,
        ..Default::default()
    });
    assert_eq!(
        manager.state().nav.entries(),
        &[DisplayPage::Camera, DisplayPage::Splash],
        "the landing page sits under the transient"
    );
    manager.apply_action(ActionIntent::Page(PageIntent::SplashOver));
    assert_eq!(
        manager.state().nav.entries(),
        &[DisplayPage::Camera],
        "the transient is gone and the root is what remains"
    );
    assert_eq!(manager.boot_page(), DisplayPage::Camera);
}

#[test]
fn the_splash_lands_on_the_page_that_offers_setup_when_it_was_asked_for() {
    let mut manager = DeviceManager::boot(Boot {
        backed: Backed {
            network: true,
            camera: true,
            ..Backed::NONE
        },
        boot_page: DisplayPage::Camera,
        ..Default::default()
    });
    long_press_the_button(&mut manager);
    manager.apply_action(ActionIntent::Page(PageIntent::SplashOver));
    assert_eq!(manager.state().page(), DisplayPage::Network);
    assert_eq!(
        manager.state().nav.entries(),
        &[DisplayPage::Network],
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
        boot_page: DisplayPage::Camera,
        ..Default::default()
    }));
    assert_eq!(manager.state().page(), DisplayPage::Camera);
    assert!(!manager.setup_wanted());
}

#[test]
fn a_board_with_no_radio_lands_on_its_page_even_after_the_hold() {
    let mut manager = DeviceManager::new();
    long_press_the_button(&mut manager);
    manager.apply_action(ActionIntent::Page(PageIntent::SplashOver));
    assert_eq!(manager.state().page(), DisplayPage::Ambient);
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
    manager
        .state
        .nav
        .push(DisplayPage::Splash, manager.state.backed);
    long_press_the_button(&mut manager);
    assert_eq!(
        manager.apply_action(ActionIntent::Page(PageIntent::SplashOver)),
        Some(NetworkCommand::SplashOver { setup: true }),
        "one answer: the page ended, and setup mode was asked for on it"
    );
    assert_eq!(
        manager.apply_action(ActionIntent::Page(PageIntent::SplashOver)),
        None,
        "and the landing is news once"
    );
}

#[test]
fn a_boot_through_the_splash_tells_the_link_that_setup_was_not_asked_for() {
    let mut booting = on_network_page();
    booting
        .state
        .nav
        .push(DisplayPage::Splash, booting.state.backed);
    assert_eq!(
        booting.apply_action(ActionIntent::Page(PageIntent::SplashOver)),
        Some(NetworkCommand::SplashOver { setup: false }),
        "the boot's own landing is news once"
    );
}
