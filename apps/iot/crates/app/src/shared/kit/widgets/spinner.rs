//! Four frames of a spinner; the page advances the phase.

use iot_core::ui::{NodeId, Style, Ui, Widget};

const FRAMES: [&[u8]; 4] = [b"|", b"/", b"-", b"\\"];

#[derive(Clone, Copy, Debug, Default)]
pub struct Spinner {
    phase: u8,
}

impl Spinner {
    pub const fn new(phase: u8) -> Self {
        Self { phase }
    }
}

impl Widget for Spinner {
    fn build(self, ui: &mut Ui<'_, '_>) -> NodeId {
        let frame = FRAMES[usize::from(self.phase % FRAMES.len() as u8)];
        ui.text(frame, Style::new().nowrap())
    }
}
