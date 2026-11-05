//! The microphone window the board folded, into state.

use crate::runtime::system::{Frame, System};
use crate::runtime::workers::CaptureWindow;

/// Stateless: the newest window lives in the handle, not here.
pub(crate) struct CaptureSystem {
    from: &'static CaptureWindow,
}

impl CaptureSystem {
    pub(crate) fn new(from: &'static CaptureWindow) -> Self {
        Self { from }
    }
}

impl System for CaptureSystem {
    fn tick(&mut self, frame: &mut Frame<'_>) {
        if let Some(sample) = self.from.take() {
            frame.manager.apply_capture(sample);
        }
    }
}
