use crate::drivers::input::{ButtonEvent, GestureEvent, InputEvent, SwipeDirection};
use crate::drivers::motion::MotionCapabilities;
use crate::intent::{
    ActionIntent, InputIntent, LightIntent, NetworkIntent, PageIntent, recognize, translate,
};
use crate::state::MAX_LIGHTS;
use crate::state::{Backed, Breath, DeviceState, DisplayPage, LightState, SurfaceRole};

pub(crate) const DEFAULT_BREATH: LightState = LightState::Breath(BREATH_BASE);

pub(crate) const BREATH_BASE: Breath = Breath {
    period_ms: LightState::DEFAULT_PERIOD_MS,
    hue_period_ms: LightState::DEFAULT_HUE_PERIOD_MS,
    hue_span: LightState::DEFAULT_HUE_SPAN,
    min_brightness: LightState::DEFAULT_MIN_BRIGHTNESS,
    max_brightness: LightState::DEFAULT_MAX_BRIGHTNESS,
    saturation: LightState::DEFAULT_SATURATION,
    hue: LightState::DEFAULT_HUE,
    group: LightState::DEFAULT_GROUP,
    group_len: LightState::DEFAULT_GROUP_LEN,
};

pub(crate) fn state(light: LightState) -> DeviceState {
    state_on(DisplayPage::Ambient, light)
}

pub(crate) fn state_on(page: DisplayPage, light: LightState) -> DeviceState {
    DeviceState {
        lights: [light; MAX_LIGHTS],
        surface_roles: [SurfaceRole::Pages, SurfaceRole::Absent],
        page,
        backed: Backed {
            motion: Some(MotionCapabilities::EMPTY),
            audio: true,
            playback: true,
            camera: true,
            network: true,
        },
        ..DeviceState::default()
    }
}

/// A state whose network record is the base's with one fact changed. The sites below each
/// set a single network field, and nesting it by hand would bury that fact under a record
/// that is otherwise untouched.
pub(crate) fn with_network(
    mut state: DeviceState,
    change: impl FnOnce(&mut crate::state::NetworkState),
) -> DeviceState {
    change(&mut state.network);
    state
}

pub(crate) fn op(event: InputEvent) -> InputIntent {
    recognize(event, 0)
}

pub(crate) fn long_press() -> InputEvent {
    InputEvent::Button(ButtonEvent::LongPress)
}

pub(crate) fn target(event: InputEvent, current: LightState) -> Option<LightState> {
    target_in(event, &state(current))
}

pub(crate) fn target_in(event: InputEvent, current: &DeviceState) -> Option<LightState> {
    match translate(&op(event), current) {
        ActionIntent::Light(LightIntent { state, .. }) => Some(state),
        ActionIntent::Invalid
        | ActionIntent::Page(PageIntent::NextPage)
        | ActionIntent::Audio(_)
        | ActionIntent::Playback(_)
        | ActionIntent::Page(PageIntent::SplashOver)
        | ActionIntent::Page(PageIntent::RequestSetup)
        | ActionIntent::Network(NetworkIntent::LinkReport(_))
        | ActionIntent::Network(NetworkIntent::NoCopy(_))
        | ActionIntent::Network(NetworkIntent::Mac(_))
        | ActionIntent::Network(NetworkIntent::Key { .. })
        | ActionIntent::Network(NetworkIntent::Choose(_))
        | ActionIntent::Network(NetworkIntent::AskForget(_))
        | ActionIntent::Network(NetworkIntent::Scroll { .. })
        | ActionIntent::Network(NetworkIntent::Leave)
        | ActionIntent::Network(NetworkIntent::Answer(_))
        | ActionIntent::Network(NetworkIntent::CloseSetup)
        | ActionIntent::Network(NetworkIntent::SwitchHost)
        | ActionIntent::Network(NetworkIntent::ScanNow)
        | ActionIntent::Network(NetworkIntent::SetOpenAccessPoint(_))
        | ActionIntent::Network(NetworkIntent::Dismiss) => None,
    }
}

pub(crate) fn tap_gesture(held_ms: u16) -> InputEvent {
    InputEvent::Gesture(GestureEvent::Tap {
        id: 0,
        x: 0,
        y: 0,
        held_ms,
        at_ms: 0,
    })
}

pub(crate) fn swipe_event(direction: SwipeDirection) -> InputEvent {
    InputEvent::Gesture(GestureEvent::Swipe {
        id: 0,
        direction,
        x: 0,
        y: 0,
        end_x: 0,
        end_y: 0,
        held_ms: 0,
        distance_px: 80,
    })
}
