pub(crate) use super::*;
pub(crate) use crate::services::Services;
pub(crate) use crate::shared::intent::{
    BusinessIntent, InputIntent, Intent, LightAction, NavigationIntent,
};
/// Every page the button can reach: the table without its transients, in the button's order.
pub(crate) fn cycled() -> impl Iterator<Item = PageId> {
    PageId::ALL
        .into_iter()
        .filter(|page| page.timeout_ms().is_none())
}

pub(crate) use crate::state::translate::translate;
pub(crate) use iot_core::demand::EffectiveDemand;
pub(crate) use iot_core::drivers::input::{
    ButtonEvent, GestureEvent, InputEvent, MAX_TOUCH_POINTS, SwipeDirection, TouchEvent,
    TouchPoint, TouchStatus,
};
pub(crate) use iot_core::drivers::motion::{
    MotionCapabilities, MotionCounts, MotionSample, MotionScale,
};
pub(crate) use iot_core::drivers::network::{Ipv4, SavedNetworks};

impl DeviceManager {
    pub(crate) fn state(&self) -> DeviceState {
        self.state.clone()
    }

    pub(crate) fn state_mut(&mut self) -> &mut DeviceState {
        &mut self.state
    }

    /// What the Network page publishes, for a test that walks the page's edits.
    pub(crate) fn network_view(&self) -> NetworkView {
        self.services().snapshot().network
    }

    /// A derivation from the held keys, to pin the cache to what the page claims.
    pub(crate) fn fresh_demand(&self) -> EffectiveDemand {
        self.demand.fresh(&self.state)
    }

    pub(crate) fn setup_requested(&self) -> bool {
        self.state.setup_requested
    }

    pub(crate) fn light_state(&self) -> LightState {
        self.state.lights[0]
    }

    /// Clamped like the write side: an out-of-range read is the last surface, never a panic.
    pub(crate) fn light_state_at(&self, instance: u8) -> LightState {
        self.state.lights[usize::from(DeviceState::light_index(instance))]
    }

    pub(crate) fn touch_state(&self) -> Option<TouchEvent> {
        self.state.touch.last
    }

    pub(crate) fn motion_state(&self) -> Option<MotionSample> {
        self.state.motion.sample
    }

    pub(crate) fn motion_counts_state(&self) -> MotionCounts {
        self.state.motion.counts
    }

    /// An absolute target, so a test can start from a mode instead of cycling into it.
    pub(crate) fn apply_to(&mut self, instance: u8, state: LightState) {
        let index = usize::from(DeviceState::light_index(instance));
        self.state.lights[index] = state;
    }
}

pub(crate) const TEST_SCALE: MotionScale = MotionScale::from_ranges(8_192, 64);

pub(crate) const BREATH: LightState = LightState::Breath(Breath {
    period_ms: 3_000,
    hue_period_ms: 60_000,
    hue_span: 255,
    min_brightness: 24,
    max_brightness: 80,
    saturation: 200,
    hue: 0,
    group: [16, 32, 3, 0, 0, 0, 0],
    group_len: 3,
});

pub(crate) fn op(event: InputEvent) -> InputIntent {
    InputIntent { source: 0, event }
}

pub(crate) fn tap() -> InputIntent {
    op(InputEvent::Gesture(GestureEvent::Tap {
        id: 0,
        x: 1,
        y: 2,
        held_ms: 0,
        at_ms: 0,
    }))
}

pub(crate) fn triple_click() -> InputIntent {
    op(InputEvent::Button(ButtonEvent::TripleClick))
}

pub(crate) fn long_press() -> InputIntent {
    op(InputEvent::Gesture(GestureEvent::LongPress {
        id: 0,
        x: 3,
        y: 4,
        held_ms: 0,
    }))
}

pub(crate) fn swipe() -> InputIntent {
    op(InputEvent::Gesture(GestureEvent::Swipe {
        id: 0,
        x: 80,
        y: 30,
        end_x: 80,
        end_y: 60,
        direction: SwipeDirection::Down,
        held_ms: 42,
        distance_px: 73,
    }))
}

pub(crate) fn touch_event(status: TouchStatus) -> InputIntent {
    op(InputEvent::Touch(TouchEvent {
        points: [TouchPoint {
            id: 0,
            x: 1,
            y: 2,
            status,
        }; MAX_TOUCH_POINTS],
        len: 1,
    }))
}

pub(crate) fn press(id: u8, x: u16, y: u16) -> InputIntent {
    op(InputEvent::Gesture(GestureEvent::Press { id, x, y }))
}

pub(crate) fn feed_input(manager: &mut DeviceManager, operation: InputIntent) {
    manager.apply_intent(Intent::Input(operation));
}

pub(crate) fn landed(mut manager: DeviceManager) -> DeviceManager {
    manager.apply_business(BusinessIntent::Navigation(NavigationIntent::SplashOver));
    assert_ne!(manager.state().page(), PageId::Splash);
    manager
}

/// A state with every capability wired, on `page`, with every light at `light`.
pub(crate) fn state_on(page: PageId, light: LightState) -> DeviceState {
    DeviceState {
        lights: [light; MAX_LIGHTS],
        surface_roles: [SurfaceRole::Pages, SurfaceRole::Absent],
        nav: Navigator::new(page),
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

pub(crate) fn state(light: LightState) -> DeviceState {
    state_on(PageId::Ambient, light)
}

/// The base state with one network fact changed.
pub(crate) fn with_network(
    mut state: DeviceState,
    change: impl FnOnce(&mut NetworkState),
) -> DeviceState {
    change(&mut state.network);
    state
}

pub(crate) fn target(event: InputEvent, current: LightState) -> Option<LightAction> {
    target_in(event, &state(current))
}

pub(crate) fn target_in(event: InputEvent, current: &DeviceState) -> Option<LightAction> {
    match translate(&op(event), current, &Services::default()) {
        Some(BusinessIntent::Light { action, .. }) => Some(action),
        _ => None,
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

pub(crate) fn long_press_event() -> InputEvent {
    InputEvent::Button(ButtonEvent::LongPress)
}
