//! Wiring the board's surfaces to the render layer: which renderer gets the camera.
//!
//! The renderer itself and the registry live in `iot-core::render`; this module only knows
//! the board's handles.

use alloc::boxed::Box;
use alloc::vec::Vec;

use iot_core::diagnostics::DiagnosticsSink;
use iot_core::drivers::camera::{CameraTarget, FrameSource};
use iot_core::drivers::light::RgbLight;
use iot_core::render::{LightRenderer, Registry};

pub type Render = Registry;

/// One renderer per wired surface. The camera goes to the surface that says it owns a frame
/// buffer; a board whose surfaces all decline gets it dropped rather than silently attached
/// to a light that cannot paint it.
pub fn wired<L>(lights: Vec<L>, mut camera: Option<Box<dyn FrameSource>>) -> Registry
where
    L: RgbLight + DiagnosticsSink + CameraTarget + 'static,
{
    let mut render = Registry::new();
    for (instance, light) in lights.into_iter().enumerate() {
        let takes_frames = light.takes_frames();
        let renderer = LightRenderer::new(instance as u8, light);
        let renderer = if takes_frames {
            match camera.take() {
                Some(taken) => renderer.with_camera(taken),
                None => renderer,
            }
        } else {
            renderer
        };
        render.register(Box::new(renderer), 0);
    }
    if camera.is_some() {
        log::info!("[CAM] no surface has a frame buffer, dropping the camera");
        drop(camera);
    }
    render
}
