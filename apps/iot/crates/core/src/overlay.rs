//! Where the on-panel diagnostics overlay puts each block of text.

/// The panel the layout below is built for, in panel columns/rows.
pub const PANEL_COLUMNS: usize = 320;
pub const PANEL_ROWS: usize = 240;

/// Rows along the bottom of the panel that belong to no page's content: the global FPS
/// badge, which the panel draws last and on every page.
pub const FOOTER_ROWS: usize = 8;

/// The first row below every page's content: the top of the footer.
pub const CONTENT_BOTTOM: usize = PANEL_ROWS - FOOTER_ROWS;

/// Width of one glyph cell.
pub const FONT_W: usize = 5;
/// Horizontal gap between glyph columns: the blank column that keeps two words from reading
/// as one.
pub const OVERLAY_GAP: usize = 4;
/// Distance between the left edges of adjacent glyph columns.
pub const GLYPH_PITCH: usize = FONT_W + OVERLAY_GAP;

/// Rows one glyph cell occupies, matching the 5x8 font the panel draws with: seven rows of
/// cap and x-height, and the one descender row the cell has.
pub const FONT_H: usize = 8;
/// Vertical gap between diagnostics rows.
pub const OVERLAY_ROW_GAP: usize = 5;
/// Distance between the top edges of adjacent diagnostics rows.
pub const ROW_PITCH: usize = FONT_H + OVERLAY_ROW_GAP;
/// Top edge of the first diagnostics row, in panel rows.
pub const OVERLAY_Y: usize = 8;

/// The top edge of `row`, in panel rows.
pub const fn overlay_row_top(row: usize) -> usize {
    OVERLAY_Y + row * ROW_PITCH
}

/// Left edge of the overlay, in panel columns.
pub const OVERLAY_X: usize = 10;

/// Label slot of a block, in glyph columns: a three-glyph label plus one blank cell, so the
/// reading beside it starts on one vertical line in every block.
pub const BLOCK_LABEL_GLYPHS: usize = 4;

/// Glyphs a label written in a block is allowed to occupy.
#[cfg(test)]
pub const BLOCK_LABEL_MAX_GLYPHS: usize = BLOCK_LABEL_GLYPHS - 1;

/// Distance between the left edges of adjacent text blocks.
pub const BLOCK_PITCH: usize = 104;

/// Reading column of a block, measured from the block's left edge.
pub const BLOCK_VALUE_OFFSET: usize = BLOCK_LABEL_GLYPHS * GLYPH_PITCH;

/// Left edge of the narrow block pinned to the panel's right edge, in panel columns.
pub const RIGHT_EDGE_X: usize = 242;

/// Where a page writes one of its readout rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    First,
    Second,
    Third,
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

/// Value column of the Audio level rows: a 3-glyph label slot plus one space glyph between
/// label and value.
pub const LEFT_VALUE_X: usize = block_value_x(Block::First);

/// Unit column of the Audio level rows, a [`LEVEL_VALUE_GLYPHS`]-glyph value plus a whole
/// blank cell after [`LEFT_VALUE_X`].
pub const LEVEL_UNIT_X: usize = LEFT_VALUE_X + 4 * GLYPH_PITCH;

/// Value column of the sound pressure level beside it, and the column its own unit label
/// sits in.
pub const LEVEL_SPL_X: usize = LEVEL_UNIT_X + (LEVEL_UNIT_GLYPHS + 1) * GLYPH_PITCH;
pub const LEVEL_SPL_UNIT_X: usize = LEVEL_SPL_X + 4 * GLYPH_PITCH;

/// Glyphs a level *unit* label occupies.
pub const LEVEL_UNIT_GLYPHS: usize = 4;

/// Glyphs a level value is allowed to occupy.
pub const LEVEL_VALUE_GLYPHS: usize = 3;

/// Glyphs a reading in a block on the block rhythm is allowed to occupy.
pub const BLOCK_VALUE_GLYPHS: usize = 7;

/// Glyphs the narrow right-edge block has room for.
pub const RIGHT_EDGE_VALUE_GLYPHS: usize = 5;

/// How wide a reading in `block` may be before it reaches the panel edge or the next block.
#[cfg(test)]
pub const fn block_value_glyphs(block: Block) -> usize {
    match block {
        Block::RightEdge => RIGHT_EDGE_VALUE_GLYPHS,
        _ => BLOCK_VALUE_GLYPHS,
    }
}

/// The Network page's first list row: the row below its three-row header.
pub const NETWORK_LIST_ROW: usize = 3;

/// Where one level row's text sits, as the left edge of each of its four columns.
pub struct LevelColumns {
    pub value_x: usize,
    pub unit_x: usize,
    pub spl_x: usize,
    pub spl_unit_x: usize,
}

/// The four text columns of a level row.
pub fn level_columns() -> LevelColumns {
    LevelColumns {
        value_x: LEFT_VALUE_X,
        unit_x: LEVEL_UNIT_X,
        spl_x: LEVEL_SPL_X,
        spl_unit_x: LEVEL_SPL_UNIT_X,
    }
}

/// The column just past the last one a `glyphs`-wide reading reaches.
pub const fn level_value_end(value_x: usize, glyphs: usize) -> usize {
    value_x + glyphs * GLYPH_PITCH - OVERLAY_GAP
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::audio::{SCOPE_FLOOR_DECIBELS, spl};

    const ZTS6216: i16 = 102;

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
        let columns = level_columns();
        let end = level_value_end(columns.spl_unit_x, LEVEL_UNIT_GLYPHS);
        assert!(
            end < PANEL_COLUMNS,
            "the level row ends at {end}, off a {PANEL_COLUMNS}-column panel"
        );
    }

    #[test]
    fn a_block_reading_never_outgrows_the_block_it_is_written_in() {
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

    fn last_row_that_fits() -> usize {
        (0..)
            .take_while(|row| overlay_row_top(*row) + FONT_H <= PANEL_ROWS)
            .last()
            .expect("the first row fits a panel it is meant for")
    }

    #[test]
    fn a_block_holds_eighteen_rows_and_not_one_more() {
        assert_eq!(
            last_row_that_fits(),
            17,
            "a block has {} rows, and the pages are laid out for a different count",
            last_row_that_fits() + 1
        );
    }

    #[test]
    fn a_label_stays_inside_its_own_blocks_label_slot() {
        for block in [Block::First, Block::Second, Block::Third, Block::RightEdge] {
            let label_end = level_value_end(block_x(block), BLOCK_LABEL_GLYPHS);
            assert!(
                label_end < block_value_x(block),
                "the label slot at {} reaches its own reading column",
                block_x(block)
            );
        }
    }

    #[test]
    fn a_label_leaves_the_blank_cell_its_slot_reserves() {
        let widest = level_value_end(0, BLOCK_LABEL_MAX_GLYPHS);
        assert!(
            block_value_x(Block::First) - widest >= GLYPH_PITCH,
            "the widest allowed label ends {widest}, leaving {} to the reading column",
            block_value_x(Block::First) - widest
        );
    }
}
