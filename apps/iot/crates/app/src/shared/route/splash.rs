use super::{MOTION_BACKGROUND, ON_SCREEN};
use iot_core::demand::{Claim, DemandKind};

/// How long the boot page stays up before the build's page takes over. A page property:
/// the splash owns its claim; this is how long it exists.
pub const SPLASH_MS: u64 = 1_500;

pub(super) const fn claim(kind: DemandKind) -> Claim {
    match kind {
        DemandKind::Motion => MOTION_BACKGROUND,
        DemandKind::Display => ON_SCREEN,
        DemandKind::Interaction | DemandKind::Audio | DemandKind::Camera => Claim::off(),
    }
}

#[cfg(test)]
mod tests {
    use crate::shared::intent::{ActionIntent, LightIntent, PageIntent};
    use crate::shared::route::test_support::{
        DEFAULT_BREATH, long_press, op, state_on, swipe_event, tap_gesture, with_network,
    };
    use crate::state::translate::translate;
    use crate::state::{LightState, Page};
    use iot_core::drivers::input::{ButtonEvent, GestureEvent, InputEvent, SwipeDirection};
    use iot_core::drivers::network::LinkStatus;

    #[test]
    fn the_splash_page_is_where_the_button_asks_for_setup_mode() {
        let booting = state_on(Page::Splash, LightState::Off);
        assert_eq!(
            translate(&op(long_press()), &booting),
            ActionIntent::Page(PageIntent::RequestSetup)
        );
    }

    #[test]
    fn the_splash_page_answers_nothing_but_the_hold() {
        let booting = state_on(Page::Splash, LightState::Off);
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
                translate(&op(event), &booting),
                ActionIntent::Invalid,
                "{event:?} on the splash"
            );
        }
        let with_link = with_network(state_on(Page::Splash, LightState::Off), |network| {
            network.link = LinkStatus {
                phase: iot_core::drivers::network::LinkPhase::Connected,
                ..LinkStatus::default()
            };
        });
        assert_eq!(
            translate(&op(long_press()), &with_link),
            ActionIntent::Page(PageIntent::RequestSetup)
        );
    }

    #[test]
    fn a_long_press_once_the_splash_is_over_is_the_pages_own_verb() {
        let on_air = with_network(state_on(Page::Ambient, LightState::Off), |network| {
            network.link = LinkStatus {
                phase: iot_core::drivers::network::LinkPhase::Connected,
                ..LinkStatus::default()
            };
        });
        assert_eq!(
            translate(&op(long_press()), &on_air),
            ActionIntent::Light(LightIntent {
                instance: 0,
                state: DEFAULT_BREATH,
            })
        );
    }
}
