//! The action strip under the list: one box, or two while setup mode has two verbs.

use iot_core::geometry::ControlRect;
use iot_core::overlay::{CONTENT_BOTTOM, PANEL_COLUMNS};

use super::TOUCH_MIN;

/// The action strip under the list: the page's one control, one touch target tall.
pub const TOOLBAR_H: usize = TOUCH_MIN;
/// Top edge of the action strip, in panel rows.
pub const TOOLBAR_TOP: usize = CONTENT_BOTTOM - TOOLBAR_H;

/// The action strip's own box: the page's whole width, so the one control on it is as wide
/// as a thumb can want, and one touch target tall.
pub fn toolbar_box() -> ControlRect {
    ControlRect {
        x: 0,
        y: TOOLBAR_TOP,
        w: PANEL_COLUMNS,
        h: TOOLBAR_H,
    }
}

/// Which cell of the action strip a point is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ToolbarCell {
    #[default]
    Left,
    Right,
}

/// How many cells the strip has while it is split: the way back, and the host switch.
pub const TOOLBAR_CELLS: usize = 2;

/// The seam between two cells, taken out of the cells rather than out of the slots, so the
/// slots stay an exact division of the panel and only the ink moves.
pub const TOOLBAR_GAP: usize = 8;

const _: () = assert!(
    PANEL_COLUMNS.is_multiple_of(TOOLBAR_CELLS),
    "the strip's cells are an exact division of the panel"
);
const _: () = assert!(
    PANEL_COLUMNS / TOOLBAR_CELLS - TOOLBAR_GAP >= TOUCH_MIN,
    "a cell of the strip is smaller than a finger"
);

/// The box one cell of the split strip is drawn in.
pub const fn toolbar_cell(cell: ToolbarCell) -> ControlRect {
    let index = match cell {
        ToolbarCell::Left => 0,
        ToolbarCell::Right => 1,
    };
    let slot = PANEL_COLUMNS / TOOLBAR_CELLS;
    ControlRect {
        x: index * slot + TOOLBAR_GAP / 2,
        y: TOOLBAR_TOP,
        w: slot - TOOLBAR_GAP,
        h: TOOLBAR_H,
    }
}

/// Whether a tap at panel row `y` lands on the action strip.
pub fn toolbar_at(y: u16) -> bool {
    (TOOLBAR_TOP..CONTENT_BOTTOM).contains(&usize::from(y))
}

/// Which cell of the strip a point is on, if it is on the strip at all.
///
/// The seam counts as both halves' edge rather than as a dead gap: a finger aiming at two 152 px
/// cells does not have to miss an 8 px seam, and the two verbs the split strip carries are never
/// both destructive. A strip with one verb is still one box, and either cell means it.
pub fn toolbar_cell_at(x: u16, y: u16) -> Option<ToolbarCell> {
    if !toolbar_at(y) {
        return None;
    }
    Some(if usize::from(x) < PANEL_COLUMNS / TOOLBAR_CELLS {
        ToolbarCell::Left
    } else {
        ToolbarCell::Right
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_action_strip_is_one_target_under_the_list_and_the_page_ends_where_it_does() {
        let strip = toolbar_box();
        assert_eq!(strip.y, TOOLBAR_TOP);
        assert_eq!(strip.y + strip.h, CONTENT_BOTTOM);
        assert_eq!(strip.w, PANEL_COLUMNS);
        for y in TOOLBAR_TOP..CONTENT_BOTTOM {
            assert!(toolbar_at(y as u16), "row {y} is the strip's");
        }
        assert!(!toolbar_at(TOOLBAR_TOP as u16 - 1));
        assert!(!toolbar_at(CONTENT_BOTTOM as u16));
    }

    #[test]
    fn a_split_strip_is_two_cells_of_one_width_that_meet_at_the_middle() {
        let left = toolbar_cell(ToolbarCell::Left);
        let right = toolbar_cell(ToolbarCell::Right);
        assert_eq!(left.w, right.w, "two cells of one strip are one width");
        assert_eq!(left.y, right.y);
        assert_eq!(left.y, TOOLBAR_TOP);
        assert_eq!(left.h, TOOLBAR_H);
        assert_eq!(left.x, TOOLBAR_GAP / 2, "the seam is outside the left cell");
        assert!(right.x + right.w + TOOLBAR_GAP / 2 == PANEL_COLUMNS);
        assert!(
            left.x + left.w <= right.x,
            "the cells do not overlap, whatever the seam does"
        );
        assert!(left.w >= TOUCH_MIN, "each cell is still a finger's own");

        // Every point of the strip is one cell or the other, split at the panel's middle: an 8 px
        // seam is not a gap a thumb is expected to miss, and the two verbs the split strip carries
        // are never both destructive.
        for (x, cell) in [
            (0, ToolbarCell::Left),
            (PANEL_COLUMNS / 2 - 1, ToolbarCell::Left),
            (PANEL_COLUMNS / 2, ToolbarCell::Right),
            (PANEL_COLUMNS - 1, ToolbarCell::Right),
        ] {
            assert_eq!(
                toolbar_cell_at(x as u16, (TOOLBAR_TOP + TOOLBAR_H / 2) as u16),
                Some(cell),
                "column {x}"
            );
        }
        assert_eq!(toolbar_cell_at(0, TOOLBAR_TOP as u16 - 1), None);
        assert_eq!(toolbar_cell_at(0, CONTENT_BOTTOM as u16), None);
    }
}
