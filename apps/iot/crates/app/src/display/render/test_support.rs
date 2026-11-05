//! Fixtures shared by the render layer's tests.

use crate::display::diagnostics::Diagnostics;
use crate::display::render::appearance::light_diagnostics;
use crate::display::render::{Appearance, LightAppearance, RenderController};
use crate::model::intent::{ActionIntent, PageIntent};
use crate::model::state::{DeviceManager, DeviceState, LightState, MAX_LIGHTS};
use iot_core::demand::EffectiveDemand;

pub(super) const BOOT_BREATH: crate::model::state::Breath = crate::model::state::Breath {
    period_ms: 3_000,
    hue_period_ms: 60_000,
    hue_span: 255,
    min_brightness: 24,
    max_brightness: 80,
    saturation: 200,
    hue: 0,
    group: [16, 32, 3, 0, 0, 0, 0],
    group_len: 3,
};

pub(super) fn state(light: LightState) -> DeviceState {
    DeviceState {
        lights: [light; MAX_LIGHTS],
        ..DeviceState::default()
    }
}

pub(super) fn collect(ctl: &mut RenderController, s: &DeviceState) -> Vec<Appearance> {
    let mut seen = Vec::new();
    ctl.reconcile(s, &EffectiveDemand::default(), |a| seen.push(a));
    seen
}

pub(super) fn landed() -> DeviceManager {
    let mut manager = DeviceManager::new();
    manager.apply_action(ActionIntent::Page(PageIntent::SplashOver));
    manager
}

pub(super) fn diagnostics_for(light: LightState) -> Diagnostics {
    light_diagnostics(&state(light), &EffectiveDemand::default())
}

pub(super) fn light_appearances(seen: Vec<Appearance>) -> Vec<(u8, LightAppearance)> {
    seen.into_iter()
        .filter_map(|appearance| match appearance {
            Appearance::Surface {
                instance,
                appearance,
            } => Some((instance, appearance)),
            Appearance::Diagnostics(_) => None,
        })
        .collect()
}
