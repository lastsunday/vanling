//! Paints the product's pages into a board-owned frame. The snapshot arrives with each call,
//! so the screen never holds a product type; the only thing kept between calls is the last
//! snapshot, which is what lets a scroll move the pixels it already has instead of drawing
//! them again.

use core::any::Any;

use crate::services::camera::CameraPage;
use crate::services::{descriptor, paint};
use crate::shared::snapshot::{FrameSnapshot, ScrollShift};
use crate::shared::surface::{Panel, no_custom};
use iot_core::drivers::camera::CameraSnapshot;
use iot_core::drivers::surface::{
    DamageRect, FrameDamage, ScreenContext, ScreenPainter, ScreenTarget,
};
use iot_core::light::{Rgb, rgb565};
use iot_core::ui::panel::{CONTENT_BOTTOM, FOOTER_ROWS, PANEL_COLUMNS};
use iot_core::ui::{Canvas, Screen, ThemeId};

pub struct AppPainter {
    overlay: bool,
    /// The snapshot the frame currently in the buffer was painted from.
    last: Option<FrameSnapshot>,
    /// The pixels the panel has to receive for the frame in the buffer.
    damage: FrameDamage,
}

impl AppPainter {
    pub const fn new(overlay: bool) -> Self {
        Self {
            overlay,
            last: None,
            damage: FrameDamage::FULL,
        }
    }

    fn panel<'a>(
        &'a mut self,
        target: ScreenTarget<'a>,
        context: ScreenContext,
        damage: FrameDamage,
        theme: ThemeId,
    ) -> Panel<'a> {
        let canvas = Canvas::new(target.frame, target.width, target.height, target.ground)
            .with_damage(damage);
        Panel::new(
            canvas,
            Screen {
                instance: context.instance,
                fps: context.fps,
                debug: self.overlay,
                spl_offset_db: context.spl_offset_db,
                probe: context.stack_probe,
                theme,
            },
        )
    }
}

impl ScreenPainter for AppPainter {
    fn paint(&mut self, target: ScreenTarget<'_>, context: ScreenContext, snapshot: &dyn Any) {
        let Some(frame) = snapshot.downcast_ref::<FrameSnapshot>() else {
            debug_assert!(false, "the screen's snapshot is the frame it was handed");
            self.reset();
            return;
        };
        let shift = self
            .last
            .as_ref()
            .filter(|prev| prev.theme == frame.theme)
            .and_then(|prev| descriptor(frame.page).scroll_shift(prev, frame));
        let repaint = match shift {
            Some(shift) => {
                scroll_blit(target.frame, target.width, &shift);
                let (mut repaint, mut ship) =
                    scroll_damage(&shift, target.width as u16, target.height as u16);
                // The badge sits below every scroll box, so the blit leaves it intact; it is
                // still damage so its fps reading keeps moving while the page scrolls.
                if self.overlay {
                    repaint.add(footer_damage());
                    ship.add(footer_damage());
                }
                // Text is drawn by turning the ground over, so a band that is painted twice
                // would toggle: every painted band starts from the ground again.
                if let Some(ground) = target.ground {
                    clear_damage(target.frame, target.width, ground, repaint);
                }
                self.damage = ship;
                repaint
            }
            None => {
                self.damage = FrameDamage::FULL;
                FrameDamage::FULL
            }
        };
        let mut panel = self.panel(target, context, repaint, frame.theme);
        paint(frame.page, frame, &mut panel);
        self.last = Some(*frame);
    }

    fn paint_camera(
        &mut self,
        target: ScreenTarget<'_>,
        readout: CameraSnapshot,
        context: ScreenContext,
        snapshot: &dyn Any,
    ) {
        let Some(frame) = snapshot.downcast_ref::<FrameSnapshot>() else {
            debug_assert!(false, "the screen's snapshot is the frame it was handed");
            self.reset();
            return;
        };
        let mut panel = self.panel(target, context, FrameDamage::FULL, frame.theme);
        panel.declare(frame, no_custom, |snapshot, ui| {
            CameraPage::view_frame(snapshot, ui, readout)
        });
        // The camera wrote its own pixels into this buffer: the next page paint cannot start
        // from them.
        self.reset();
    }

    fn damage(&self) -> FrameDamage {
        self.damage
    }

    fn keeps_frame(&mut self, snapshot: &dyn Any) -> bool {
        let Some(next) = snapshot.downcast_ref::<FrameSnapshot>() else {
            return false;
        };
        self.last.as_ref().is_some_and(|prev| {
            prev.theme == next.theme && descriptor(next.page).scroll_shift(prev, next).is_some()
        })
    }

    fn reset(&mut self) {
        self.last = None;
        self.damage = FrameDamage::FULL;
    }
}

/// Moves each region's pixels by its shift, in place. The rows that leave the box are dropped;
/// the rows that enter it are what the page repaints again.
fn scroll_blit(frame: &mut [u8], width: usize, shift: &ScrollShift) {
    let stride = width * 2;
    for (view, &delta) in shift.views().iter().zip(shift.shifts()) {
        if delta == 0 {
            continue;
        }
        let (x, y) = (view.rect.x.max(0) as usize, view.rect.y.max(0) as usize);
        let (w, h) = (view.rect.w.max(0) as usize, view.rect.h.max(0) as usize);
        let distance = delta.unsigned_abs() as usize;
        if w == 0 || distance >= h || (y + h) * stride + (x + w) * 2 > frame.len() {
            continue;
        }
        let row_bytes = w * 2;
        if delta > 0 {
            for row in 0..h - distance {
                let src = (y + row + distance) * stride + x * 2;
                frame.copy_within(src..src + row_bytes, src - distance * stride);
            }
        } else {
            for row in (0..h - distance).rev() {
                let src = (y + row) * stride + x * 2;
                frame.copy_within(src..src + row_bytes, src + distance * stride);
            }
        }
    }
}

/// What a scroll frame changed: rows to repaint again, and rows the panel has to receive. The panel
/// is GRAM and cannot move its own pixels, so the second set is the whole box. A shift as tall
/// as the box reuses no row.
fn scroll_damage(shift: &ScrollShift, width: u16, height: u16) -> (FrameDamage, FrameDamage) {
    let (mut repaint, mut ship) = (FrameDamage::NONE, FrameDamage::NONE);
    for (view, &delta) in shift.views().iter().zip(shift.shifts()) {
        if delta == 0 {
            continue;
        }
        let rect = view.rect;
        let rows = (delta.unsigned_abs() as i32).min(rect.h);
        let y = if delta > 0 {
            rect.y + rect.h - rows
        } else {
            rect.y
        };
        add_damage(&mut repaint, rect.x, y, rect.w, rows, width, height);
        add_damage(&mut ship, rect.x, rect.y, rect.w, rect.h, width, height);
        for damage in [&mut repaint, &mut ship] {
            add_damage(
                damage,
                view.gutter.x,
                view.gutter.y,
                view.gutter.w,
                view.gutter.h,
                width,
                height,
            );
            add_damage(
                damage,
                view.header.x,
                view.header.y,
                view.header.w,
                view.header.h,
                width,
                height,
            );
        }
    }
    (repaint, ship)
}

/// The footer band the debug badge draws in. It sits below every scroll box, so a scroll
/// blit never moves it; adding it to damage only lets the badge's fps reading refresh.
fn footer_damage() -> DamageRect {
    DamageRect::new(
        0,
        CONTENT_BOTTOM as u16,
        PANEL_COLUMNS as u16,
        FOOTER_ROWS as u16,
    )
}

fn add_damage(damage: &mut FrameDamage, x: i32, y: i32, w: i32, h: i32, width: u16, height: u16) {
    let (frame_w, frame_h) = (i32::from(width), i32::from(height));
    let x0 = x.max(0).min(frame_w);
    let y0 = y.max(0).min(frame_h);
    let x1 = (x + w).max(0).min(frame_w);
    let y1 = (y + h).max(0).min(frame_h);
    if x1 > x0 && y1 > y0 {
        damage.add(DamageRect::new(
            x0 as u16,
            y0 as u16,
            (x1 - x0) as u16,
            (y1 - y0) as u16,
        ));
    }
}

fn clear_damage(frame: &mut [u8], width: usize, ground: Rgb, damage: FrameDamage) {
    let packed = rgb565(ground).to_be_bytes();
    for rect in damage.rects() {
        let (x0, x1) = (usize::from(rect.x), usize::from(rect.right()));
        let row_bytes = (x1 - x0) * 2;
        for y in usize::from(rect.y)..usize::from(rect.bottom()) {
            let start = y * width * 2 + x0 * 2;
            for pixel in frame[start..start + row_bytes].as_chunks_mut::<2>().0 {
                pixel.copy_from_slice(&packed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{Services, descriptor};
    use crate::shared::route::PageId;
    use crate::shared::snapshot::{PageSnapshot, ScrollAreaView, TEXT_AREAS, TextSnapshot};
    use alloc::vec;
    use iot_core::ui::Rect;

    const WIDTH: usize = 320;
    const HEIGHT: usize = 240;

    fn view(x: i32, y: i32, w: i32, h: i32, offset: i16) -> ScrollAreaView {
        ScrollAreaView {
            rect: Rect::new(x, y, w, h),
            header: Rect::new(x, y - 13, w + 4, 13),
            gutter: Rect::new(x + w + 2, y, 2, h),
            offset,
            max: 100,
            bar: true,
        }
    }

    fn frame(areas: [ScrollAreaView; TEXT_AREAS]) -> FrameSnapshot {
        FrameSnapshot {
            page: PageId::Text,
            pages: PageSnapshot {
                text: TextSnapshot {
                    areas,
                    moving: true,
                    ..TextSnapshot::default()
                },
                ..PageSnapshot::default()
            },
            ..FrameSnapshot::default()
        }
    }

    fn two(offset: i16) -> FrameSnapshot {
        frame([
            view(10, 40, 100, 30, offset),
            view(170, 40, 100, 30, offset),
        ])
    }

    fn shift(deltas: [i16; TEXT_AREAS]) -> ScrollShift {
        ScrollShift::new(&two(10).pages.text.areas, &deltas)
    }

    fn pixel(frame: &[u8], x: usize, y: usize) -> u16 {
        let at = (y * WIDTH + x) * 2;
        u16::from_be_bytes([frame[at], frame[at + 1]])
    }

    fn set_pixel(frame: &mut [u8], x: usize, y: usize, value: u16) {
        let at = (y * WIDTH + x) * 2;
        frame[at..at + 2].copy_from_slice(&value.to_be_bytes());
    }

    /// The box rectangles in one damage set: its 100-px-wide marks, not header or gutter.
    fn boxes(damage: FrameDamage, x: u16) -> Vec<(u16, u16)> {
        damage
            .rects()
            .iter()
            .filter(|rect| rect.x == x && rect.w == 100)
            .map(|rect| (rect.y, rect.h))
            .collect()
    }

    #[test]
    fn the_overlay_does_not_turn_a_scroll_into_a_full_repaint() {
        let mut painter = AppPainter::new(true);
        painter.last = Some(two(10));
        assert!(
            painter.keeps_frame(&two(16)),
            "the badge is a footer strip, not a reason to redraw every pixel"
        );
        assert!(
            !painter.keeps_frame(&two(10)),
            "a still page has nothing to blit"
        );
    }

    #[test]
    fn the_badge_band_is_the_footer() {
        let rect = footer_damage();
        assert_eq!((rect.y, rect.h), (228, 12));
        assert_eq!(rect.w, 320);
    }

    #[test]
    fn a_fling_ending_with_no_offset_change_ships_nothing() {
        let before = two(10);
        let mut after = two(10);
        after.pages.text.moving = false;
        let shift = descriptor(PageId::Text)
            .scroll_shift(&before, &after)
            .expect("a no-op scroll frame");
        assert_eq!(
            scroll_damage(&shift, WIDTH as u16, HEIGHT as u16),
            (FrameDamage::NONE, FrameDamage::NONE)
        );
    }

    #[test]
    fn scroll_blit_moves_each_box_up_and_leaves_the_rest() {
        let mut frame = vec![0u8; WIDTH * HEIGHT * 2];
        for y in 40..70 {
            for x in 10..110 {
                set_pixel(&mut frame, x, y, y as u16);
            }
        }
        scroll_blit(&mut frame, WIDTH, &shift([6, 0]));
        for y in 40..64 {
            assert_eq!(pixel(&frame, 10, y), (y + 6) as u16);
        }
        for y in 64..70 {
            assert_eq!(pixel(&frame, 10, y), y as u16, "the tail is not moved");
        }
        assert_eq!(pixel(&frame, 9, 50), 0, "left of the box is untouched");
        assert_eq!(pixel(&frame, 110, 50), 0, "right of the box is untouched");
        assert_eq!(pixel(&frame, 170, 50), 0, "the other box is untouched");
    }

    #[test]
    fn scroll_blit_moves_content_down_too() {
        let mut frame = vec![0u8; WIDTH * HEIGHT * 2];
        for y in 40..70 {
            for x in 10..110 {
                set_pixel(&mut frame, x, y, y as u16);
            }
        }
        scroll_blit(&mut frame, WIDTH, &shift([-6, 0]));
        for y in 46..70 {
            assert_eq!(pixel(&frame, 10, y), (y - 6) as u16);
        }
        for y in 40..46 {
            assert_eq!(pixel(&frame, 10, y), y as u16, "the head is not moved");
        }
    }

    #[test]
    fn both_boxes_damage_their_own_strip_header_and_thumb() {
        let (damage, _) = scroll_damage(&shift([6, -4]), WIDTH as u16, HEIGHT as u16);
        assert_eq!(damage.rects().len(), 6);
        let hit = |x: i32, y: i32| {
            damage
                .rects()
                .iter()
                .any(|rect| rect.contains(x as u16, y as u16))
        };
        assert!(hit(10, 64), "first box's bottom strip");
        assert!(hit(170, 40), "second box's top strip");
        assert!(hit(10, 27), "first header");
        assert!(hit(170, 27), "second header");
        assert!(hit(112, 50), "first thumb");
        assert!(hit(272, 50), "second thumb");
    }

    #[test]
    fn the_panel_gets_the_whole_box_but_the_page_repaints_only_the_strip() {
        let (width, height) = (WIDTH as u16, HEIGHT as u16);
        let (repaint, ship) = scroll_damage(&shift([6, -4]), width, height);
        assert_eq!(boxes(repaint, 10), vec![(64, 6)]);
        assert_eq!(boxes(repaint, 170), vec![(40, 4)]);
        assert_eq!(boxes(ship, 10), vec![(40, 30)]);
        assert_eq!(boxes(ship, 170), vec![(40, 30)]);
        assert_eq!(
            ship.rects().len(),
            6,
            "a whole box replaces its strip, it does not add a rect"
        );
    }

    #[test]
    fn a_fast_flick_keeps_the_scroll_path() {
        let before = two(10);
        let after = two(40);
        let mut painter = AppPainter::new(true);
        painter.last = Some(before);
        assert!(
            painter.keeps_frame(&after),
            "a fast flick must not refill and repaint the whole page"
        );
        let (repaint, ship) = scroll_damage(&shift([30, 30]), WIDTH as u16, HEIGHT as u16);
        assert_eq!(boxes(repaint, 10), vec![(40, 30)], "the whole box repaints");
        assert_eq!(boxes(ship, 10), vec![(40, 30)]);
        assert_eq!(ship.rects().len(), 6);
    }

    #[test]
    fn a_scroll_ships_every_row_the_panel_cannot_move_itself() {
        let base = Services::default().snapshot();
        let frame_at = |left: i16, right: i16| {
            let mut pages = base;
            pages.text.areas[0].offset = left;
            pages.text.areas[1].offset = right;
            pages.text.moving = true;
            FrameSnapshot {
                page: PageId::Text,
                pages,
                ..FrameSnapshot::default()
            }
        };

        let context = ScreenContext {
            instance: 0,
            fps: 60,
            spl_offset_db: 100,
            stack_probe: None,
        };
        let ground = descriptor(PageId::Text).ground();
        let mut painter = AppPainter::new(false);
        let mut frame = vec![0u8; WIDTH * HEIGHT * 2];
        // Stands in for the panel's GRAM: only what damage names ever changes.
        let mut panel = vec![0u8; WIDTH * HEIGHT * 2];

        let mut partial = 0usize;
        let mut ship = |painter: &mut AppPainter,
                        frame: &mut [u8],
                        panel: &mut [u8],
                        snapshot: &FrameSnapshot| {
            painter.paint(
                ScreenTarget {
                    frame,
                    width: WIDTH,
                    height: HEIGHT,
                    ground,
                },
                context,
                snapshot,
            );
            let damage = painter.damage();
            if damage.is_full() {
                panel.copy_from_slice(frame);
                return;
            }
            partial += 1;
            for rect in damage.rects() {
                let start_of_row = usize::from(rect.x) * 2;
                let row_bytes = usize::from(rect.w) * 2;
                for y in usize::from(rect.y)..usize::from(rect.bottom()) {
                    let start = y * WIDTH * 2 + start_of_row;
                    panel[start..start + row_bytes]
                        .copy_from_slice(&frame[start..start + row_bytes]);
                }
            }
        };

        let mut steps = vec![(0, 0)];
        for offset in (1..=40).step_by(2) {
            steps.push((offset as i16, 0));
        }
        // A jump at least as tall as the box: no row is reusable, so the box repaints whole.
        steps.push((40, 40));
        for offset in (1..=40).step_by(2) {
            steps.push((40, offset as i16));
        }
        for offset in (0..=40).rev().step_by(2) {
            steps.push((40, offset as i16));
        }
        for offset in (0..=40).rev().step_by(2) {
            steps.push((offset as i16, 0));
        }

        for &(left, right) in &steps {
            ship(&mut painter, &mut frame, &mut panel, &frame_at(left, right));
            if panel != frame {
                let at = panel
                    .chunks_exact(2)
                    .zip(frame.chunks_exact(2))
                    .position(|(panel, frame)| panel != frame)
                    .expect("the buffers differ");
                panic!(
                    "at ({left}, {right}) the panel is stale at ({}, {})",
                    at % WIDTH,
                    at / WIDTH
                );
            }
        }
        assert!(partial > 20, "the path under test never scrolled partially");
    }

    #[test]
    fn clearing_damage_lays_the_ground_back() {
        let mut frame = vec![0xFFu8; WIDTH * HEIGHT * 2];
        let (damage, _) = scroll_damage(&shift([6, 0]), WIDTH as u16, HEIGHT as u16);
        clear_damage(&mut frame, WIDTH, Rgb(0x12, 0x34, 0x56), damage);
        let ground = rgb565(Rgb(0x12, 0x34, 0x56));
        assert_eq!(pixel(&frame, 10, 64), ground);
        assert_eq!(pixel(&frame, 10, 27), ground);
        assert_eq!(pixel(&frame, 10, 50), 0xFFFF, "outside damage is untouched");
    }
}
