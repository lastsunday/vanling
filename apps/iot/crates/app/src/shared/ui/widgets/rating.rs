//! Star rating: a whole-star input row, and a read-only row that fills a fraction of each star.
//! The star is a scanline fill of one const polygon, so the mark costs no glyph and no table.

use iot_core::ui::{Canvas, Rect};

use crate::shared::ui::theme::Theme;

/// One star's touch slot; the glyph sits centred inside it.
pub const SLOT: i16 = 44;
pub const GLYPH: i16 = 24;
pub const HEIGHT: i16 = SLOT;

/// The five-point star, outer radius 500 and inner 190, in thousandths of the glyph box.
const STAR: [(i32, i32); 10] = [
    (500, 0),
    (612, 346),
    (975, 345),
    (681, 559),
    (794, 904),
    (500, 690),
    (206, 904),
    (319, 559),
    (25, 345),
    (388, 346),
];

pub const fn width(count: u8) -> i16 {
    SLOT * count as i16
}

/// The star a panel point picks, whole-star input only.
pub fn star_at(rect: Rect, count: u8, x: i32, y: i32) -> Option<u8> {
    if !rect.contains(x, y) || count == 0 {
        return None;
    }
    let index = (x - rect.x) / i32::from(SLOT);
    (index < i32::from(count)).then_some(index as u8)
}

/// Draws `count` stars, the first `whole` filled whole and the next filled `part` tenths.
pub fn draw(canvas: &mut Canvas<'_>, rect: Rect, count: u8, whole: u8, part: u8, theme: &Theme) {
    for index in 0..count {
        let slot = Rect::new(
            rect.x + i32::from(index) * i32::from(SLOT),
            rect.y,
            i32::from(SLOT),
            rect.h,
        );
        let glyph = Rect::new(
            slot.x + (slot.w - i32::from(GLYPH)) / 2,
            slot.y + (slot.h - i32::from(GLYPH)) / 2,
            i32::from(GLYPH),
            i32::from(GLYPH),
        );
        canvas.set_color(Some(theme.muted));
        fill(canvas, glyph);
        let tenths = if index < whole {
            10
        } else if index == whole {
            part.min(10)
        } else {
            0
        };
        if tenths > 0 {
            canvas.set_color(Some(theme.accent));
            canvas.push_clip(Rect::new(
                glyph.x,
                glyph.y,
                glyph.w * i32::from(tenths) / 10,
                glyph.h,
            ));
            fill(canvas, glyph);
            canvas.pop_clip();
        }
    }
    canvas.set_color(None);
}

fn fill(canvas: &mut Canvas<'_>, rect: Rect) {
    let mut crossings = [0i32; STAR.len()];
    for row in 0..rect.h {
        let y = (row * 1000 + 500) / rect.h.max(1);
        let mut count = 0;
        for index in 0..STAR.len() {
            let (x0, y0) = STAR[index];
            let (x1, y1) = STAR[(index + 1) % STAR.len()];
            if (y0 <= y) != (y1 <= y) {
                crossings[count] = x0 + (y - y0) * (x1 - x0) / (y1 - y0);
                count += 1;
            }
        }
        for index in 1..count {
            let mut at = index;
            while at > 0 && crossings[at - 1] > crossings[at] {
                crossings.swap(at - 1, at);
                at -= 1;
            }
        }
        let mut pair = 0;
        while pair + 1 < count {
            let left = rect.x + crossings[pair] * rect.w / 1000;
            let right = rect.x + crossings[pair + 1] * rect.w / 1000;
            if right > left {
                canvas.fill_rect(Rect::new(left, rect.y + row, right - left, 1));
            }
            pair += 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: Rect = Rect::new(10, 0, width(5) as i32, SLOT as i32);

    #[test]
    fn every_star_takes_its_own_slot() {
        for index in 0..5u8 {
            let x = BOX.x + i32::from(index) * i32::from(SLOT) + 4;
            assert_eq!(star_at(BOX, 5, x, 10), Some(index));
        }
        assert_eq!(star_at(BOX, 5, BOX.x - 1, 10), None);
        assert_eq!(
            star_at(BOX, 5, BOX.x + i32::from(SLOT) * 5, 10),
            None,
            "past the last slot"
        );
    }

    #[test]
    fn the_row_is_exactly_five_touch_targets_wide() {
        assert_eq!(width(5), 220);
        assert_eq!(HEIGHT, 44);
        assert!(width(5) <= 306, "the page's content width");
    }

    #[test]
    fn the_polygon_stays_inside_its_box() {
        for (x, y) in STAR {
            assert!((0..=1000).contains(&x) && (0..=1000).contains(&y));
        }
    }
}
