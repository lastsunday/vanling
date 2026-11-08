//! The render policy that follows from being a page: what it grounds the panel with, whether
//! frames arrive from a camera, and how often diagnostics may repaint it.
//!
//! None of this is state. It is what the render layer decides when it is handed a page, so it
//! sits with the renderer rather than in the page table the model owns.

use crate::shared::route::Page;
use iot_core::paint::Rgb;

/// The least wall time between diagnostics-driven repaints. A repaint ships the whole 320×240
/// frame down one blocking SPI transfer (on the order of 15 ms), so without a bound the live
/// pages hold the shared cooperative executor in a ~15 ms block almost continuously, starving
/// the playback feed and the capture poll past their ring runway. A page raises it for itself
/// here.
pub const DIAG_REPAINT_STEP_MS: u64 = 40;

/// The fill this page covers the panel with before drawing, or `None` when it draws over what
/// the light was already showing.
pub fn ground(page: Page) -> Option<Rgb> {
    match page {
        Page::Camera | Page::Network => Some(Rgb(0, 0, 0)),
        Page::Ambient | Page::Attitude | Page::Audio | Page::Speaker | Page::Splash => None,
    }
}

/// Whether this page's frames arrive from the camera instead of being drawn.
pub fn draws_frames(page: Page) -> bool {
    matches!(page, Page::Camera)
}

/// The least wall time this page waits between diagnostics-driven repaints. The Audio page
/// raises it: a capture poll closes two envelope columns every 20 ms, so the default step
/// already redraws a 200-column sweep every ~45 ms and this page alone held the executor 40%
/// of the wall time on the S3. At twice the step a frame advances the sweep eight columns
/// instead of four — still a sweep, at half the cost.
pub fn repaint_step_ms(page: Page) -> u64 {
    match page {
        Page::Audio => DIAG_REPAINT_STEP_MS * 2,
        Page::Ambient
        | Page::Attitude
        | Page::Speaker
        | Page::Camera
        | Page::Network
        | Page::Splash => DIAG_REPAINT_STEP_MS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_answers_every_policy_question() {
        for page in Page::ALL {
            let ground = ground(page);
            assert_eq!(
                ground,
                matches!(page, Page::Camera | Page::Network).then_some(Rgb(0, 0, 0)),
                "{page:?} grounds"
            );
            assert_eq!(draws_frames(page), page == Page::Camera, "{page:?} frames");
            assert!(
                repaint_step_ms(page) >= DIAG_REPAINT_STEP_MS,
                "{page:?} cannot repaint faster than the floor"
            );
            assert_eq!(
                repaint_step_ms(page) > DIAG_REPAINT_STEP_MS,
                page == Page::Audio,
                "{page:?} step"
            );
        }
    }
}
