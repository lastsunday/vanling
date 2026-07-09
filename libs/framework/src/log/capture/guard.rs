use std::sync::Arc;

use super::Capture;

pub struct Guard {
    pub(super) capture: Arc<Capture>,
}

impl Drop for Guard {
    #[inline]
    fn drop(&mut self) {
        self.capture.stop();
    }
}
