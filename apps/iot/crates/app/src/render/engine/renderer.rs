//! What a renderer is, and how it asks to be driven.

use alloc::boxed::Box;
use core::future::Future;
use core::pin::Pin;

use super::appearance::{Appearance, Topic};

/// How a renderer wants to be driven after an appearance sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drive {
    Idle,
    TimeDriven,
}

/// Pluggable binding between one device and the render layer: the controller decides *what*
/// to show (central state diff), the renderer answers *how*, including whether it needs
/// time-driven frames.
pub trait Renderer {
    /// The appearances this renderer claims. It is never handed one it did not ask for.
    fn topics(&self) -> &[Topic];

    fn on_appearance(&mut self, appearance: Appearance<'_>, now_ms: u32) -> Drive;

    fn step(&mut self, now_ms: u32) -> Drive;

    /// Ships whatever the last appearance or step left buffered, if anything. A renderer that
    /// owns no bus, or has nothing pending, answers `None`; the screen's transfer is awaited
    /// here so the executor keeps running between its DMA chunks.
    fn flush<'a>(&'a mut self) -> Option<Pin<Box<dyn Future<Output = ()> + 'a>>> {
        None
    }
}
