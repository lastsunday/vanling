use super::{MOTION_BACKGROUND, ON_SCREEN};
use iot_core::demand::{Claim, DemandKind};

pub(super) const fn claim(kind: DemandKind) -> Claim {
    match kind {
        DemandKind::Audio => ON_SCREEN,
        DemandKind::Motion => MOTION_BACKGROUND,
        DemandKind::Display => ON_SCREEN,
        DemandKind::Interaction | DemandKind::Camera => Claim::off(),
    }
}

#[cfg(test)]
mod tests {
    use crate::shared::intent::{AudioIntent, BusinessIntent, LightAction};
    use crate::shared::route::test_support::{
        DEFAULT_BREATH, op, state, state_on, tap_gesture, target, target_in,
    };
    use crate::state::translate::translate;
    use crate::state::{LightState, Page};
    use iot_core::drivers::input::{ButtonEvent, InputEvent};

    mod audio_page {
        use super::*;

        #[test]
        fn a_button_click_on_the_audio_page_records_and_elsewhere_moves_the_color() {
            let on_audio = state_on(Page::Audio, DEFAULT_BREATH);
            assert_eq!(
                translate(&op(InputEvent::Button(ButtonEvent::Click)), &on_audio),
                BusinessIntent::Audio(AudioIntent::Start),
                "on the audio page the button is the record control, the same as the tap"
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state_on(Page::Camera, DEFAULT_BREATH)
                ),
                BusinessIntent::Light {
                    instance: 0,
                    action: LightAction::NextColor { forward: true },
                },
                "the camera page is not a control page, so the colour still walks"
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state(DEFAULT_BREATH)
                ),
                BusinessIntent::Light {
                    instance: 0,
                    action: LightAction::NextColor { forward: true },
                }
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state_on(Page::Attitude, DEFAULT_BREATH)
                ),
                BusinessIntent::Light {
                    instance: 0,
                    action: LightAction::NextColor { forward: true },
                }
            );
        }

        #[test]
        fn a_tap_on_the_audio_page_records_and_elsewhere_moves_the_color() {
            let on_audio = state_on(Page::Audio, DEFAULT_BREATH);
            assert_eq!(
                translate(&op(tap_gesture(0)), &on_audio),
                BusinessIntent::Audio(AudioIntent::Start),
                "on the audio page the tap is the record control"
            );
            assert_eq!(
                translate(&op(tap_gesture(0)), &state(LightState::Off)),
                BusinessIntent::Invalid
            );
            assert_eq!(
                target_in(tap_gesture(0), &state_on(Page::Attitude, DEFAULT_BREATH)),
                target(tap_gesture(0), DEFAULT_BREATH)
            );
        }
    }
}
