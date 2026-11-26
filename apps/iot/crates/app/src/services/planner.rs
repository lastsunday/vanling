//! The planner-widget page: a month grid, an hour wheel, a star row and an HSV picker. Every
//! mark is one canvas slot whose draw and hit test share one geometry.

use iot_core::drivers::input::{GestureEvent, InputEvent};
use iot_core::ui::panel::PANEL_COLUMNS;
use iot_core::ui::{Canvas, Rect, Screen, ScrollId, Style, Ui};

use crate::shared::i18n::Msg;
use crate::shared::intent::{BusinessIntent, GalleryIntent, InputIntent};
use crate::shared::model::{Command, DeviceState, Effects};
use crate::shared::page;
use crate::shared::route::PageId;
use crate::shared::snapshot::{FrameSnapshot, PageSnapshot, PlannerSnapshot, ScrollShift};
use crate::shared::surface::{finish, headless};
use crate::shared::ui::scroll::{BAR, ScrollConfig};
use crate::shared::ui::tag::TagTable;
use crate::shared::ui::theme::Theme;
use crate::shared::ui::widgets::{
    Button, Picker, Wheel, calendar, color_picker, rating, time_picker,
};

use super::gallery::{REPAINT_MS, Scroller};
use super::{Descriptor, Repaint};

const AREA: ScrollId = ScrollId::new(1);
const CONTENT_W: i16 = PANEL_COLUMNS as i16 - page::MARGIN - 2 * BAR;

const CALENDAR_SLOT: u8 = 0;
const WHEEL_SLOT: u8 = 1;
const RATING_SLOT: u8 = 2;
const PICKER_SLOT: u8 = 3;

/// Sub-cell tags the page computes for itself: a day, an hour, a star.
const TAG_CALENDAR: u16 = 100;
const TAG_WHEEL: u16 = 140;
const TAG_RATING: u16 = 170;
const TAG_PICKER: u16 = 180;

const SCROLL: ScrollConfig = ScrollConfig {
    bounce: true,
    bar: true,
    pull: 0,
};

/// The two month buttons; the discriminant is the tag the declaration draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
enum Action {
    Prev = 1,
    Next,
}

impl Action {
    const ALL: [Self; 2] = [Self::Prev, Self::Next];

    const fn tag(self) -> u16 {
        self as u16
    }

    fn from_tag(tag: u16) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.tag() == tag)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerPage {
    scroll: Scroller,
    tags: TagTable<{ Action::ALL.len() + 4 }>,
    calendar: Rect,
    wheel: Rect,
    stars: Rect,
    picker: Rect,
    /// The finger that is painting the picker, so its drags do not scroll the page.
    picker_drag: Option<u8>,
    values: PlannerSnapshot,
}

impl Default for PlannerPage {
    fn default() -> Self {
        let mut ui = headless();
        declare(&FrameSnapshot::default(), &mut ui);
        finish(&mut ui);
        let mut scroll = Scroller::default();
        scroll.measure(&ui, AREA, SCROLL);
        Self {
            scroll,
            tags: TagTable::measure(&ui),
            calendar: ui.paint_rect(CALENDAR_SLOT).unwrap_or_default(),
            wheel: ui.paint_rect(WHEEL_SLOT).unwrap_or_default(),
            stars: ui.paint_rect(RATING_SLOT).unwrap_or_default(),
            picker: ui.paint_rect(PICKER_SLOT).unwrap_or_default(),
            picker_drag: None,
            values: PlannerSnapshot::default(),
        }
    }
}

impl PlannerPage {
    fn tag_at(&self, x: i32, y: i32) -> u16 {
        self.scroll
            .tag_at(|x, y, offset| self.tags.at(x, y, offset), x, y)
    }

    fn snapshot(&self) -> PlannerSnapshot {
        let mut view = self.values;
        view.area = self.scroll.view();
        view.moving = self.scroll.moving();
        view
    }

    fn picker_at(&self, x: i32, y: i32) -> bool {
        let area = self.scroll.area();
        area.rect.contains(x, y) && self.picker.contains(x, y + i32::from(area.offset))
    }

    fn sample_picker(&mut self, x: u16, y: u16) {
        let offset = self.scroll.area().offset;
        let content = i32::from(y) + i32::from(offset);
        match color_picker::point_at(self.picker, i32::from(x), content) {
            Some(color_picker::Sample::Square { sat, val }) => {
                self.values.sat = sat;
                self.values.val = val;
            }
            Some(color_picker::Sample::Hue(hue)) => self.values.hue = hue,
            None => {}
        }
    }

    fn tap_tag(&mut self, tag: u16) {
        if let Some(day) = tag
            .checked_sub(TAG_CALENDAR)
            .filter(|day| (1..=31).contains(day))
        {
            if calendar::days_in_month(self.values.year, self.values.month) >= day as u8 {
                self.values.day = day as u8;
            }
            return;
        }
        if let Some(hour) = tag.checked_sub(TAG_WHEEL).filter(|hour| *hour < 24) {
            self.values.hour = hour as u8;
            return;
        }
        if let Some(star) = tag.checked_sub(TAG_RATING).filter(|star| *star < 5) {
            self.values.rating = star as u8 + 1;
            return;
        }
        if let Some(action) = Action::from_tag(tag) {
            self.shift_month(match action {
                Action::Prev => -1,
                Action::Next => 1,
            });
        }
    }

    fn shift_month(&mut self, delta: i32) {
        let mut month = i32::from(self.values.month) + delta;
        let mut year = i32::from(self.values.year);
        while month < 1 {
            month += 12;
            year -= 1;
        }
        while month > 12 {
            month -= 12;
            year += 1;
        }
        self.values.year = year.clamp(1, i32::from(u16::MAX)) as u16;
        self.values.month = month as u8;
        let days = calendar::days_in_month(self.values.year, self.values.month);
        self.values.day = self.values.day.clamp(1, days);
    }
}

impl super::Service for PlannerPage {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        _state: &mut DeviceState,
    ) -> (Option<Command>, Effects) {
        let BusinessIntent::Gallery { wire, intent } = intent else {
            return (None, Effects::default());
        };
        if *wire != PageId::Planner.code() {
            return (None, Effects::default());
        }
        match *intent {
            GalleryIntent::Tap { tag } => {
                self.picker_drag = None;
                self.tap_tag(tag);
            }
            GalleryIntent::Point { id, tag, x, y } if tag == TAG_PICKER => {
                self.picker_drag = Some(id);
                self.sample_picker(x, y);
            }
            _ => self.picker_drag = None,
        }
        self.scroll.apply(*intent);
        (None, Effects::default())
    }
}

impl super::LocalPage for PlannerPage {
    fn bind(&self, op: &InputIntent, _state: &DeviceState) -> Option<BusinessIntent> {
        let gallery = |intent| {
            Some(BusinessIntent::Gallery {
                wire: PageId::Planner.code(),
                intent,
            })
        };
        match op.event {
            InputEvent::Gesture(GestureEvent::Press { id, x, y }) => {
                if self.picker_at(i32::from(x), i32::from(y)) {
                    return gallery(GalleryIntent::Point {
                        id,
                        tag: TAG_PICKER,
                        x,
                        y,
                    });
                }
            }
            InputEvent::Gesture(GestureEvent::Drag { id, x, y, .. }) => {
                if self.picker_drag == Some(id) {
                    return gallery(GalleryIntent::Point {
                        id,
                        tag: TAG_PICKER,
                        x,
                        y,
                    });
                }
            }
            InputEvent::Gesture(GestureEvent::Tap { id, .. })
            | InputEvent::Gesture(GestureEvent::Swipe { id, .. })
            | InputEvent::Gesture(GestureEvent::LongPress { id, .. })
                if self.picker_drag == Some(id) =>
            {
                return gallery(GalleryIntent::Tap { tag: TAG_PICKER });
            }
            _ => {}
        }
        if let InputEvent::Gesture(GestureEvent::Tap { x, y, .. }) = op.event
            && !self.scroll.dragging()
        {
            let area = self.scroll.area();
            let (x, y) = (i32::from(x), i32::from(y));
            if area.rect.contains(x, y) {
                let content = y + i32::from(area.offset);
                if let Some(day) = calendar::day_at(
                    self.calendar,
                    self.values.year,
                    self.values.month,
                    x,
                    content,
                ) {
                    return gallery(GalleryIntent::Tap {
                        tag: TAG_CALENDAR + u16::from(day),
                    });
                }
                if let Some(hour) = time_picker::row_at(
                    self.wheel,
                    Wheel {
                        value: self.values.hour,
                        max: 23,
                    },
                    x,
                    content,
                ) {
                    return gallery(GalleryIntent::Tap {
                        tag: TAG_WHEEL + u16::from(hour),
                    });
                }
                if let Some(star) = rating::star_at(self.stars, 5, x, content) {
                    return gallery(GalleryIntent::Tap {
                        tag: TAG_RATING + u16::from(star),
                    });
                }
                if color_picker::point_at(self.picker, x, content).is_some() {
                    return gallery(GalleryIntent::Tap { tag: TAG_PICKER });
                }
            }
        }
        self.scroll.bind(
            op,
            PageId::Planner.code(),
            |x, y| self.tag_at(x, y),
            |_| false,
        )
    }

    fn publish(&self, out: &mut PageSnapshot) {
        out.planner = self.snapshot();
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        declare(snapshot, ui);
    }

    fn descriptor() -> Descriptor {
        Descriptor::new(Self::view)
            .with_custom(custom)
            .with_repaint_ms(REPAINT_MS)
            .with_repaints(Repaint::WhenOrOverlay(planner_moved))
            .with_scroll(planner_shift)
    }

    fn tick(&mut self, step_ms: u16) {
        self.scroll.tick(step_ms);
    }

    fn on_hide(&mut self) {
        self.scroll.reset();
        self.picker_drag = None;
    }
}

fn declare(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
    let lang = snapshot.lang;
    let w = snapshot.pages.planner;
    let theme = *Theme::of(snapshot.theme);
    page::body(ui, |ui| {
        page::title(ui, Msg::PlannerTitle.bytes(lang));
        page::scroller(ui, AREA, w.area, |ui| {
            page::caption(ui, Msg::CapCalendar.bytes(lang), &theme);
            ui.row(Style::new().gap(theme.gap), |ui| {
                ui.add(
                    Button::new(Msg::PrevMonth.bytes(lang))
                        .width((CONTENT_W - theme.gap) / 2)
                        .tag(Action::Prev.tag()),
                );
                ui.add(
                    Button::new(Msg::NextMonth.bytes(lang))
                        .width((CONTENT_W - theme.gap) / 2)
                        .tag(Action::Next.tag()),
                );
            });
            ui.custom(
                Style::new()
                    .width(calendar::WIDTH)
                    .height(calendar::HEIGHT)
                    .tag(TAG_CALENDAR),
                CALENDAR_SLOT,
            );
            page::caption(ui, Msg::CapTimePicker.bytes(lang), &theme);
            ui.custom(
                Style::new()
                    .width(time_picker::COL_W)
                    .height(time_picker::HEIGHT)
                    .tag(TAG_WHEEL),
                WHEEL_SLOT,
            );
            page::caption(ui, Msg::CapRating.bytes(lang), &theme);
            ui.custom(
                Style::new()
                    .width(rating::width(5))
                    .height(rating::HEIGHT)
                    .tag(TAG_RATING),
                RATING_SLOT,
            );
            page::caption(ui, Msg::CapColorPicker.bytes(lang), &theme);
            ui.custom(
                Style::new()
                    .width(color_picker::WIDTH)
                    .height(color_picker::HEIGHT)
                    .tag(TAG_PICKER),
                PICKER_SLOT,
            );
        });
    });
}

fn custom(snapshot: &FrameSnapshot, screen: Screen, slot: u8, canvas: &mut Canvas<'_>, rect: Rect) {
    let theme = *Theme::of(screen.theme);
    let w = snapshot.pages.planner;
    match slot {
        CALENDAR_SLOT => calendar::draw(canvas, rect, w.year, w.month, Some(w.day), &theme),
        WHEEL_SLOT => time_picker::draw(
            canvas,
            rect,
            Wheel {
                value: w.hour,
                max: 23,
            },
            &theme,
        ),
        RATING_SLOT => rating::draw(canvas, rect, 5, w.rating, 0, &theme),
        PICKER_SLOT => color_picker::draw(
            canvas,
            rect,
            Picker {
                hue: w.hue,
                sat: w.sat,
                val: w.val,
            },
            &theme,
        ),
        _ => {}
    }
}

fn planner_moved(prev: &FrameSnapshot, next: &FrameSnapshot) -> bool {
    prev.lang != next.lang || prev.pages.planner != next.pages.planner
}

fn planner_shift(prev: &FrameSnapshot, next: &FrameSnapshot) -> Option<ScrollShift> {
    super::gallery::shift(
        PageId::Planner,
        prev,
        next,
        |pages| pages.planner.area,
        |pages| pages.planner.moving,
    )
}

const _: () = assert!(CONTENT_W + 2 * BAR + page::MARGIN == PANEL_COLUMNS as i16);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{LocalPage, Service};
    use crate::shared::i18n::Lang;
    use alloc::vec::Vec;
    use iot_core::ui::PX;
    use iot_core::ui::panel::CONTENT_BOTTOM;

    fn tap(page: &mut PlannerPage, tag: u16) {
        let mut state = DeviceState::default();
        page.apply(
            &BusinessIntent::Gallery {
                wire: PageId::Planner.code(),
                intent: GalleryIntent::Tap { tag },
            },
            &mut state,
        );
    }

    fn send(page: &mut PlannerPage, event: GestureEvent) {
        let op = InputIntent {
            source: 0,
            event: InputEvent::Gesture(event),
        };
        let mut state = DeviceState::default();
        if let Some(bound) = page.bind(&op, &state) {
            page.apply(&bound, &mut state);
        }
    }

    #[test]
    fn the_whole_gallery_fits_one_declaration() {
        for lang in Lang::ALL {
            let mut ui = headless();
            PlannerPage::view(
                &FrameSnapshot {
                    lang: *lang,
                    ..FrameSnapshot::default()
                },
                &mut ui,
            );
            assert!(
                ui.node_count() <= iot_core::ui::MAX_NODES,
                "{lang:?}: {} nodes",
                ui.node_count()
            );
            assert!(
                ui.text_bytes() <= 2048,
                "{lang:?}: {} bytes",
                ui.text_bytes()
            );
            finish(&mut ui);
            for rect in ui.rects() {
                assert!(
                    rect.x >= 0
                        && rect.y >= 0
                        && rect.x + rect.w <= PANEL_COLUMNS as i32 * PX
                        && rect.y + rect.h <= CONTENT_BOTTOM as i32 * PX,
                    "{lang:?}: {rect:?} leaves the panel"
                );
            }
        }
    }

    #[test]
    fn every_control_is_drawn_once() {
        let page = PlannerPage::default();
        let drawn: Vec<u16> = page.tags.iter().map(|(tag, _)| tag).collect();
        let expected: Vec<u16> = Action::ALL
            .iter()
            .map(|action| action.tag())
            .chain([TAG_CALENDAR, TAG_WHEEL, TAG_RATING, TAG_PICKER])
            .collect();
        assert_eq!(drawn, expected);
    }

    #[test]
    fn a_month_button_turns_the_grid_and_clamps_the_day() {
        let mut page = PlannerPage::default();
        page.values.month = 1;
        page.values.day = 31;
        tap(&mut page, Action::Prev.tag());
        assert_eq!((page.values.year, page.values.month), (2025, 12));
        tap(&mut page, Action::Next.tag());
        assert_eq!((page.values.year, page.values.month), (2026, 1));
        tap(&mut page, Action::Next.tag());
        assert_eq!(page.values.month, 2);
        assert_eq!(page.values.day, 28, "a short month keeps a real day");
    }

    #[test]
    fn a_tap_on_the_grid_picks_the_day_it_covers() {
        let mut page = PlannerPage::default();
        let area = page.scroll.area();
        let target = page.calendar.y + page.calendar.h / 2 - (area.rect.y + area.rect.h / 2);
        page.scroll.park(i16::try_from(target.max(0)).unwrap_or(0));
        let offset = page.scroll.area().offset;
        let index = u8::try_from(
            i32::from(calendar::first_weekday(page.values.year, page.values.month)) + 14,
        )
        .unwrap();
        let cell = calendar::cell_rect(page.calendar, index);
        send(
            &mut page,
            GestureEvent::Press {
                id: 0,
                x: u16::try_from(cell.x + 4).unwrap(),
                y: u16::try_from(cell.y + 4 - i32::from(offset)).unwrap(),
            },
        );
        send(
            &mut page,
            GestureEvent::Tap {
                id: 0,
                x: u16::try_from(cell.x + 4).unwrap(),
                y: u16::try_from(cell.y + 4 - i32::from(offset)).unwrap(),
                held_ms: 40,
                at_ms: 40,
            },
        );
        assert_eq!(page.values.day, 15);
    }

    #[test]
    fn the_wheel_the_stars_and_the_buttons_change_their_values() {
        let mut page = PlannerPage::default();
        tap(&mut page, TAG_WHEEL + 17);
        assert_eq!(page.values.hour, 17);
        tap(&mut page, TAG_RATING + 2);
        assert_eq!(page.values.rating, 3);
        tap(&mut page, TAG_RATING + 4);
        assert_eq!(page.values.rating, 5);
    }

    #[test]
    fn a_picker_press_paints_and_its_drag_does_not_scroll() {
        let mut page = PlannerPage::default();
        let area = page.scroll.area();
        let target = page.picker.y + page.picker.h / 2 - (area.rect.y + area.rect.h / 2);
        page.scroll.park(i16::try_from(target.max(0)).unwrap_or(0));
        let offset = page.scroll.area().offset;
        let square = color_picker::square(page.picker);
        let (x, y) = (
            u16::try_from(square.right() - 1).unwrap(),
            u16::try_from(square.bottom() - 1 - i32::from(offset)).unwrap(),
        );
        send(&mut page, GestureEvent::Press { id: 0, x, y });
        assert_eq!((page.values.sat, page.values.val), (255, 0));
        let before = page.scroll.area().offset;
        send(
            &mut page,
            GestureEvent::Drag {
                id: 0,
                x,
                y: y - 20,
                at_ms: 20,
            },
        );
        assert_eq!(
            page.scroll.area().offset,
            before,
            "the picker owns the drag"
        );
        assert_ne!(page.values.val, 0);
    }

    #[test]
    fn a_scroll_frame_moves_only_the_gallery() {
        let mut page = PlannerPage::default();
        let before = FrameSnapshot {
            page: PageId::Planner,
            pages: PageSnapshot {
                planner: page.snapshot(),
                ..PageSnapshot::default()
            },
            ..FrameSnapshot::default()
        };
        page.scroll.apply(GalleryIntent::Scroll {
            area: 0,
            lines: 3,
            speed: 0,
        });
        let after = FrameSnapshot {
            page: PageId::Planner,
            pages: PageSnapshot {
                planner: page.snapshot(),
                ..PageSnapshot::default()
            },
            ..FrameSnapshot::default()
        };
        assert!(after.pages.planner.area.offset > 0);
        let shift = planner_shift(&before, &after).expect("a pure scroll");
        assert_eq!(shift.shifts()[0], after.pages.planner.area.offset);
    }
}
