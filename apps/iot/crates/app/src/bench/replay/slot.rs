//! The panel replay as an ordinary system: the composition root registers it only in a bench
//! build, so the schedule never names the bench and the bench never needs the coordinator.

use alloc::boxed::Box;

use crate::runtime::system::{Frame, System};

use super::PanelBench;

pub(crate) struct Slot(Box<PanelBench>);

impl Slot {
    pub(crate) fn new() -> Self {
        Self(Box::default())
    }
}

impl System for Slot {
    fn tick(&mut self, frame: &mut Frame<'_>) {
        self.0.step(frame.manager, frame.bus);
    }
}
