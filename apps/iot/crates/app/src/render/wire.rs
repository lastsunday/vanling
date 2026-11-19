//! Composition: one renderer per wired surface, and the slot roles the state layer addresses
//! them by. Both come from this one pass, so a renderer can never outnumber the consumers it
//! paints.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::render::{AppPainter, LampRenderer, Renderer, Renderers, ScreenRenderer};
use crate::shared::model::{MAX_LIGHTS, SurfaceRole};
use iot_core::drivers::camera::FrameSource;
use iot_core::drivers::surface::WiredSurface;

/// Whether the debug overlay is drawn, and therefore whether its counters earn repaints.
const OVERLAY: bool = true;

/// Turns each board surface into the renderer that draws on it.
pub fn wire(
    surfaces: Vec<WiredSurface>,
    mut camera: Option<Box<dyn FrameSource + Send>>,
) -> (Renderers, [SurfaceRole; MAX_LIGHTS]) {
    assert!(
        surfaces.len() <= MAX_LIGHTS,
        "more wired surfaces than consumers"
    );
    let mut roles = [SurfaceRole::Absent; MAX_LIGHTS];
    for (role, surface) in roles.iter_mut().zip(&surfaces) {
        *role = surface.role();
    }
    let mut renderers = Renderers::new();
    for (instance, surface) in surfaces.into_iter().enumerate() {
        let instance = instance as u8;
        let renderer: Box<dyn Renderer> = match surface {
            WiredSurface::Pages(surface) => {
                let screen = ScreenRenderer::new(
                    instance,
                    surface,
                    Box::new(AppPainter::new(OVERLAY)),
                    OVERLAY,
                );
                match camera.take() {
                    Some(taken) => Box::new(screen.with_camera(taken)),
                    None => Box::new(screen),
                }
            }
            WiredSurface::Lamp(light) => Box::new(LampRenderer::new(instance, light)),
        };
        renderers.register(renderer, 0);
    }
    if camera.is_some() {
        log::info!("[CAM] no surface has a frame buffer, dropping the camera");
        drop(camera);
    }
    (renderers, roles)
}
