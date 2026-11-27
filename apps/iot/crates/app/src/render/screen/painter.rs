//! Paints the product's pages into a board-owned frame. The snapshot arrives with each call,
//! so the screen never holds a product type; the only thing kept between calls is the last
//! snapshot, which is what lets a scroll move the pixels it already has instead of drawing
//! them again.

use core::any::Any;

use crate::services::camera::CameraPage;
use crate::services::descriptor;
use crate::shared::snapshot::{FrameSnapshot, ScrollShift};
use crate::shared::surface::{Panel, footer_damage, no_custom};
use iot_core::drivers::camera::CameraSnapshot;
use iot_core::drivers::surface::{
    DamageRect, FrameDamage, ScreenContext, ScreenPainter, ScreenTarget,
};
use iot_core::ui::{Canvas, FramePrint, Screen, Source, ThemeId};

pub struct AppPainter {
    overlay: bool,
    /// The snapshot the frame currently in the buffer was painted from.
    last: Option<FrameSnapshot>,
    /// What that frame's pixels depend on, so the next paint can name the ones that moved.
    print: Option<FramePrint>,
    /// The pixels the panel has to receive for the frame in the buffer.
    damage: FrameDamage,
}

impl AppPainter {
    pub const fn new(overlay: bool) -> Self {
        Self {
            overlay,
            last: None,
            print: None,
            damage: FrameDamage::FULL,
        }
    }

    /// The snapshot the buffer holds, when it is the same page and theme as `next`. A theme or
    /// page change moves every pixel, so nothing of the old frame survives it.
    fn previous(&self, next: &FrameSnapshot) -> Option<&FrameSnapshot> {
        self.last
            .as_ref()
            .filter(|prev| prev.theme == next.theme && prev.page == next.page)
    }

    /// The scroll `next` is of the buffer's frame, when it is a scroll and nothing else.
    fn shift(&self, next: &FrameSnapshot) -> Option<ScrollShift> {
        self.previous(next)
            .and_then(|prev| descriptor(next.page).scroll_shift(prev, next))
    }

    fn panel<'a>(
        &'a mut self,
        target: ScreenTarget<'a>,
        context: ScreenContext,
        theme: ThemeId,
    ) -> Panel<'a> {
        let canvas = Canvas::new(target.frame, target.width, target.height, target.ground);
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
        let previous = self.print;
        let mut ship = None;
        let source = if self.previous(frame).is_none() {
            Source::Full
        } else if let Some(shift) = self.shift(frame) {
            scroll_blit(target.frame, target.width, &shift);
            let (repaint, mut rows) =
                scroll_damage(&shift, target.width as u16, target.height as u16);
            // The badge sits below every scroll box, so the blit leaves it intact; it is still
            // damage so its fps reading keeps moving while the page scrolls.
            if self.overlay {
                rows.add(footer_damage());
            }
            ship = Some(rows);
            Source::Window(repaint)
        } else {
            Source::Diff {
                prev: previous.as_ref(),
                moved: self.last.as_ref() != Some(frame),
            }
        };
        let mut next = FramePrint::new();
        let mut panel = self.panel(target, context, frame.theme);
        let damage = panel.view_measured(
            frame,
            descriptor(frame.page).view(),
            descriptor(frame.page).custom(),
            &mut next,
            source,
        );
        self.damage = ship.unwrap_or(damage);
        self.print = Some(next);
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
        let mut panel = self.panel(target, context, frame.theme);
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
        self.previous(next).is_some()
    }

    fn reset(&mut self) {
        self.last = None;
        self.print = None;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{Services, descriptor};
    use crate::shared::route::PageId;
    use crate::shared::snapshot::{PageSnapshot, ScrollAreaView, TEXT_AREAS, TextSnapshot};
    use crate::shared::ui::theme::Theme;
    use alloc::vec;
    use iot_core::light::{Rgb, rgb565};
    use iot_core::ui::Rect;

    const WIDTH: usize = 320;
    const HEIGHT: usize = 240;

    fn context() -> ScreenContext {
        ScreenContext {
            instance: 0,
            fps: 60,
            spl_offset_db: 100,
            stack_probe: None,
        }
    }

    /// Paints `previous` so the painter knows its fingerprints, the way the screen's last
    /// frame does before the frame under test arrives.
    fn seed(painter: &mut AppPainter, previous: &FrameSnapshot) {
        let mut scratch = vec![0u8; WIDTH * HEIGHT * 2];
        painter.paint(
            ScreenTarget {
                frame: &mut scratch,
                width: WIDTH,
                height: HEIGHT,
                ground: Some(Rgb(0, 0, 0)),
            },
            context(),
            previous,
        );
    }

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

    fn dialog_frame(minute: u16) -> FrameSnapshot {
        use crate::shared::snapshot::PickerOpen;
        use crate::shared::ui::widgets::time_picker::TimeView;
        let mut frame = FrameSnapshot {
            page: PageId::Planner,
            ..FrameSnapshot::default()
        };
        let wheel = |position: i32| crate::shared::ui::widgets::WheelView {
            position,
            count: 60,
            wrap: true,
            moving: true,
        };
        frame.pages.planner.dialog.open = PickerOpen::Time;
        frame.pages.planner.dialog.time = TimeView {
            wheels: [
                wheel(9 * 256),
                wheel(i32::from(minute) * 256),
                wheel(30 * 256),
            ],
            columns: 3,
        };
        frame
    }

    /// The minute column of the dialog the planner page declares over the whole panel.
    fn minute_column() -> DamageRect {
        let body = crate::shared::ui::widgets::picker::frame(Rect::new(0, 0, 320, 228)).body;
        let column = crate::shared::ui::widgets::time_picker::column_rect(body, 3, 1);
        DamageRect::new(
            column.x as u16,
            column.y as u16,
            column.w as u16,
            column.h as u16,
        )
    }

    #[test]
    fn a_dialog_wheel_frame_ships_only_the_column_that_moved() {
        let context = ScreenContext {
            instance: 0,
            fps: 60,
            spl_offset_db: 100,
            stack_probe: None,
        };
        let mut painter = AppPainter::new(false);
        let mut frame = vec![0xABu8; WIDTH * HEIGHT * 2];
        seed(&mut painter, &dialog_frame(5));
        assert!(
            painter.keeps_frame(&dialog_frame(6)),
            "a wheel under an open card leaves the card's own pixels alone"
        );
        painter.paint(
            ScreenTarget {
                frame: &mut frame,
                width: WIDTH,
                height: HEIGHT,
                ground: Some(Rgb(0, 0, 0)),
            },
            context,
            &dialog_frame(6),
        );
        assert_eq!(painter.damage().rects(), &[minute_column()]);
        let column = minute_column();
        assert_eq!(
            pixel(&frame, usize::from(column.x) + 1, usize::from(column.y) + 1),
            rgb565(Theme::of(ThemeId::Dark).surface.rgb888()),
            "the column was laid back down as the card"
        );
        assert_eq!(
            pixel(&frame, 0, 0),
            0xABAB,
            "a pixel outside the column is still the one the last frame left"
        );
    }

    #[test]
    fn a_partial_column_matches_the_same_frame_painted_whole() {
        let context = ScreenContext {
            instance: 0,
            fps: 60,
            spl_offset_db: 100,
            stack_probe: None,
        };
        let ground = Some(Rgb(0, 0, 0));
        let whole = {
            let mut painter = AppPainter::new(false);
            let mut frame = vec![0u8; WIDTH * HEIGHT * 2];
            painter.paint(
                ScreenTarget {
                    frame: &mut frame,
                    width: WIDTH,
                    height: HEIGHT,
                    ground,
                },
                context,
                &dialog_frame(6),
            );
            frame
        };
        let mut painter = AppPainter::new(false);
        seed(&mut painter, &dialog_frame(5));
        let mut frame = vec![0xABu8; WIDTH * HEIGHT * 2];
        painter.paint(
            ScreenTarget {
                frame: &mut frame,
                width: WIDTH,
                height: HEIGHT,
                ground,
            },
            context,
            &dialog_frame(6),
        );
        let column = minute_column();
        for y in usize::from(column.y)..usize::from(column.bottom()) {
            for x in usize::from(column.x)..usize::from(column.right()) {
                assert_eq!(
                    pixel(&frame, x, y),
                    pixel(&whole, x, y),
                    "the column at ({x}, {y}) is the one a whole repaint leaves"
                );
            }
        }
    }

    /// The planner page scrolled down to the inline hour wheel, at `hour`.
    fn inline_frame(hour: u8) -> FrameSnapshot {
        let mut page = crate::services::planner::PlannerPage::default();
        page.park(300);
        let mut planner = page.snapshot();
        planner.hour = hour;
        planner.inline = crate::shared::ui::widgets::WheelView {
            position: i32::from(hour) * 256,
            count: 24,
            wrap: true,
            moving: true,
        };
        let mut frame = FrameSnapshot {
            page: PageId::Planner,
            ..FrameSnapshot::default()
        };
        frame.pages.planner = planner;
        frame
    }

    #[test]
    fn an_inline_wheel_frame_repaints_only_the_wheel_and_its_reading() {
        let ground = Some(Rgb(0, 0, 0));
        let whole = {
            let mut painter = AppPainter::new(false);
            let mut frame = vec![0u8; WIDTH * HEIGHT * 2];
            painter.paint(
                ScreenTarget {
                    frame: &mut frame,
                    width: WIDTH,
                    height: HEIGHT,
                    ground,
                },
                context(),
                &inline_frame(10),
            );
            frame
        };
        let mut painter = AppPainter::new(false);
        let mut frame = vec![0u8; WIDTH * HEIGHT * 2];
        for hour in [9, 10] {
            painter.paint(
                ScreenTarget {
                    frame: &mut frame,
                    width: WIDTH,
                    height: HEIGHT,
                    ground,
                },
                context(),
                &inline_frame(hour),
            );
        }
        let damage = painter.damage();
        assert!(
            !damage.is_full(),
            "the wheel frame repaints a corner of the page"
        );
        let inside = |x: usize, y: usize| {
            damage.rects().iter().any(|rect| {
                (rect.x as usize..rect.right() as usize).contains(&x)
                    && (rect.y as usize..rect.bottom() as usize).contains(&y)
            })
        };
        let mut painted = 0u32;
        for rect in damage.rects() {
            painted += u32::from(rect.w) * u32::from(rect.h);
            for y in rect.y as usize..rect.bottom() as usize {
                for x in rect.x as usize..rect.right() as usize {
                    assert_eq!(
                        pixel(&frame, x, y),
                        pixel(&whole, x, y),
                        "the wheel at ({x}, {y}) is the one a whole repaint leaves"
                    );
                }
            }
        }
        assert!(
            painted < (WIDTH * HEIGHT / 4) as u32,
            "the frame is a corner of the page, not the page"
        );
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                if pixel(&frame, x, y) != pixel(&whole, x, y) {
                    assert!(
                        inside(x, y),
                        "a pixel outside the damage moved at ({x}, {y})"
                    );
                }
            }
        }
    }

    #[test]
    fn a_dialog_opening_repaints_every_pixel() {
        let mut painter = AppPainter::new(false);
        seed(&mut painter, &dialog_frame(5));
        let mut opened = dialog_frame(5);
        opened.pages.planner.dialog.open = crate::shared::snapshot::PickerOpen::None;
        assert!(painter.keeps_frame(&opened), "the plan is still a diff");
        let mut frame = vec![0xABu8; WIDTH * HEIGHT * 2];
        painter.paint(
            ScreenTarget {
                frame: &mut frame,
                width: WIDTH,
                height: HEIGHT,
                ground: Some(Rgb(0, 0, 0)),
            },
            context(),
            &opened,
        );
        assert!(
            painter.damage().is_full(),
            "a different tree cannot be patched"
        );
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
            painter.keeps_frame(&two(10)),
            "a still page keeps its frame and ships nothing"
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
        let ground = Rgb(0x12, 0x34, 0x56);
        let mut canvas = Canvas::new(&mut frame, WIDTH, HEIGHT, Some(ground));
        canvas.clear(damage);
        let ground = rgb565(ground);
        assert_eq!(pixel(&frame, 10, 64), ground);
        assert_eq!(pixel(&frame, 10, 27), ground);
        assert_eq!(pixel(&frame, 10, 50), 0xFFFF, "outside damage is untouched");
    }
}
