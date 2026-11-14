//! The microphone window the board folded, into state.

use crate::runtime::system::System;
use crate::runtime::workers::CaptureWindow;
use crate::state::DeviceManager;

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
    fn tick(&mut self, manager: &mut DeviceManager, _now_ms: u32) {
        if let Some(sample) = self.from.take() {
            manager.apply_capture(sample);
        }
    }
}
