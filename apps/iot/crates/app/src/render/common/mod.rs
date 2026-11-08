//! What every surface's renderer shares: the diff that says what changed, the appearance it
//! derives, and the colour stepping both a panel and a lamp go through.

mod appearance;
mod color;
mod reconcile;
mod registry;

#[cfg(test)]
pub(crate) use self::appearance::light_diagnostics;
pub use self::appearance::{Appearance, Consumer, LightAppearance, light_appearance};
pub(crate) use self::color::ColorDriver;
pub use self::reconcile::{Activity, RenderController, Renderer};
pub use self::registry::Registry;
