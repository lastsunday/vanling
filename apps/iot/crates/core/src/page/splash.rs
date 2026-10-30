use super::{MOTION_BACKGROUND, ON_SCREEN};
use crate::demand::{Claim, DemandKind};
use crate::diagnostics::Diagnostics;
use crate::drivers::input::{ButtonEvent, InputEvent};
use crate::intent::{BusinessIntent, OperationIntent};
use crate::state::DeviceState;

/// How long the boot page stays up before the build's page takes over. A page property:
/// the splash owns its claim, gestures and repaint; this is how long it exists.
pub const SPLASH_MS: u64 = 1_500;

pub(super) const fn claim(kind: DemandKind) -> Claim {
    match kind {
        DemandKind::Motion => MOTION_BACKGROUND,
        DemandKind::Display => ON_SCREEN,
        DemandKind::Interaction | DemandKind::Audio | DemandKind::Camera => Claim::off(),
    }
}

pub(super) fn translate(op: &OperationIntent, _current: &DeviceState) -> BusinessIntent {
    match op.event {
        InputEvent::Button(ButtonEvent::LongPress) => BusinessIntent::RequestSetup,
        _ => BusinessIntent::Invalid,
    }
}

pub(super) const fn repaint(_prev: &Diagnostics, _next: &Diagnostics, _overlay: bool) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::input::{GestureEvent, SwipeDirection};
    use crate::drivers::network::LinkStatus;
    use crate::intent::{BusinessIntent, translate};
    use crate::page::test_support::{
        DEFAULT_BREATH, long_press, op, state_on, swipe_event, tap_gesture,
    };
    use crate::state::{DeviceState, DisplayPage, LightState};

    #[test]
    fn the_splash_page_is_where_the_button_asks_for_setup_mode() {
        let booting = state_on(DisplayPage::Splash, LightState::Off);
        assert_eq!(
            translate(&op(long_press()), &booting),
            BusinessIntent::RequestSetup
        );
    }

    #[test]
    fn the_splash_page_answers_nothing_but_the_hold() {
        let booting = state_on(DisplayPage::Splash, LightState::Off);
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
                BusinessIntent::Invalid,
                "{event:?} on the splash"
            );
        }
        let with_link = DeviceState {
            link: LinkStatus {
                phase: crate::drivers::network::LinkPhase::Connected,
                ..LinkStatus::default()
            },
            ..state_on(DisplayPage::Splash, LightState::Off)
        };
        assert_eq!(
            translate(&op(long_press()), &with_link),
            BusinessIntent::RequestSetup
        );
    }

    #[test]
    fn a_long_press_once_the_splash_is_over_is_the_pages_own_verb() {
        let on_air = DeviceState {
            link: LinkStatus {
                phase: crate::drivers::network::LinkPhase::Connected,
                ..LinkStatus::default()
            },
            ..state_on(DisplayPage::Ambient, LightState::Off)
        };
        assert_eq!(
            translate(&op(long_press()), &on_air),
            BusinessIntent::SetLight {
                instance: 0,
                state: DEFAULT_BREATH,
            }
        );
    }
}
