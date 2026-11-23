//! The page's furniture: margins, line rhythm, title, scrolling body, and the reading row.

use iot_core::ui::panel;
use iot_core::ui::{FontFamily, Length, ScrollId, Style, Ui};

use crate::shared::snapshot::ScrollAreaView;
use crate::shared::ui::scroll::{BAR, MIN_THUMB, thumb};

pub const MARGIN: i16 = panel::MARGIN as i16;
pub const LINE: i16 = panel::LINE as i16;
pub const PITCH: i16 = panel::ROW_PITCH as i16;
pub const TOUCH: i16 = panel::TOUCH_MIN as i16;

/// The label slot of a reading line, in pixels and in ASCII cells.
pub const LABEL: i16 = 24;
pub const LABEL_BYTES: usize = LABEL as usize / panel::GLYPH;

/// The value slot. 42 px holds `-3276.8`, the widest number a page prints; a longer value
/// keeps its own width instead of being cut.
pub const VALUE: i16 = 42;
pub const VALUE_BYTES: usize = VALUE as usize / panel::GLYPH;

/// One reading line: `label`, a cell of air, `value`. Fixed, so a column of readings keeps its
/// width frame to frame and every value ends on the same edge.
pub const READING: i16 = LABEL + VALUE + panel::GLYPH as i16;
const READING_BYTES: usize = READING as usize / panel::GLYPH;

/// Runs a page's body inside the panel margin: one margin at the left and top, nothing at the
/// right, so a stretched child can reach the panel's edge; `grow(1)` leaves a `grow(1)` leaf
/// inside it the rows the page did not use.
pub fn body(ui: &mut Ui<'_, '_>, run: impl FnOnce(&mut Ui<'_, '_>)) {
    ui.column(
        Style::new()
            .padding(MARGIN, 8, 0, 0)
            .stretch()
            .grow(1)
            .gap(1),
        run,
    );
}

/// The page's name, a step larger than body copy so the panel has a head.
pub fn title(ui: &mut Ui<'_, '_>, run: &[u8]) {
    ui.text(
        run,
        Style::new()
            .nowrap()
            .font_size(Length::Px(20))
            .font_family(FontFamily::Vector),
    );
}

/// A scrolling body beside its thumb: the box grows into the rows the page left it.
pub fn scroller(
    ui: &mut Ui<'_, '_>,
    id: ScrollId,
    view: ScrollAreaView,
    body: impl FnOnce(&mut Ui<'_, '_>),
) {
    ui.row(Style::new().grow(1).basis(0).stretch().gap(BAR), |ui| {
        ui.scroll(
            Style::new()
                .grow(1)
                .basis(0)
                .gap(2)
                .scroll_area(id)
                .scroll_offset(view.offset),
            body,
        );
        ui.stack(Style::new().width(BAR), |ui| {
            if view.bar
                && let Some((top, height)) = thumb(view.rect.h, view.offset, view.max, MIN_THUMB)
            {
                ui.node(
                    Style::new()
                        .absolute()
                        .top(top)
                        .width(BAR)
                        .height(height)
                        .bordered(),
                );
            }
        });
    });
}

pub fn reading(ui: &mut Ui<'_, '_>, label: &[u8], value: &[u8]) {
    let mut buf = [0u8; 96];
    ui.text(line(&mut buf, label, value), Style::new().nowrap());
}

/// `label` in its slot, `value` right-aligned in its own; one width either way.
fn line<'a>(buf: &'a mut [u8], label: &[u8], value: &[u8]) -> &'a [u8] {
    let cells = if value.len() <= VALUE_BYTES {
        READING_BYTES
    } else {
        LABEL_BYTES + 1 + value.len()
    };
    let len = cells.min(buf.len());
    buf[..len].fill(b' ');
    let take = label.len().min(LABEL_BYTES);
    buf[..take].copy_from_slice(&label[..take]);
    let room = value.len().min(len.saturating_sub(LABEL_BYTES + 1));
    let start = len - room;
    buf[start..len].copy_from_slice(&value[value.len() - room..]);
    &buf[..len]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reading_keeps_its_width_whatever_the_number_is() {
        let (mut a, mut b) = ([0u8; 96], [0u8; 96]);
        let short = line(&mut a, b"A0", b"1");
        let long = line(&mut b, b"A0", b"-3276.8");
        assert_eq!(short.len(), READING_BYTES);
        assert_eq!(long.len(), READING_BYTES);
    }

    #[test]
    fn values_end_on_the_same_edge() {
        let (mut a, mut b) = ([0u8; 96], [0u8; 96]);
        let one = line(&mut a, b"A0", b"1");
        let many = line(&mut b, b"A0", b"-1234");
        assert_eq!(&one[LABEL_BYTES..], b"       1");
        assert_eq!(&many[LABEL_BYTES..], b"   -1234");
    }

    #[test]
    fn a_value_wider_than_its_slot_is_not_cut() {
        let mut buf = [0u8; 96];
        let value = b"192.168.100.7";
        let line = line(&mut buf, b"IP", value);
        assert_eq!(line.len(), LABEL_BYTES + 1 + value.len());
        assert_eq!(&line[LABEL_BYTES + 1..], value);
    }

    #[test]
    fn a_label_wider_than_its_slot_stops_at_the_slot() {
        let (mut a, mut b) = ([0u8; 96], [0u8; 96]);
        assert_eq!(line(&mut a, b"LOHI", b"1"), line(&mut b, b"LOHIX", b"1"));
    }
}
