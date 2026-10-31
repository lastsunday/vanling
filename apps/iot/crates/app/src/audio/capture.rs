//! The capture path's half of the microphone window: the driver publishes into it.

use iot_core::drivers::audio::{AudioSample, CaptureSink};

use crate::workers::CaptureWindow;

impl CaptureSink for CaptureWindow {
    fn publish(&self, sample: AudioSample) {
        self.push(sample);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::INTENT_SLOTS;
    use iot_core::intent::Intent;

    #[test]
    fn a_capture_window_costs_one_slot_beside_the_ring() {
        let ring = core::mem::size_of::<Intent>() * INTENT_SLOTS;
        let window = core::mem::size_of::<AudioSample>();
        assert!(
            ring < window,
            "a capture window is sizing the intent ring again: {ring} B of ring against a \
             {window} B window"
        );
        assert!(
            core::mem::size_of::<CaptureWindow>() < 2 * window,
            "the capture window holds one window, not a queue of them"
        );
    }
}
