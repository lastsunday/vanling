use super::{MOTION_BACKGROUND, ON_SCREEN};
use crate::demand::{Claim, DemandKind};
use crate::diagnostics::Diagnostics;
use crate::drivers::input::{ButtonEvent, GestureEvent, InputEvent};
use crate::intent::{BusinessIntent, CaptureAction, OperationIntent};
use crate::page::light;
use crate::state::{AudioPhase, DeviceState};

pub(super) const fn claim(kind: DemandKind) -> Claim {
    match kind {
        DemandKind::Audio => ON_SCREEN,
        DemandKind::Motion => MOTION_BACKGROUND,
        DemandKind::Display => ON_SCREEN,
        DemandKind::Interaction | DemandKind::Camera => Claim::off(),
    }
}

pub(super) fn translate(op: &OperationIntent, current: &DeviceState) -> BusinessIntent {
    match op.event {
        InputEvent::Button(ButtonEvent::Click) | InputEvent::Gesture(GestureEvent::Tap { .. }) => {
            BusinessIntent::Capture(capture_target(current.audio.phase))
        }
        InputEvent::Button(ButtonEvent::DoubleClick) => light::step(current, op.source),
        InputEvent::Button(ButtonEvent::LongPress) => light::cycle(current, op.source),
        InputEvent::Gesture(GestureEvent::LongPress { .. }) => {
            light::cycle_panel(current, op.source)
        }
        InputEvent::Gesture(GestureEvent::Swipe { direction, .. }) => {
            light::swipe(current, op.source, direction)
        }
        _ => BusinessIntent::Invalid,
    }
}

pub(super) fn repaint(prev: &Diagnostics, next: &Diagnostics, _overlay: bool) -> bool {
    prev.audio.phase != next.audio.phase
        || prev.audio.envelope != next.audio.envelope
        || prev.audio.restarts != next.audio.restarts
        || prev.audio.dba_lsb != next.audio.dba_lsb
}

fn capture_target(phase: AudioPhase) -> CaptureAction {
    if phase == AudioPhase::Recording {
        CaptureAction::Stop
    } else {
        CaptureAction::Start
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::translate;
    use crate::page::light::COLOR_GROUPS;
    use crate::page::test_support::{
        BREATH_BASE, DEFAULT_BREATH, op, state, state_on, tap_gesture, target, target_in,
    };
    use crate::state::{DisplayPage, LightState};

    mod audio_page {
        use super::*;

        #[test]
        fn a_button_click_on_the_audio_page_records_and_elsewhere_moves_the_color() {
            let on_audio = state_on(DisplayPage::Audio, DEFAULT_BREATH);
            assert_eq!(
                translate(&op(InputEvent::Button(ButtonEvent::Click)), &on_audio),
                BusinessIntent::Capture(CaptureAction::Start),
                "on the audio page the button is the record control, the same as the tap"
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state_on(DisplayPage::Camera, DEFAULT_BREATH)
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Breath(COLOR_GROUPS[1].into_breath(BREATH_BASE)),
                },
                "the camera page is not a control page, so the colour still walks"
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state(DEFAULT_BREATH)
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Breath(COLOR_GROUPS[1].into_breath(BREATH_BASE)),
                }
            );
            assert_eq!(
                translate(
                    &op(InputEvent::Button(ButtonEvent::Click)),
                    &state_on(DisplayPage::Attitude, DEFAULT_BREATH)
                ),
                BusinessIntent::SetLight {
                    instance: 0,
                    state: LightState::Breath(COLOR_GROUPS[1].into_breath(BREATH_BASE)),
                }
            );
        }

        #[test]
        fn a_tap_on_the_audio_page_records_and_elsewhere_moves_the_color() {
            let on_audio = state_on(DisplayPage::Audio, DEFAULT_BREATH);
            assert_eq!(
                translate(&op(tap_gesture(0)), &on_audio),
                BusinessIntent::Capture(CaptureAction::Start),
                "on the audio page the tap is the record control"
            );
            assert_eq!(
                translate(&op(tap_gesture(0)), &state(LightState::Off)),
                BusinessIntent::Invalid
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
