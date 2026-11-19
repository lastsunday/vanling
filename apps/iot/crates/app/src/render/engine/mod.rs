//! What every renderer shares: the diff that says what moved, the appearance it derives, and
//! the colour stepping both a screen and a lamp go through.

mod appearance;
mod color;
mod projection;
mod reconciler;
mod renderer;
mod renderers;

pub use self::appearance::{Appearance, LightAppearance, Topic};
pub(crate) use self::color::ColorDriver;
#[cfg(test)]
pub(crate) use self::projection::light_appearance;
#[cfg(test)]
pub(crate) use self::reconciler::RenderController;
pub use self::renderer::{Activity, Renderer};
pub use self::renderers::Renderers;

#[cfg(test)]
mod test_support;
