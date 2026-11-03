//! The input path's tests, one file per piece of it: the button scanner, the tracker that keeps ids on fingers, and the classifier that turns samples into gestures.

use super::*;
use crate::demand::DemandKey;

/// A stand-in's pressed flag is atomic rather than a `Cell`: `InputSource` carries `Send`,
/// so a double that could not cross a core could not stand in for a driver either.
struct Pressed(core::sync::atomic::AtomicBool);

impl Pressed {
    const fn new(pressed: bool) -> Self {
        Self(core::sync::atomic::AtomicBool::new(pressed))
    }

    fn get(&self) -> bool {
        self.0.load(core::sync::atomic::Ordering::Relaxed)
    }

    fn set(&self, pressed: bool) {
        self.0.store(pressed, core::sync::atomic::Ordering::Relaxed);
    }
}

mod button;
mod classifier;
mod tracker;
