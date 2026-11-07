//! Placement every page shares: the panel's last column, the tallest text block, and the
//! strip the FPS badge owns.

use iot_core::overlay::{CONTENT_BOTTOM, FONT_H, PANEL_COLUMNS, PANEL_ROWS, overlay_row_top};

pub(crate) const PANEL_RIGHT: isize = PANEL_COLUMNS as isize - 1;

/// The FPS badge's row, at the panel's own bottom.
pub(crate) const FPS_Y: usize = 232;

/// The lowest row a text block may start on and still fit.
pub(crate) const LAST_BLOCK_ROW: usize = 17;

const _: () = assert!(
    FPS_Y + FONT_H <= PANEL_ROWS,
    "the FPS badge runs off the bottom of the panel"
);
const _: () = assert!(
    FPS_Y == CONTENT_BOTTOM,
    "the badge's strip is not the rows core sized the pages' controls against"
);
const _: () = assert!(
    overlay_row_top(LAST_BLOCK_ROW) + FONT_H <= PANEL_ROWS,
    "the tallest text block runs off the bottom of the panel"
);
