//! The cross-page answers of the input boundary: what the splash admits, and the setup
//! shortcut that outranks the page's own verb.

use super::*;
use crate::shared::intent::NetworkIntent;
use iot_core::drivers::network::{LinkPhase, LinkStatus, NetworkMode, SetupHost};

fn connected(state: DeviceState) -> DeviceState {
    with_network(state, |network| {
        network.link = LinkStatus {
            phase: LinkPhase::Connected,
            ..LinkStatus::default()
        };
    })
}

fn in_setup(state: DeviceState) -> DeviceState {
    with_network(state, |network| {
        network.link.mode = NetworkMode::Setup {
            host: SetupHost::Panel,
            cancellable: false,
        };
    })
}

#[test]
fn the_splash_page_is_where_the_button_asks_for_setup_mode() {
    let booting = state_on(PageId::Splash, LightState::Off);
    assert_eq!(
        translate(&op(long_press_event()), &booting, &Services::default()),
        Some(BusinessIntent::Navigation(NavigationIntent::RequestSetup))
    );
}

#[test]
fn the_splash_page_answers_nothing_but_the_hold() {
    let booting = state_on(PageId::Splash, LightState::Off);
    for event in [
        InputEvent::Button(ButtonEvent::Click),
        InputEvent::Button(ButtonEvent::DoubleClick),
        InputEvent::Button(ButtonEvent::TripleClick),
        tap_gesture(0),
        swipe_event(SwipeDirection::Up),
        InputEvent::Gesture(GestureEvent::LongPress {
            id: 0,
            x: 0,
            y: 0,
            held_ms: 800,
        }),
    ] {
        assert_eq!(
            translate(&op(event), &booting, &Services::default()),
            None,
            "{event:?} on the splash"
        );
    }
    assert_eq!(
        translate(
            &op(long_press_event()),
            &connected(booting),
            &Services::default()
        ),
        Some(BusinessIntent::Navigation(NavigationIntent::RequestSetup))
    );
}

#[test]
fn a_long_press_once_the_splash_is_over_is_the_pages_own_verb() {
    assert_eq!(
        translate(
            &op(long_press_event()),
            &connected(state_on(PageId::Input, LightState::Off)),
            &Services::default()
        ),
        Some(BusinessIntent::Light {
            instance: 0,
            action: LightAction::Cycle,
        })
    );
}

#[test]
fn a_long_press_offers_the_access_points_front_whatever_page_is_up() {
    assert_eq!(
        translate(
            &op(long_press_event()),
            &in_setup(state_on(PageId::Input, LightState::Off)),
            &Services::default()
        ),
        Some(BusinessIntent::Network(NetworkIntent::SwitchHost))
    );
}
