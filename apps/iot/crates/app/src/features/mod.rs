//! The product's pages, one vertical slice each: the page's model, controller, layout and
//! view live together, and the directory is the unit of cohesion.
//!
//! [`paint`] is the one page-to-view dispatch. Params are the page and a surface already
//! built by the renderer; the feature modules answer with the page's own ink.

pub mod ambient;
pub mod attitude;
pub mod audio;
pub mod camera;
pub mod network;
pub mod speaker;
pub mod splash;

use crate::shared::route::DisplayPage;
use crate::shared::view_kit::surface::Surface;

/// Draws `page` into `surface` and stamps the debug badge every page but the camera carries.
pub fn paint(page: DisplayPage, surface: &mut Surface<'_>) {
    match page {
        DisplayPage::Splash => surface.stamp_splash(),
        DisplayPage::Attitude => surface.stamp_attitude(),
        DisplayPage::Audio => surface.stamp_audio(),
        DisplayPage::Speaker => surface.stamp_speaker(),
        DisplayPage::Camera => {
            surface.stamp_camera(iot_core::drivers::camera::CameraSnapshot::STARTING)
        }
        DisplayPage::Network => surface.stamp_network(),
        DisplayPage::Ambient if surface.debug => surface.stamp_ambient(),
        DisplayPage::Ambient => {}
    }
    if surface.debug && page != DisplayPage::Camera {
        surface.stamp_fps();
    }
}

#[cfg(test)]
mod tests;
