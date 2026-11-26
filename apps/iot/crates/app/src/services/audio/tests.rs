//! The Audio page's input contract: the tap is its record control, and nowhere else's.

use crate::shared::intent::AudioIntent;
use crate::state::test_support::*;

#[test]
fn a_button_click_only_turns_the_page() {
    assert_eq!(
        translate(
            &op(InputEvent::Button(ButtonEvent::Click)),
            &state_on(PageId::Audio, BREATH),
            &Services::default()
        ),
        Some(BusinessIntent::Navigation(NavigationIntent::NextPage)),
        "the button is the page cycle, not the record control"
    );
}

#[test]
fn a_tap_records_on_the_audio_page_and_does_nothing_elsewhere() {
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
        None,
        "the attitude page has no light verb"
    );
}
