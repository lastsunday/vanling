pub(crate) use super::*;
pub(crate) use crate::shared::intent::{BusinessIntent, InputIntent, Intent, NavigationIntent};
pub(crate) use crate::shared::route::CYCLE;
pub(crate) use crate::state::translate::translate;
pub(crate) use iot_core::demand::EffectiveDemand;
pub(crate) use iot_core::drivers::input::{
    ButtonEvent, GestureEvent, InputEvent, MAX_TOUCH_POINTS, SwipeDirection, TouchEvent,
    TouchPoint, TouchStatus,
};
pub(crate) use iot_core::drivers::motion::{MotionCounts, MotionSample, MotionScale};
pub(crate) use iot_core::drivers::network::{Ipv4, SavedNetworks};

impl DeviceManager {
    pub(crate) fn state(&self) -> DeviceState {
        self.state.clone()
    }

    pub(crate) fn state_mut(&mut self) -> &mut DeviceState {
        &mut self.state
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
        self.state.lights[usize::from(instance).min(MAX_LIGHTS - 1)]
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
    pub(crate) fn apply_to(&mut self, instance: usize, state: LightState) {
        self.state.lights[instance.min(MAX_LIGHTS - 1)] = state;
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
    assert_ne!(manager.state().page(), Page::Splash);
    manager
}
