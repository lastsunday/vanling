//! Time-driven notices, advanced from the wall clock.

use crate::runtime::system::System;
use crate::state::DeviceManager;

pub(crate) struct NoticeSystem;

impl System for NoticeSystem {
    fn tick(&mut self, manager: &mut DeviceManager, now_ms: u32) {
        manager.tick_notice(now_ms);
    }
}
