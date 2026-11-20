//! The current page's own clock: motion the snapshot cannot carry by itself.

use super::super::pacing::STEP_MS;
use crate::runtime::system::System;
use crate::state::DeviceManager;

pub(crate) struct PageSystem;

impl System for PageSystem {
    fn tick(&mut self, manager: &mut DeviceManager, _now_ms: u32) {
        manager.tick_pages(STEP_MS as u16);
    }
}
