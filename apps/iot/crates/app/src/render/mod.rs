//! The render layer: what the device's surfaces should show, grouped by the surface that shows
//! it. `common` holds what every surface shares, `screen` the panel, `lamp` the light; `wiring`
//! is the one place that knows all of them.

mod common;
mod lamp;
mod screen;
mod wiring;

pub use self::common::{
    Activity, Appearance, Consumer, LightAppearance, Registry, RenderController, Renderer,
    light_appearance,
};
pub use self::lamp::LampRenderer;
pub use self::screen::{
    AppPainter, DIAG_REPAINT_STEP_MS, ScreenRenderer, diag_repaint_due, windowed_percent,
    windowed_rate,
};
pub use self::wiring::{Renderers, wired};

pub use iot_core::paint::{MODE_BREATH, MODE_OFF, MODE_SOLID};

#[cfg(test)]
mod test_support;
