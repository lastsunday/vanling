//! The panel replay as an ordinary system: the composition root registers it only in a bench
//! build, so the schedule never names the bench and the bench never needs the coordinator.

use alloc::boxed::Box;

use crate::runtime::feature::input::IntentBus;
use crate::runtime::system::System;
use crate::state::DeviceManager;

use super::PanelBench;

pub(crate) struct Slot {
    bench: Box<PanelBench>,
    bus: &'static IntentBus,
}

impl Slot {
    pub(crate) fn new(bus: &'static IntentBus) -> Self {
        Self {
            bench: Box::default(),
            bus,
        }
    }
}

impl System for Slot {
    fn tick(&mut self, manager: &mut DeviceManager, _now_ms: u32) {
        self.bench.step(manager, self.bus);
    }
}
