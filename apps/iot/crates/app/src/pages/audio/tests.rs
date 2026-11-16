//! The Audio page's input contract: the button and the tap are its record control, and
//! nowhere else's.

use crate::shared::intent::AudioIntent;
use crate::state::test_support::*;

#[test]
fn a_button_click_on_the_audio_page_records_and_elsewhere_moves_the_color() {
    let click = op(InputEvent::Button(ButtonEvent::Click));
    assert_eq!(
        translate(&click, &state_on(Page::Audio, BREATH)),
        BusinessIntent::Audio(AudioIntent::Start),
        "on the audio page the button is the record control, the same as the tap"
    );
    let color = BusinessIntent::Light {
        instance: 0,
        action: LightAction::NextColor { forward: true },
    };
    for page in [Page::Ambient, Page::Attitude, Page::Camera] {
        assert_eq!(
            translate(&click, &state_on(page, BREATH)),
            color,
            "{page:?} is not a control page, so the colour still walks"
        );
    }
}

#[test]
fn a_tap_on_the_audio_page_records_and_elsewhere_moves_the_color() {
    let tap = tap_gesture(0);
    assert_eq!(
        translate(&op(tap), &state_on(Page::Audio, BREATH)),
        BusinessIntent::Audio(AudioIntent::Start),
        "on the audio page the tap is the record control"
    );
    assert_eq!(
        translate(&op(tap), &state(LightState::Off)),
        BusinessIntent::Invalid
    );
    assert_eq!(
        target_in(tap, &state_on(Page::Attitude, BREATH)),
        target(tap, BREATH)
    );
}
