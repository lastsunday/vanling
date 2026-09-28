//! Where the on-panel diagnostics overlay puts each column of text.
//!
//! Here rather than beside the panel because the panel is only reachable on the
//! device, and a column that moved is a bug that reads as a design decision: a
//! unit nudged left by a digit is invisible as a defect and obvious as a wrong
//! answer. Every column is a fixed distance along a fixed pitch, so a readout
//! that changes width cannot drag the unit beside it out from under the row
//! above, and the arithmetic that guarantees that is checkable without a screen.

/// Width of one glyph cell. The pitch every column is placed on is this plus
/// [`OVERLAY_GAP`], so a column is an index into a rhythm rather than a number
/// someone measured off a photograph.
pub const FONT_W: usize = 5;
/// Horizontal gap between glyph columns: the blank column that keeps two words
/// from reading as one.
pub const OVERLAY_GAP: usize = 4;
/// Distance between the left edges of adjacent glyph columns.
pub const GLYPH_PITCH: usize = FONT_W + OVERLAY_GAP;

/// Left edge of the overlay, in panel columns.
pub const OVERLAY_X: usize = 10;

/// Value column of the touch/gesture column: a 3-glyph label slot plus one
/// space glyph between label and value.
pub const LEFT_VALUE_X: usize = OVERLAY_X + 4 * GLYPH_PITCH;

/// Unit column of the Audio level rows, a [`LEVEL_VALUE_GLYPHS`]-glyph value
/// plus a whole blank cell after [`LEFT_VALUE_X`]. Fixed rather than flush to
/// the right edge so a level growing a digit cannot walk its unit sideways out
/// from under the row above: the eye finds one vertical line for the unit and
/// never checks a second.
pub const LEVEL_UNIT_X: usize = LEFT_VALUE_X + 4 * GLYPH_PITCH;

/// Value column of the sound pressure level beside it, and the column its own
/// unit label sits in. A [`LEVEL_UNIT_GLYPHS`]-glyph unit plus a whole blank
/// cell, because `DBFS` is four glyphs wide and one glyph further right than its
/// own left edge still leaves the two columns touching: the value and the unit
/// then read as one run of digits and letters. The last glyph stays well clear of
/// the right edge of a 240-column panel.
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

/// The column a value of `glyphs` glyphs ends at, exclusive.
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
        // The value is given an extra slot, so the unit is separated from it by
        // an empty cell rather than by the gap between two words — enough that a
        // number ending in a digit and a unit beginning with a letter do not read
        // as one token. A minimum rather than a count, because the distance is
        // what the eye needs and not a number anyone chose deliberately.
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
        // A minimum rather than a count, because the distance is what the eye needs
        // and not a number anyone chose deliberately: two glyphs separated by only
        // the gap between their own strokes read as one run of text, which is how
        // `DBFS` came to look like part of the pressure level beside it.
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
        // can run off the right edge. 240 columns wide, as the panel is framed.
        const PANEL_COLUMNS: usize = 240;
        let columns = level_columns();
        let end = level_value_end(columns.spl_unit_x, LEVEL_UNIT_GLYPHS);
        assert!(
            end < PANEL_COLUMNS,
            "the level row ends at {end}, off a {PANEL_COLUMNS}-column panel"
        );
    }
}
