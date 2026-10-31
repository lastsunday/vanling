//! Wiring the board's surfaces to the render layer: which renderer gets which handle.

use alloc::boxed::Box;
use alloc::vec::Vec;

use iot_core::drivers::camera::FrameSource;
use iot_core::drivers::surface::WiredSurface;
use iot_core::render::{LampRenderer, Registry, Renderer, ScreenRenderer};
use iot_core::state::{MAX_LIGHTS, SurfaceRole};

pub type Render = Registry;

/// One renderer per wired surface, and the slot roles the state layer addresses them by. Both
/// come from this one pass, so a renderer can never outnumber the slots it paints.
pub fn wired(
    surfaces: Vec<WiredSurface>,
    mut camera: Option<Box<dyn FrameSource>>,
) -> (Registry, [SurfaceRole; MAX_LIGHTS]) {
    assert!(
        surfaces.len() <= MAX_LIGHTS,
        "more wired surfaces than slots"
    );
    let mut roles = [SurfaceRole::Absent; MAX_LIGHTS];
    for (role, surface) in roles.iter_mut().zip(&surfaces) {
        *role = surface.role();
    }
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
    (render, roles)
}
