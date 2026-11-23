//! The panel's own measurements: the glass, and the lines and margins a page is laid out in.
//! Every number derives from the face the panel draws with, so a page moves with the font
//! rather than with a copy of its metrics.

use super::font::Face;

pub const PANEL_COLUMNS: usize = 320;
pub const PANEL_ROWS: usize = 240;

/// Rows the footer owns: the FPS badge and nothing else.
pub const FOOTER_ROWS: usize = 12;

/// The first row no page's content may draw in.
pub const CONTENT_BOTTOM: usize = PANEL_ROWS - FOOTER_ROWS;

/// Rows one line of text occupies at this face.
pub const LINE: usize = 12;

/// Columns one ASCII cell occupies: half a hanzi, which is what makes the face monospaced.
pub const GLYPH: usize = 6;

/// Distance between the tops of two stacked lines: one blank row between them.
pub const ROW_PITCH: usize = 13;

/// The smallest box a finger is asked to aim at.
pub const TOUCH_MIN: usize = 44;

/// Margin between the panel's edge and a page's content.
pub const MARGIN: usize = 10;

pub const RIGHT: usize = PANEL_COLUMNS - MARGIN;

const _: () = assert!(
    FACE_HEIGHT == LINE,
    "the panel's line height no longer matches the face it draws with"
);

const FACE_HEIGHT: usize = Face::new().height();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_is_measured_in_the_face_it_draws_with() {
        let face = Face::new();
        assert_eq!(face.height(), LINE);
        assert_eq!(face.advance('M'), GLYPH, "an ASCII cell is half a hanzi");
        assert_eq!(face.advance('中'), LINE, "a hanzi fills the line box");
    }
}
