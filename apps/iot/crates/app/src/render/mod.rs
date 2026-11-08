//! The render layer: what the device's surfaces should show, the registry that drives the
//! renderers which show it, and the painter that turns a snapshot into a page.

mod appearance;
mod color;
mod lamp;
mod painter;
mod policy;
mod reconcile;
mod registry;
mod repaint;
mod screen;
mod wiring;

pub use self::appearance::{Appearance, Consumer, LightAppearance, light_appearance};
pub use self::lamp::LampRenderer;
pub use self::painter::AppPainter;
pub use self::reconcile::{Activity, RenderController, Renderer};
pub use self::registry::Registry;
pub use self::repaint::{diag_repaint_due, windowed_percent, windowed_rate};
pub use self::screen::ScreenRenderer;
pub use self::wiring::{Renderers, wired};

pub use self::policy::DIAG_REPAINT_STEP_MS;
pub use iot_core::paint::{MODE_BREATH, MODE_OFF, MODE_SOLID};

#[cfg(test)]
mod test_support;
