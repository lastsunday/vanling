//! The render layer: what the device's surfaces should show, grouped by the surface that
//! shows it. `engine` holds what every surface shares, `screen` the page screen, `lamp` the
//! light; `wire` is the one place that knows all of them.

mod engine;
mod lamp;
mod screen;
mod wire;

pub use self::engine::{Activity, Appearance, LightAppearance, Renderer, Renderers, Topic};
#[cfg(test)]
pub(crate) use self::engine::{RenderController, light_appearance};
pub use self::lamp::LampRenderer;
pub use self::screen::{AppPainter, ScreenRenderer};
pub use self::wire::wire;
