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
    use crate::model::intent::{ActionIntent, AudioIntent, LightIntent};
    use crate::model::route::test_support::{
        BREATH_BASE, DEFAULT_BREATH, op, state, state_on, tap_gesture, target, target_in,
    };
    use crate::model::state::{COLOR_GROUPS, DisplayPage, LightState, translate};
    use iot_core::drivers::input::{ButtonEvent, InputEvent};

    mod audio_page {
        use super::*;

        #[test]
        fn a_button_click_on_the_audio_page_records_and_elsewhere_moves_the_color() {
            let on_audio = state_on(DisplayPage::Audio, DEFAULT_BREATH);
            assert_eq!(
                translate(&op(InputEvent::Button(ButtonEvent::Click)), &on_audio),
                ActionIntent::Audio(AudioIntent::Start),
                "on the audio page the button is the record control, the same as the tap"
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state_on(DisplayPage::Camera, DEFAULT_BREATH)
                ),
                ActionIntent::Light(LightIntent {
                    instance: 0,
                    state: LightState::Breath(COLOR_GROUPS[1].into_breath(BREATH_BASE)),
                }),
                "the camera page is not a control page, so the colour still walks"
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state(DEFAULT_BREATH)
                ),
                ActionIntent::Light(LightIntent {
                    instance: 0,
                    state: LightState::Breath(COLOR_GROUPS[1].into_breath(BREATH_BASE)),
                })
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state_on(DisplayPage::Attitude, DEFAULT_BREATH)
                ),
                ActionIntent::Light(LightIntent {
                    instance: 0,
                    state: LightState::Breath(COLOR_GROUPS[1].into_breath(BREATH_BASE)),
                })
            );
        }

        #[test]
        fn a_tap_on_the_audio_page_records_and_elsewhere_moves_the_color() {
            let on_audio = state_on(DisplayPage::Audio, DEFAULT_BREATH);
            assert_eq!(
                translate(&op(tap_gesture(0)), &on_audio),
                ActionIntent::Audio(AudioIntent::Start),
                "on the audio page the tap is the record control"
            );
            assert_eq!(
                translate(&op(tap_gesture(0)), &state(LightState::Off)),
                ActionIntent::Invalid
            );
            assert_eq!(
                target_in(
                    tap_gesture(0),
                    &state_on(DisplayPage::Attitude, DEFAULT_BREATH)
                ),
                target(tap_gesture(0), DEFAULT_BREATH)
            );
        }
    }
}
