//! Time-driven notices, advanced from the wall clock.

use super::{Frame, System};

pub(crate) struct NoticeSystem;

impl System for NoticeSystem {
    fn tick(&mut self, frame: &mut Frame<'_>) {
        frame.manager.tick_notice(frame.now_ms);
    }
}
