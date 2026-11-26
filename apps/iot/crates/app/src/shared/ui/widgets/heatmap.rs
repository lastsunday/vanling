//! A year at a glance: 53 weeks by 7 days, five intensity levels, a pickable cell. The levels
//! come from the seed, so the page carries two bytes instead of a year of counts.

use iot_core::ui::{Canvas, Color, Rect};

use crate::shared::ui::theme::Theme;

pub const CELL: i16 = 4;
pub const GAP: i16 = 1;
pub const WEEKS: usize = 53;
pub const DAYS: usize = 7;
pub const CELLS: u16 = (WEEKS * DAYS) as u16;
pub const WIDTH: i16 = WEEKS as i16 * (CELL + GAP) - GAP;
pub const GRID_H: i16 = DAYS as i16 * (CELL + GAP) - GAP;
pub const LABEL_H: i16 = 12;
pub const LEGEND_H: i16 = 12;
pub const HEIGHT: i16 = LABEL_H + GRID_H + LEGEND_H;

/// The month marks the panel has room for, one every nine weeks.
const MONTHS: [u8; 6] = *b"JFMAMJ";

#[derive(Clone, Copy, Debug)]
pub struct Heatmap<'a> {
    pub seed: u16,
    pub picked: Option<u16>,
    pub less: &'a [u8],
    pub more: &'a [u8],
}

/// The five-step level a day draws at: zero is empty, four is the peak.
pub fn level(seed: u16, index: u16) -> u8 {
    let mut hash =
        u32::from(seed).wrapping_mul(0x9E37_79B1) ^ u32::from(index).wrapping_mul(0x85EB_CA77);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(0xC2B2_AE35);
    hash ^= hash >> 16;
    let base = (hash % 5) as u8;
    if index % DAYS as u16 >= 5 && base > 1 {
        base - 2
    } else {
        base
    }
}

pub fn cell_rect(rect: Rect, index: u16) -> Rect {
    let week = i32::from(index) / DAYS as i32;
    let day = i32::from(index) % DAYS as i32;
    Rect::new(
        rect.x + week * i32::from(CELL + GAP),
        rect.y + i32::from(LABEL_H) + day * i32::from(CELL + GAP),
        i32::from(CELL),
        i32::from(CELL),
    )
}

pub fn cell_at(rect: Rect, x: i32, y: i32) -> Option<u16> {
    let grid = Rect::new(
        rect.x,
        rect.y + i32::from(LABEL_H),
        i32::from(WIDTH),
        i32::from(GRID_H),
    );
    if !grid.contains(x, y) {
        return None;
    }
    let pitch = i32::from(CELL + GAP);
    let week = (x - rect.x) / pitch;
    let day = (y - grid.y) / pitch;
    if (x - rect.x) % pitch >= i32::from(CELL) || (y - grid.y) % pitch >= i32::from(CELL) {
        return None;
    }
    Some((week * DAYS as i32 + day) as u16)
}

pub fn draw(canvas: &mut Canvas<'_>, rect: Rect, map: Heatmap<'_>, theme: &Theme) {
    canvas.set_color(Some(theme.muted));
    for (mark, letter) in MONTHS.iter().enumerate() {
        let glyph = [*letter];
        canvas.draw_text(
            &glyph,
            (rect.x + mark as i32 * 9 * i32::from(CELL + GAP)).max(0) as usize,
            rect.y.max(0) as usize,
        );
    }
    let base = theme.accent;
    for index in 0..CELLS {
        let cell = cell_rect(rect, index);
        let step = level(map.seed, index);
        let color = if step == 0 {
            Color::rgba(base.rgb888().0, base.rgb888().1, base.rgb888().2, 0x18)
        } else {
            let alpha = [0x00, 0x50, 0x80, 0xB0, 0xFF][usize::from(step)];
            Color::rgba(base.rgb888().0, base.rgb888().1, base.rgb888().2, alpha)
        };
        canvas.set_color(Some(color));
        canvas.fill_rect(cell);
        if map.picked == Some(index) {
            canvas.set_color(Some(theme.foreground));
            canvas.fill_rect(Rect::new(cell.x, cell.y, cell.w, 1));
            canvas.fill_rect(Rect::new(cell.x, cell.bottom() - 1, cell.w, 1));
        }
    }
    let legend_y = rect.y + i32::from(LABEL_H + GRID_H) + 2;
    canvas.set_color(Some(theme.muted));
    canvas.draw_text(map.less, rect.x.max(0) as usize, legend_y.max(0) as usize);
    let mut left = rect.x + canvas.measure(map.less) as i32 + 4;
    for step in 0..=4u8 {
        let color = if step == 0 {
            Color::rgba(base.rgb888().0, base.rgb888().1, base.rgb888().2, 0x18)
        } else {
            let alpha = [0x00, 0x50, 0x80, 0xB0, 0xFF][usize::from(step)];
            Color::rgba(base.rgb888().0, base.rgb888().1, base.rgb888().2, alpha)
        };
        canvas.set_color(Some(color));
        canvas.fill_rect(Rect::new(
            left,
            legend_y + 3,
            i32::from(CELL),
            i32::from(CELL),
        ));
        left += i32::from(CELL + GAP);
    }
    canvas.set_color(Some(theme.muted));
    canvas.draw_text(
        map.more,
        (left + 2).max(0) as usize,
        legend_y.max(0) as usize,
    );
    canvas.set_color(None);
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: Rect = Rect::new(10, 0, WIDTH as i32, HEIGHT as i32);

    #[test]
    fn every_cell_answers_its_own_index_and_the_gutter_answers_none() {
        for index in [0u16, 1, 7, 370] {
            let cell = cell_rect(BOX, index);
            assert_eq!(cell_at(BOX, cell.x + 1, cell.y + 1), Some(index));
        }
        let cell = cell_rect(BOX, 0);
        assert_eq!(cell_at(BOX, cell.right(), cell.y), None);
        assert_eq!(cell_at(BOX, BOX.x, BOX.y + 2), None, "the month row");
    }

    #[test]
    fn the_levels_are_stable_and_bounded() {
        for index in 0..CELLS {
            let first = level(7, index);
            assert_eq!(first, level(7, index));
            assert!(first <= 4);
        }
        assert!(
            (0..CELLS).any(|index| level(7, index) != level(8, index)),
            "the seed moves the picture"
        );
    }

    #[test]
    fn the_grid_fits_the_content_width() {
        assert_eq!(WIDTH, 264);
        assert_eq!(HEIGHT, 58);
        assert!(WIDTH <= 306);
    }
}
