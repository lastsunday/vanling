//! Wiring the board's surfaces to the render layer: which renderer gets which handle.

use alloc::boxed::Box;
use alloc::vec::Vec;

use iot_core::drivers::camera::FrameSource;
use iot_core::drivers::surface::WiredSurface;
use iot_core::render::{LampRenderer, Registry, Renderer, ScreenRenderer};

pub type Render = Registry;

/// One renderer per wired surface. A panel takes the camera, because a panel is the only
/// surface with the frame buffer a camera writes into; a board with no panel drops it.
pub fn wired(surfaces: Vec<WiredSurface>, mut camera: Option<Box<dyn FrameSource>>) -> Registry {
    let mut render = Registry::new();
    for (instance, surface) in surfaces.into_iter().enumerate() {
        let instance = instance as u8;
        let renderer: Box<dyn Renderer> = match surface {
            WiredSurface::Pages(surface) => {
                let panel = ScreenRenderer::new(instance, surface);
                match camera.take() {
                    Some(taken) => Box::new(panel.with_camera(taken)),
                    None => Box::new(panel),
                }
            }
            WiredSurface::Lamp(light) => Box::new(LampRenderer::new(instance, light)),
        };
        render.register(renderer, 0);
    }
    if camera.is_some() {
        log::info!("[CAM] no surface has a frame buffer, dropping the camera");
        drop(camera);
    }
    render
}
