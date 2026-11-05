//! The microphone window the board folded, into state.

use super::{Frame, System};

/// Stateless: the newest window lives in the handle, not here.
pub(crate) struct CaptureSystem;

impl System for CaptureSystem {
    fn tick(&mut self, frame: &mut Frame<'_>) {
        if let Some(sample) = frame.workers.input.from.take() {
            frame.manager.apply_capture(sample);
        }
    }
}
