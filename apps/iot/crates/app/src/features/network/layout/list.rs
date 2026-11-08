//! The scan list: three bands between the page header and the action strip.

use iot_core::overlay::{FONT_H, ROW_PITCH, overlay_row_top};

use super::TOUCH_MIN;
use super::toolbar::TOOLBAR_TOP;

/// The scan rows the panel shows at once: what the list's scroll window keeps on glass.
pub const LIST_ROWS: usize = 3;

/// The row below the header's three, where the page's own content starts.
const LIST_ROW: usize = 3;

/// Top edge of the scan list, in panel rows.
pub const LIST_TOP: usize = overlay_row_top(LIST_ROW);

/// The scan list's rows and their pitch.
pub const LIST_ROW_H: usize = (TOOLBAR_TOP - LIST_TOP) / LIST_ROWS;

const _: () = assert!(
    LIST_TOP + LIST_ROWS * LIST_ROW_H <= TOOLBAR_TOP,
    "the list's last row runs into the page's action strip"
);
const _: () = assert!(
    LIST_ROW_H >= TOUCH_MIN,
    "a control on this page is smaller than a finger"
);

/// Top edge of the first of the two lines a band carries: the network's name, and the
/// address its radio answers on below it.
pub const fn band_lines_top(band: usize) -> usize {
    LIST_TOP + band * LIST_ROW_H + (LIST_ROW_H - ROW_PITCH - FONT_H) / 2
}

const _: () = assert!(
    (LIST_ROW_H - ROW_PITCH - FONT_H) / 2 + ROW_PITCH + FONT_H <= LIST_ROW_H,
    "a band's second line runs into the row under it"
);

/// The band of the scan list a point falls on, if it falls on the list at all.
pub fn list_band_at(y: u16) -> Option<usize> {
    let y = usize::from(y);
    (LIST_TOP..TOOLBAR_TOP)
        .contains(&y)
        .then(|| ((y - LIST_TOP) / LIST_ROW_H).min(LIST_ROWS - 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use iot_core::overlay::CONTENT_BOTTOM;

    #[test]
    fn a_band_is_a_row_from_the_top_of_the_list_to_the_next_one() {
        for band in 0..LIST_ROWS {
            for offset in 0..LIST_ROW_H {
                assert_eq!(
                    list_band_at((LIST_TOP + band * LIST_ROW_H + offset) as u16),
                    Some(band),
                    "band {band} at offset {offset}"
                );
            }
        }
        assert_eq!(
            list_band_at((TOOLBAR_TOP - 1) as u16),
            Some(LIST_ROWS - 1),
            "the list's own last row is the last band"
        );
        assert_eq!(list_band_at(LIST_TOP as u16 - 1), None);
        assert_eq!(
            list_band_at(TOOLBAR_TOP as u16),
            None,
            "the action strip is not a band of the list"
        );
        assert_eq!(list_band_at(CONTENT_BOTTOM as u16), None);
    }

    #[test]
    fn a_band_holds_a_name_and_the_address_below_it_without_reaching_the_next_band() {
        for band in 0..LIST_ROWS {
            let top = band_lines_top(band);
            let band_top = LIST_TOP + band * LIST_ROW_H;
            assert!(top >= band_top, "band {band}'s name is above its own row");
            assert!(
                top + ROW_PITCH + FONT_H <= band_top + LIST_ROW_H,
                "band {band}'s address runs into the row under it"
            );
            let leftover = LIST_ROW_H - (ROW_PITCH + FONT_H);
            assert_eq!(top - band_top, leftover / 2, "band {band}");
            assert_eq!(
                band_top + LIST_ROW_H - (top + ROW_PITCH + FONT_H),
                leftover - leftover / 2,
                "band {band}"
            );
        }
    }
}
