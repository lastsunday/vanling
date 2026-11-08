//! The product's pages, one vertical slice each: the page's model, controller, layout and
//! view live together, and the directory is the unit of cohesion.
//!
//! [`paint`] is the one page-to-view dispatch. Params are the page and a surface already
//! built by the renderer; the page modules answer with the page's own ink.

pub mod ambient;
pub mod attitude;
pub mod audio;
pub mod camera;
pub mod network;
pub mod speaker;
pub mod splash;

use crate::shared::route::Page;
use crate::shared::view_kit::surface::Surface;

/// Draws `page` into `surface` and stamps the debug badge every page but the camera carries.
pub fn paint(page: Page, surface: &mut Surface<'_>) {
    match page {
        Page::Splash => surface.stamp_splash(),
        Page::Attitude => surface.stamp_attitude(),
        Page::Audio => surface.stamp_audio(),
        Page::Speaker => surface.stamp_speaker(),
        Page::Camera => surface.stamp_camera(iot_core::drivers::camera::CameraSnapshot::STARTING),
        Page::Network => surface.stamp_network(),
        Page::Ambient if surface.debug => surface.stamp_ambient(),
        Page::Ambient => {}
    }
    if surface.debug && page != Page::Camera {
        surface.stamp_fps();
    }
}

#[cfg(test)]
mod tests;
