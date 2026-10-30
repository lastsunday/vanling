//! The microphone window the capture path writes and the pipeline reads.

use core::future::Future;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use iot_core::drivers::audio::{AudioSample, CaptureSink};

/// Off the intent ring: at 1.7 KB the window would set every ring slot that wide.
pub struct CaptureWindow(Channel<CriticalSectionRawMutex, AudioSample, 1>);

impl CaptureWindow {
    pub fn take(&self) -> Option<AudioSample> {
        self.0.try_receive().ok()
    }

    pub fn ready(&self) -> impl Future<Output = ()> {
        self.0.ready_to_receive()
    }
}

impl CaptureSink for CaptureWindow {
    fn publish(&self, sample: AudioSample) {
        let _ = self.0.try_send(sample);
    }
}

pub static CAPTURE: CaptureWindow = CaptureWindow(Channel::new());

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
