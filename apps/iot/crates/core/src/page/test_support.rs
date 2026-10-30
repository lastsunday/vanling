use crate::drivers::input::{
    ButtonEvent, FingerLast, GestureEvent, InputEvent, MAX_TRACKED_POINTS, SwipeDirection,
};
use crate::drivers::light::MAX_LIGHTS;
use crate::drivers::motion::{MotionCapabilities, MotionCounts};
use crate::intent::{BusinessIntent, OperationIntent, recognize, translate};
use crate::state::{
    AudioState, Breath, DeviceManager, DeviceState, DisplayPage, LightState, PlaybackState,
    SurfaceRole,
};

pub(crate) const DEFAULT_BREATH: LightState = LightState::Breath(BREATH_BASE);

pub(crate) const BREATH_BASE: Breath = Breath {
    period_ms: DeviceManager::DEFAULT_PERIOD_MS,
    hue_period_ms: DeviceManager::DEFAULT_HUE_PERIOD_MS,
    hue_span: DeviceManager::DEFAULT_HUE_SPAN,
    min_brightness: DeviceManager::DEFAULT_MIN_BRIGHTNESS,
    max_brightness: DeviceManager::DEFAULT_MAX_BRIGHTNESS,
    saturation: DeviceManager::DEFAULT_SATURATION,
    hue: DeviceManager::DEFAULT_HUE,
    group: DeviceManager::DEFAULT_GROUP,
    group_len: DeviceManager::DEFAULT_GROUP_LEN,
};

pub(crate) fn state(light: LightState) -> DeviceState {
    state_on(DisplayPage::Ambient, light)
}

pub(crate) fn state_on(page: DisplayPage, light: LightState) -> DeviceState {
    DeviceState {
        lights: [light; MAX_LIGHTS],
        surface_roles: [SurfaceRole::Pages, SurfaceRole::Absent],
        notice: None,
        outage_announced: false,
        page,
        link: crate::drivers::network::LinkStatus::default(),
        mac: None,
        aps: None,
        setup: crate::state::NetworkSetup::default(),
        setup_wanted: false,
        splash_reported: false,
        join: None,
        leave: false,
        setup_cancel: false,
        switch_front: false,
        setup_open: None,
        scan_now: false,
        forget: None,
        motion_enabled: true,
        motion: None,
        motion_counts: MotionCounts::default(),
        motion_caps: MotionCapabilities::EMPTY,
        audio_enabled: true,
        audio: AudioState::default(),
        playback_enabled: true,
        playback: PlaybackState::default(),
        camera_enabled: true,
        network_enabled: true,
        touch: None,
        touch_points: [None; MAX_TRACKED_POINTS],
        live_dir: [0; MAX_TRACKED_POINTS],
        two_finger_runs: 0,
        touch_frames: 0,
        finger: [FingerLast::default(); MAX_TRACKED_POINTS],
        touch_held_ms: 0,
        tap_count: 0,
        press_count: 0,
        long_press_count: 0,
        ghost_count: 0,
        swipe_count: 0,
        last_swipe: None,
        last_gesture_origin: None,
        last_gesture_end: None,
        chip_gesture_id: 0,
    }
}

pub(crate) fn op(event: InputEvent) -> OperationIntent {
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
        BusinessIntent::SetLight { state, .. } => Some(state),
        BusinessIntent::Invalid
        | BusinessIntent::NextPage
        | BusinessIntent::Capture(_)
        | BusinessIntent::Playback(_)
        | BusinessIntent::SplashOver
        | BusinessIntent::RequestSetup
        | BusinessIntent::LinkReport(_)
        | BusinessIntent::NoCopy(_)
        | BusinessIntent::Mac(_)
        | BusinessIntent::Key { .. }
        | BusinessIntent::Choose(_)
        | BusinessIntent::AskForget(_)
        | BusinessIntent::Scroll { .. }
        | BusinessIntent::Leave
        | BusinessIntent::Answer(_)
        | BusinessIntent::CloseSetup
        | BusinessIntent::SwitchFront
        | BusinessIntent::ScanNow
        | BusinessIntent::SetOpenAccessPoint(_)
        | BusinessIntent::Dismiss => None,
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
