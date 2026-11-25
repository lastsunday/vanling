//! A panel that rises from the bottom edge, or slides in from the left.

use iot_core::ui::panel::{LINE, TOUCH_MIN};
use iot_core::ui::{Canvas, Rect};

use crate::shared::ui::theme::{CORNER, Theme};

const WIDTH: i32 = 220;
const TITLE_H: i32 = LINE as i32 + 8;
pub const ROW_H: i32 = TOUCH_MIN as i32;
const PAD: i32 = 4;
const ROWS: usize = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Edge {
    #[default]
    Bottom,
    Left,
}

#[derive(Clone, Copy, Debug)]
pub struct Sheet<'a> {
    pub title: &'a [u8],
    pub rows: [&'a [u8]; ROWS],
    pub selected: u8,
}

/// The drawer's rectangle inside `rect`, hugging its edge.
pub fn card(rect: Rect, edge: Edge) -> Rect {
    match edge {
        Edge::Bottom => {
            let h = (TITLE_H + ROW_H * ROWS as i32 + 2 * PAD).min(rect.h);
            Rect::new(rect.x, rect.bottom() - h, rect.w, h)
        }
        Edge::Left => {
            let w = WIDTH.min(rect.w);
            Rect::new(rect.x, rect.y, w, rect.h)
        }
    }
}

/// The row a point picks, or `None` on the title, the frame or the padding.
pub fn row_at(card: Rect, x: i32, y: i32) -> Option<u8> {
    let inner = Rect::new(
        card.x + PAD,
        card.y + PAD + TITLE_H,
        card.w - 2 * PAD,
        ROW_H * ROWS as i32,
    );
    if !inner.contains(x, y) {
        return None;
    }
    u8::try_from((y - inner.y) / ROW_H)
        .ok()
        .filter(|row| usize::from(*row) < ROWS)
}

impl Sheet<'_> {
    pub fn draw(self, canvas: &mut Canvas<'_>, rect: Rect, edge: Edge, theme: &Theme) {
        canvas.set_color(Some(theme.ground));
        canvas.fill_rect(rect);
        let card = card(rect, edge);
        canvas.set_color(Some(theme.surface));
        canvas.fill_round_rect(card, CORNER);
        canvas.set_color(Some(theme.muted));
        canvas.stroke_round_rect(card, CORNER);
        if edge == Edge::Bottom {
            canvas.fill_rect(Rect::new(card.x + (card.w - 28) / 2, card.y + PAD, 28, 3));
        }
        canvas.push_clip(card);
        canvas.set_color(Some(theme.foreground));
        canvas.draw_text(
            self.title,
            (card.x + PAD).max(0) as usize,
            (card.y + PAD + (TITLE_H - LINE as i32) / 2).max(0) as usize,
        );
        for (index, row) in self.rows.iter().enumerate() {
            let row_rect = Rect::new(
                card.x + PAD,
                card.y + PAD + TITLE_H + index as i32 * ROW_H,
                card.w - 2 * PAD,
                ROW_H,
            );
            if index == usize::from(self.selected) {
                canvas.set_color(Some(theme.selection));
                canvas.fill_rect(row_rect);
            }
            canvas.set_color(Some(theme.foreground));
            canvas.push_clip(row_rect);
            canvas.draw_text(
                row,
                (row_rect.x + PAD).max(0) as usize,
                (row_rect.y + (ROW_H - LINE as i32) / 2).max(0) as usize,
            );
            canvas.pop_clip();
        }
        canvas.pop_clip();
        canvas.set_color(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANEL: Rect = Rect::new(0, 0, 320, 228);

    #[test]
    fn the_bottom_sheet_hugs_the_edge_and_every_row_reads_back() {
        let card = card(PANEL, Edge::Bottom);
        assert_eq!(card.bottom(), PANEL.bottom());
        for row in 0..ROWS as u8 {
            let y = card.y + PAD + TITLE_H + i32::from(row) * ROW_H + ROW_H / 2;
            assert_eq!(row_at(card, card.x + card.w / 2, y), Some(row));
        }
        assert_eq!(
            row_at(card, card.x + 1, card.y + 1),
            None,
            "the title is not a row"
        );
    }

    #[test]
    fn the_left_sheet_hugs_the_left_edge() {
        let card = card(PANEL, Edge::Left);
        assert_eq!((card.x, card.y, card.h), (PANEL.x, PANEL.y, PANEL.h));
    }
}
