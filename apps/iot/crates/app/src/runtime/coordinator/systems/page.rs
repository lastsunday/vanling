//! The current page's own clock: motion the snapshot cannot carry by itself.

use super::super::pacing::MAX_CATCH_UP_MS;
use crate::runtime::system::System;
use crate::state::DeviceManager;

/// Drives page physics on wall time, not on a fixed step per loop pass.
///
/// A pass that spent 52 ms rasterizing would otherwise advance a glide by 20 ms, so a
/// scroll ran at a third of its speed on exactly the pages that could least afford it.
pub(crate) struct PageSystem {
    last_ms: Option<u32>,
}

impl PageSystem {
    pub(crate) const fn new() -> Self {
        Self { last_ms: None }
    }
}

impl Default for PageSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// The elapsed milliseconds since the previous pass, clamped so one long stall does not
/// replay a scroll's whole flight at once. The first pass has no previous stamp and moves 0.
fn page_step(last_ms: &mut Option<u32>, now_ms: u32) -> u16 {
    match last_ms.replace(now_ms) {
        Some(previous) => now_ms.wrapping_sub(previous).min(MAX_CATCH_UP_MS) as u16,
        None => 0,
    }
}

impl System for PageSystem {
    fn tick(&mut self, manager: &mut DeviceManager, now_ms: u32) {
        manager.tick_pages(page_step(&mut self.last_ms, now_ms));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_clock_follows_wall_time_and_clamps_a_stall() {
        let mut last = None;
        assert_eq!(
            page_step(&mut last, 1_000),
            0,
            "the first pass moves nothing"
        );
        assert_eq!(
            page_step(&mut last, 1_052),
            52,
            "a 52 ms pass advances 52 ms"
        );
        assert_eq!(page_step(&mut last, 5_000), MAX_CATCH_UP_MS as u16);
        assert_eq!(page_step(&mut last, 5_020), 20);
        assert_eq!(
            page_step(&mut last, 5_000),
            MAX_CATCH_UP_MS as u16,
            "a counter that wrapped is a stall, not a negative step"
        );
    }
}
