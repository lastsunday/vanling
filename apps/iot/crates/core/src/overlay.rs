//! Where the on-panel diagnostics overlay puts each block of text.
//!
//! Here rather than beside the panel because the panel is only reachable on the
//! device, and a column that moved is a bug that reads as a design decision: a unit
//! nudged left by a digit is invisible as a defect and obvious as a wrong answer.
//! Every block is a fixed distance along a fixed pitch, so a readout that changes
//! width cannot drag the unit beside it out from under the row above.
//!
//! The panel is addressed 320 columns by 240 rows, and the layout follows from that:
//! wide enough for three text blocks across, short enough that a block cannot hold
//! twenty rows at this pitch. The tests below pin both against that geometry, which
//! is the whole reason the numbers live here and not in the panel driver.

/// The panel the layout below is built for, in panel columns/rows.
///
/// Named here because every number in this module is a distance on that panel,
/// and a panel that changed shape is a reason to re-read all of them. It is not
/// a board fact core may rely on: the board owns what it mounted, and its own
/// constants are asserted against these two at the wiring point, so a board that
/// mounts a different panel cannot compile against a layout meant for this one.
pub const PANEL_COLUMNS: usize = 320;
pub const PANEL_ROWS: usize = 240;

/// Width of one glyph cell. The pitch every column is placed on is this plus
/// [`OVERLAY_GAP`], so a column is an index into a rhythm rather than a number
/// someone measured off a photograph.
pub const FONT_W: usize = 5;
/// Horizontal gap between glyph columns: the blank column that keeps two words
/// from reading as one.
pub const OVERLAY_GAP: usize = 4;
/// Distance between the left edges of adjacent glyph columns.
pub const GLYPH_PITCH: usize = FONT_W + OVERLAY_GAP;

/// Rows one glyph cell occupies, matching the 5x7 font the panel draws with.
pub const FONT_H: usize = 7;
/// Vertical gap between diagnostics rows.
pub const OVERLAY_ROW_GAP: usize = 6;
/// Distance between the top edges of adjacent diagnostics rows.
pub const ROW_PITCH: usize = FONT_H + OVERLAY_ROW_GAP;
/// Top edge of the first diagnostics row, in panel rows.
pub const OVERLAY_Y: usize = 8;

/// The top edge of `row`, in panel rows. Every row is placed through this rather
/// than by arithmetic at the call site, so the vertical rhythm is one number.
pub const fn overlay_row_top(row: usize) -> usize {
    OVERLAY_Y + row * ROW_PITCH
}

/// Left edge of the overlay, in panel columns.
pub const OVERLAY_X: usize = 10;

/// Label slot of a block, in glyph columns: a three-glyph label plus one blank
/// cell, so the reading beside it starts on one vertical line in every block.
pub const BLOCK_LABEL_GLYPHS: usize = 4;

/// Distance between the left edges of adjacent text blocks. Sized by the widest
/// reading any block takes — seven glyphs, a coordinate pair written `x y` — plus
/// the blank cell that keeps one block's last digit from reading as the next
/// block's first letter. A fourth block would not fit beside these three, which is
/// why the Audio page's narrow block is pinned to the panel edge instead.
pub const BLOCK_PITCH: usize = 104;

/// Reading column of a block, measured from the block's left edge.
pub const BLOCK_VALUE_OFFSET: usize = BLOCK_LABEL_GLYPHS * GLYPH_PITCH;

/// Left edge of the narrow block pinned to the panel's right edge, in panel
/// columns. For a page whose graphics own the left columns — the Audio sweep —
/// this is the only width the rest of the panel leaves, and it is set by the
/// sweep's right-hand column rather than by the block rhythm above.
pub const RIGHT_EDGE_X: usize = 242;

/// Where a page writes one of its readout rows.
///
/// A row names its block rather than an x coordinate, because the blocks are
/// what a page's layout is made of: a column that moves has to move once, for
/// every row in it, and the only way to guarantee that is for nothing to know
/// where the column is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    /// The leftmost block: the page's title and its primary readout.
    First,
    /// The middle block, for a page whose rows outgrow one column.
    Second,
    /// The third block, and the width the attitude dial is drawn into.
    Third,
    /// The narrow block against the panel's right edge.
    RightEdge,
}

/// The block's left edge, in panel columns.
pub const fn block_x(block: Block) -> usize {
    match block {
        Block::First => OVERLAY_X,
        Block::Second => OVERLAY_X + BLOCK_PITCH,
        Block::Third => OVERLAY_X + 2 * BLOCK_PITCH,
        Block::RightEdge => RIGHT_EDGE_X,
    }
}

/// The block's reading column, in panel columns.
pub const fn block_value_x(block: Block) -> usize {
    block_x(block) + BLOCK_VALUE_OFFSET
}

/// Value column of the Audio level rows: a 3-glyph label slot plus one
/// space glyph between label and value.
pub const LEFT_VALUE_X: usize = block_value_x(Block::First);

/// Unit column of the Audio level rows, a [`LEVEL_VALUE_GLYPHS`]-glyph value plus a
/// whole blank cell after [`LEFT_VALUE_X`]. Fixed rather than flush right so a level
/// growing a digit cannot walk its unit out from under the row above.
pub const LEVEL_UNIT_X: usize = LEFT_VALUE_X + 4 * GLYPH_PITCH;

/// Value column of the sound pressure level beside it, and the column its own unit label
/// sits in. A [`LEVEL_UNIT_GLYPHS`]-glyph unit plus a whole blank cell, because `DBFS` is
/// four glyphs wide and one glyph further right than its own left edge still leaves the
/// two columns touching.
pub const LEVEL_SPL_X: usize = LEVEL_UNIT_X + (LEVEL_UNIT_GLYPHS + 1) * GLYPH_PITCH;
pub const LEVEL_SPL_UNIT_X: usize = LEVEL_SPL_X + 4 * GLYPH_PITCH;

/// Glyphs a level *unit* label occupies. `DBFS` is the widest of them; `SPL` and
/// `CLIP` are both three, and the level column is shared with the clip latch.
pub const LEVEL_UNIT_GLYPHS: usize = 4;

/// Glyphs a level value is allowed to occupy. Three, because that is the widest
/// either reading gets — a dBFS reading is a sign and two digits, and a pressure
/// level of this microphone's is two or three — and reserving more would spend
/// panel columns on widths no reading can use.
pub const LEVEL_VALUE_GLYPHS: usize = 3;

/// Glyphs a reading in a block on the block rhythm is allowed to occupy. Seven is
/// the widest any readout gets — a coordinate pair written `319 239` — and
/// [`BLOCK_PITCH`] is sized on it.
pub const BLOCK_VALUE_GLYPHS: usize = 7;

/// Glyphs the narrow right-edge block has room for. Five, and not by choice: the
/// Audio sweep owns the columns to its left, so this block is what is left over,
/// and a reading wider than this one has to be written in a full block instead.
pub const RIGHT_EDGE_VALUE_GLYPHS: usize = 5;

/// How wide a reading in `block` may be before it reaches the panel edge or the
/// next block. A page asks this rather than counting, because the reservation and
/// the pitch that follows from it are the same decision.
pub const fn block_value_glyphs(block: Block) -> usize {
    match block {
        Block::RightEdge => RIGHT_EDGE_VALUE_GLYPHS,
        _ => BLOCK_VALUE_GLYPHS,
    }
}

/// Where one level row's text sits, as the left edge of each of its four
/// columns. The value and its unit are one gap apart, so the unit is a column
/// the eye can learn and the reading beside it is free to change width.
pub struct LevelColumns {
    pub value_x: usize,
    pub unit_x: usize,
    pub spl_x: usize,
    pub spl_unit_x: usize,
}

/// The four text columns of a level row. Takes no width argument on purpose:
/// every level row's unit is at the same place whether the reading in it is one
/// glyph or three, and a layout that could be asked to make room for one would
/// be free to move the unit when it did.
pub fn level_columns() -> LevelColumns {
    LevelColumns {
        value_x: LEFT_VALUE_X,
        unit_x: LEVEL_UNIT_X,
        spl_x: LEVEL_SPL_X,
        spl_unit_x: LEVEL_SPL_UNIT_X,
    }
}

/// The column just past the last one a `glyphs`-wide reading reaches.
pub fn level_value_end(value_x: usize, glyphs: usize) -> usize {
    value_x + glyphs * GLYPH_PITCH - OVERLAY_GAP
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::audio::{SCOPE_FLOOR_DECIBELS, spl};

    /// A stand-in for the board's microphone, since the offset is a property of
    /// the hardware and this module is only asserting what a column does with
    /// whatever number arrives.
    const ZTS6216: i16 = 102;

    /// How many glyphs a reading occupies, counted the way the panel's formatter
    /// does: a minus sign, then one glyph per digit.
    fn glyphs(reading: i16) -> usize {
        let mut rest = reading.unsigned_abs();
        let mut count = usize::from(reading < 0);
        while rest > 0 {
            count += 1;
            rest /= 10;
        }
        count
    }

    #[test]
    fn a_unit_starts_a_whole_blank_cell_after_the_widest_value() {
        // Enough that a number ending in a digit and a unit beginning with a
        // letter do not read as one token.
        let columns = level_columns();
        let end = level_value_end(columns.value_x, LEVEL_VALUE_GLYPHS);
        assert!(
            columns.unit_x - end >= GLYPH_PITCH,
            "only {} columns of clearance after the widest value, wanted a blank cell",
            columns.unit_x - end
        );
        for width in 1..=LEVEL_VALUE_GLYPHS {
            assert!(
                level_value_end(columns.value_x, width) < columns.unit_x,
                "a {width}-glyph value reaches the unit column"
            );
        }
    }

    #[test]
    fn a_pressure_level_reading_never_outgrows_the_column_reserved_for_it() {
        // A pressure level is a dBFS reading plus a fixed number of decibels, so
        // it widens and narrows as the room gets louder — 42 dB SPL is two
        // glyphs and 102 is three. The reservation has to be the widest of them,
        // because the SPL column is placed once and never moves: a reading that
        // outgrew its column would run into the unit beside it.
        for decibels in -SCOPE_FLOOR_DECIBELS..=0 {
            let reading = spl(decibels as i16, ZTS6216);
            assert!(
                glyphs(reading) <= LEVEL_VALUE_GLYPHS,
                "{decibels} dBFS read {reading} dB SPL, wider than {LEVEL_VALUE_GLYPHS} glyphs"
            );
        }
    }

    #[test]
    fn no_reading_or_label_reaches_into_the_column_after_it() {
        // The general form of the collision: every column has to keep clear of the
        // next one at its widest, whether that column holds a reading or a fixed
        // label. Checked against the real end of each column rather than a flat
        // glyph count, because a column's width is the thing being asked about —
        // sizing a gap as "four glyphs" is what let `DBFS` sit one pixel from the
        // pressure level beside it while every numeric test still passed.
        let columns = level_columns();
        for (left, right, glyphs_here) in [
            (columns.value_x, columns.unit_x, LEVEL_VALUE_GLYPHS),
            (columns.unit_x, columns.spl_x, LEVEL_UNIT_GLYPHS),
            (columns.spl_x, columns.spl_unit_x, LEVEL_VALUE_GLYPHS),
        ] {
            let end = level_value_end(left, glyphs_here);
            assert!(
                end + OVERLAY_GAP <= right,
                "the column at {left} ends at {end} and leaves only {} to the one at {right}",
                right as isize - end as isize
            );
        }
    }

    #[test]
    fn every_level_column_keeps_a_whole_blank_cell_between_it_and_the_next() {
        // Two glyphs separated by only the gap between their own strokes read as
        // one run of text, which is how `DBFS` came to look like part of the
        // pressure level beside it.
        let columns = level_columns();
        for (left, right, glyphs_here) in [
            (columns.value_x, columns.unit_x, LEVEL_VALUE_GLYPHS),
            (columns.unit_x, columns.spl_x, LEVEL_UNIT_GLYPHS),
            (columns.spl_x, columns.spl_unit_x, LEVEL_VALUE_GLYPHS),
        ] {
            let end = level_value_end(left, glyphs_here);
            assert!(
                right - end >= GLYPH_PITCH,
                "only {} columns of clearance after the column at {left}, wanted a blank cell",
                right - end
            );
        }
    }

    #[test]
    fn the_level_row_stays_inside_the_panel() {
        // The one row that has to hold four columns at once, so it is the one that
        // can run off the right edge.
        let columns = level_columns();
        let end = level_value_end(columns.spl_unit_x, LEVEL_UNIT_GLYPHS);
        assert!(
            end < PANEL_COLUMNS,
            "the level row ends at {end}, off a {PANEL_COLUMNS}-column panel"
        );
    }

    #[test]
    fn a_block_reading_never_outgrows_the_block_it_is_written_in() {
        // Landscape is wide enough for the blocks and short enough that a page
        // needing a seventh reading has to find it another way, so the reserved
        // width is a fact the pages are laid out against rather than a number they
        // discover by overrunning the next block.
        for block in [Block::First, Block::Second, Block::Third, Block::RightEdge] {
            let end = level_value_end(block_value_x(block), block_value_glyphs(block));
            assert!(
                end < PANEL_COLUMNS,
                "the block at {} ends at {end}, off a {PANEL_COLUMNS}-column panel",
                block_x(block)
            );
        }
    }

    #[test]
    fn the_right_edge_block_is_narrower_than_the_others() {
        // Because it is defined by the sweep beside it rather than by the block
        // pitch. A page that needs a wide reading in it is silently writing past
        // the panel edge, which reads as a clipped digit.
        assert!(
            block_value_glyphs(Block::RightEdge) < block_value_glyphs(Block::First),
            "the right-edge block reserves as much as a full block, so nothing is \
             pinning it to the width the sweep leaves it"
        );
    }

    #[test]
    fn one_blocks_widest_reading_keeps_a_blank_cell_before_the_next() {
        // The pitch's whole reason for being. Two glyphs separated by only the
        // gap between their own strokes read as one run of text, so a reading that
        // grew into the next block's first letter would not look like a defect.
        for (left, right) in [(Block::First, Block::Second), (Block::Second, Block::Third)] {
            let end = level_value_end(block_value_x(left), BLOCK_VALUE_GLYPHS);
            assert!(
                block_x(right) - end >= GLYPH_PITCH,
                "only {} columns of clearance after the block at {}, wanted a blank cell",
                block_x(right) - end,
                block_x(left)
            );
        }
    }

    /// The last row whose text still fits the panel height.
    fn last_row_that_fits() -> usize {
        (0..)
            .take_while(|row| overlay_row_top(*row) + FONT_H <= PANEL_ROWS)
            .last()
            .expect("the first row fits a panel it is meant for")
    }

    #[test]
    fn a_block_holds_eighteen_rows_and_not_one_more() {
        // The portrait panel was tall enough for twenty-one rows and landscape is
        // not, and the failure is silent: `stamp_pixel` clips an out-of-panel row
        // rather than complaining, so a page that outgrew the panel would simply
        // lose readouts. The count is pinned instead, because the pages are laid
        // out against it.
        assert_eq!(
            last_row_that_fits(),
            17,
            "a block has {} rows, and the pages are laid out for a different count",
            last_row_that_fits() + 1
        );
    }

    #[test]
    fn a_label_stays_inside_its_own_blocks_label_slot() {
        // The reading column is what makes the labels line up, so a label wider
        // than its slot reaches under the reading it is supposed to introduce.
        for block in [Block::First, Block::Second, Block::Third, Block::RightEdge] {
            let label_end = level_value_end(block_x(block), BLOCK_LABEL_GLYPHS);
            assert!(
                label_end < block_value_x(block),
                "the label slot at {} reaches its own reading column",
                block_x(block)
            );
        }
    }
}
