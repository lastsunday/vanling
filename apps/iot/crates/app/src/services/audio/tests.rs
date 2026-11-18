//! The Audio page's input contract: the button and the tap are its record control, and
//! nowhere else's.

use crate::shared::intent::AudioIntent;
use crate::state::test_support::*;

#[test]
fn a_button_click_on_the_audio_page_records_and_elsewhere_moves_the_color() {
    let click = op(InputEvent::Button(ButtonEvent::Click));
    assert_eq!(
        translate(
            &click,
            &state_on(PageId::Audio, BREATH),
            &Services::default()
        ),
        Some(BusinessIntent::Audio(AudioIntent::Start)),
        "on the audio page the button is the record control, the same as the tap"
    );
    let color = BusinessIntent::Light {
        instance: 0,
        action: LightAction::NextColor { forward: true },
    };
    for page in [PageId::Ambient, PageId::Attitude, PageId::Camera] {
        assert_eq!(
            translate(&click, &state_on(page, BREATH), &Services::default()),
            Some(color),
            "{page:?} is not a control page, so the colour still walks"
        );
    }
}

#[test]
fn a_tap_on_the_audio_page_records_and_elsewhere_moves_the_color() {
    let tap = tap_gesture(0);
    assert_eq!(
        translate(
            &op(tap),
            &state_on(PageId::Audio, BREATH),
            &Services::default()
        ),
        Some(BusinessIntent::Audio(AudioIntent::Start)),
        "on the audio page the tap is the record control"
    );
    assert_eq!(
        translate(&op(tap), &state(LightState::Off), &Services::default()),
        None
    );
    assert_eq!(
        target_in(tap, &state_on(PageId::Attitude, BREATH)),
        target(tap, BREATH)
    );
}
