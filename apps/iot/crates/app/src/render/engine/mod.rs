//! What every renderer shares: the diff that says what moved, the appearance it derives, and
//! the colour stepping both a screen and a lamp go through.

mod appearance;
mod color;
mod differ;
mod projection;
mod renderer;
mod set;

pub use self::appearance::{Appearance, LightAppearance, Topic};
pub(crate) use self::color::{ColorOutput, Shown};
#[cfg(test)]
pub(crate) use self::differ::Differ;
#[cfg(test)]
pub(crate) use self::projection::light_appearance;
pub use self::renderer::{Drive, Renderer};
pub use self::set::RenderSet;

#[cfg(test)]
mod test_support;
