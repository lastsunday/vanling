//! Fixtures shared by the render layer's tests.

use crate::render::{Appearance, Differ, LightAppearance};
use crate::shared::intent::{BusinessIntent, NavigationIntent};
use crate::shared::model::{DeviceState, LightState, MAX_LIGHTS};
use crate::shared::snapshot::{FrameSnapshot, PageSnapshot};
use crate::state::DeviceManager;
use iot_core::demand::{EffectiveDemand, Mode};

use super::projection::frame_snapshot;

pub(super) const BOOT_BREATH: crate::state::Breath = match LightState::boot() {
    LightState::Breath(breath) => breath,
    _ => panic!("the boot light breathes"),
};

/// An appearance a test keeps: the frame is copied out, because a tick only lends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Seen {
    Surface {
        instance: u8,
        light: LightAppearance,
    },
    Frame(FrameSnapshot),
    Demand(Mode),
}

impl Seen {
    pub(super) fn of(appearance: Appearance<'_>) -> Self {
        match appearance {
            Appearance::Surface { instance, light } => Self::Surface { instance, light },
            Appearance::Frame(frame) => Self::Frame(*frame),
            Appearance::Demand(mode) => Self::Demand(mode),
        }
    }
}

pub(super) fn state(light: LightState) -> DeviceState {
    DeviceState {
        lights: [light; MAX_LIGHTS],
        ..DeviceState::default()
    }
}

pub(super) fn collect(ctl: &mut Differ, s: &DeviceState) -> Vec<Seen> {
    let mut seen = Vec::new();
    ctl.diff(
        s,
        PageSnapshot::default(),
        &EffectiveDemand::default(),
        |appearance| seen.push(Seen::of(appearance)),
    );
    seen
}

pub(super) fn landed() -> DeviceManager {
    let mut manager = DeviceManager::new();
    manager.apply_business(BusinessIntent::Navigation(NavigationIntent::SplashOver));
    manager
}

pub(super) fn frame_for(light: LightState) -> FrameSnapshot {
    frame_snapshot(
        &state(light),
        PageSnapshot::default(),
        &EffectiveDemand::default(),
    )
}

pub(super) fn light_appearances(seen: Vec<Seen>) -> Vec<(u8, LightAppearance)> {
    seen.into_iter()
        .filter_map(|appearance| match appearance {
            Seen::Surface { instance, light } => Some((instance, light)),
            Seen::Frame(_) | Seen::Demand(_) => None,
        })
        .collect()
}
