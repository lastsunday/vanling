//! The render layer: what the device's surfaces should show, and the registry that drives
//! the renderers which show it.

mod appearance;
mod color;
mod lamp;
mod policy;
mod reconcile;
mod registry;
mod repaint;
mod screen;

pub use self::appearance::{Appearance, Consumer, LightAppearance, light_appearance};
pub use self::lamp::LampRenderer;
pub use self::reconcile::{Activity, RenderController, Renderer};
pub use self::registry::Registry;
pub use self::repaint::{diag_repaint_due, windowed_percent, windowed_rate};
pub use self::screen::ScreenRenderer;

pub use self::policy::DIAG_REPAINT_STEP_MS;
pub use iot_core::paint::{MODE_BREATH, MODE_OFF, MODE_SOLID};

#[cfg(test)]
mod test_support;
