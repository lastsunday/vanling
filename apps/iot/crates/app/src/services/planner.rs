//! The planner-widget page: a month grid, a draggable hour wheel, a star row, an HSV picker,
//! and the date/time picker dialogs. Every mark is one canvas slot whose draw and hit test
//! share one geometry.

use iot_core::drivers::input::{GestureEvent, InputEvent};
use iot_core::ui::panel::{CONTENT_BOTTOM, PANEL_COLUMNS};
use iot_core::ui::text::format_u16;
use iot_core::ui::{Canvas, Rect, Screen, ScrollId, Style, Ui};

use crate::shared::i18n::Msg;
use crate::shared::intent::{BusinessIntent, GalleryIntent, InputIntent};
use crate::shared::model::{Command, DeviceState, Effects};
use crate::shared::page;
use crate::shared::route::PageId;
use crate::shared::snapshot::{
    FrameSnapshot, PageSnapshot, PickerOpen, PickerView, PlannerSnapshot, ScrollShift,
};
use crate::shared::surface::{finish, headless};
use crate::shared::ui::scroll::{BAR, Drag, ScrollConfig};
use crate::shared::ui::tag::TagTable;
use crate::shared::ui::theme::Theme;
use crate::shared::ui::widgets::{
    Button, Picker, WheelState, calendar, color_picker, date_picker, picker, rating, time_picker,
    wheel,
};

use super::gallery::{REPAINT_MS, Scroller};
use super::{Descriptor, Repaint};

const AREA: ScrollId = ScrollId::new(1);
const CONTENT_W: i16 = PANEL_COLUMNS as i16 - page::MARGIN - 2 * BAR;

const CALENDAR_SLOT: u8 = 0;
const WHEEL_SLOT: u8 = 1;
const RATING_SLOT: u8 = 2;
const PICKER_SLOT: u8 = 3;
const DIALOG_SLOT: u8 = 4;
/// One per dialog wheel column, or the month grid in the first.
const DIALOG_COLUMN: u8 = 5;

/// Sub-cell tags the page computes for itself: a day, an hour, a star.
const TAG_CALENDAR: u16 = 100;
const TAG_WHEEL: u16 = 140;
const TAG_RATING: u16 = 170;
const TAG_PICKER: u16 = 180;
const TAG_DIALOG_OK: u16 = 200;
const TAG_DIALOG_CANCEL: u16 = 201;
const TAG_PREV: u16 = 202;
const TAG_NEXT: u16 = 203;
const TAG_DAY: u16 = 210;

const AREA_INLINE: u8 = 0;
const AREA_DATE: u8 = 1;
const AREA_TIME: u8 = 2;

/// The full-panel rect the dialog slot covers, and the rect the dialog's card is drawn in.
const DIALOG_RECT: Rect = Rect::new(0, 0, PANEL_COLUMNS as i32, CONTENT_BOTTOM as i32);

const WHEEL_W: i16 = 56;
const WHEEL_H: i16 = 132;

const SCROLL: ScrollConfig = ScrollConfig {
    bounce: true,
    bar: true,
    pull: 0,
};

/// The controls the declaration draws; the dialog's own targets are hit-tested inside its slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
enum Action {
    Prev = 1,
    Next,
    DateOpen,
    TimeOpen,
}

impl Action {
    const ALL: [Self; 4] = [Self::Prev, Self::Next, Self::DateOpen, Self::TimeOpen];

    const fn tag(self) -> u16 {
        self as u16
    }

    fn from_tag(tag: u16) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.tag() == tag)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WheelDrag {
    id: u8,
    area: u8,
    column: u8,
    drag: Drag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerPage {
    scroll: Scroller,
    tags: TagTable<{ Action::ALL.len() + 4 }>,
    calendar: Rect,
    wheel: Rect,
    stars: Rect,
    picker: Rect,
    /// The full-panel rect the dialog slot shares.
    overlay: Rect,
    /// The finger that is painting the picker, so its drags do not scroll the page.
    picker_drag: Option<u8>,
    wheel_drag: Option<WheelDrag>,
    inline: WheelState,
    date: date_picker::DateWheels,
    month: date_picker::MonthView,
    time: time_picker::TimeState,
    dialog: PickerOpen,
    values: PlannerSnapshot,
}

impl Default for PlannerPage {
    fn default() -> Self {
        let mut ui = headless();
        declare(&FrameSnapshot::default(), &mut ui);
        finish(&mut ui);
        let mut scroll = Scroller::default();
        scroll.measure(&ui, AREA, SCROLL);
        let values = PlannerSnapshot::default();
        Self {
            scroll,
            tags: TagTable::measure(&ui),
            calendar: ui.paint_rect(CALENDAR_SLOT).unwrap_or_default(),
            wheel: ui.paint_rect(WHEEL_SLOT).unwrap_or_default(),
            stars: ui.paint_rect(RATING_SLOT).unwrap_or_default(),
            picker: ui.paint_rect(PICKER_SLOT).unwrap_or_default(),
            overlay: ui.paint_rect(DIALOG_SLOT).unwrap_or_default(),
            picker_drag: None,
            wheel_drag: None,
            inline: WheelState::new(u16::from(values.hour), 24, true),
            date: date_picker::DateWheels::new(values.year, values.month, values.day),
            month: date_picker::MonthView {
                year: values.year,
                month: values.month,
                day: values.day,
            },
            time: time_picker::TimeState::new(values.hour, values.minute, values.second, 3),
            dialog: PickerOpen::None,
            values,
        }
    }
}

impl PlannerPage {
    /// Puts the page at `offset`, for tests that need a control below the fold on screen.
    #[cfg(test)]
    pub(crate) fn park(&mut self, offset: i16) {
        self.scroll.park(offset);
    }

    fn tag_at(&self, x: i32, y: i32) -> u16 {
        self.scroll
            .tag_at(|x, y, offset| self.tags.at(x, y, offset), x, y)
    }

    pub(crate) fn snapshot(&self) -> PlannerSnapshot {
        let mut view = self.values;
        view.area = self.scroll.view();
        view.moving = self.scroll.moving();
        view.hour = self.inline.index() as u8;
        view.inline = self.inline.view();
        view.dialog = PickerView {
            open: self.dialog,
            date: self.date.views(),
            month: self.month,
            time: self.time.view(),
            moving: self.inline.moving() || self.date.moving() || self.time.moving(),
        };
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
        if let Some(star) = tag.checked_sub(TAG_RATING).filter(|star| *star < 5) {
            self.values.rating = star as u8 + 1;
            return;
        }
        match Action::from_tag(tag) {
            Some(Action::Prev) => self.shift_month(-1),
            Some(Action::Next) => self.shift_month(1),
            Some(Action::DateOpen) => self.open_date(),
            Some(Action::TimeOpen) => self.open_time(),
            None => {}
        }
    }

    fn shift_month(&mut self, delta: i32) {
        let mut view = date_picker::MonthView {
            year: self.values.year,
            month: self.values.month,
            day: self.values.day,
        };
        date_picker::shift_month(&mut view, delta);
        self.values.year = view.year;
        self.values.month = view.month;
        self.values.day = view.day;
    }

    fn open_date(&mut self) {
        self.picker_drag = None;
        self.wheel_drag = None;
        self.date =
            date_picker::DateWheels::new(self.values.year, self.values.month, self.values.day);
        self.month = date_picker::MonthView {
            year: self.values.year,
            month: self.values.month,
            day: self.values.day,
        };
        self.dialog = match date_picker::kind(self.overlay) {
            date_picker::Kind::Month => PickerOpen::Month,
            date_picker::Kind::Wheels => PickerOpen::Date,
        };
    }

    fn open_time(&mut self) {
        self.picker_drag = None;
        self.wheel_drag = None;
        self.time = time_picker::TimeState::new(
            self.inline.index() as u8,
            self.values.minute,
            self.values.second,
            3,
        );
        self.dialog = PickerOpen::Time;
    }

    fn commit(&mut self) {
        match self.dialog {
            PickerOpen::Date => {
                let (year, month, day) = self.date.date();
                self.values.year = year;
                self.values.month = month;
                self.values.day = day;
            }
            PickerOpen::Month => {
                self.values.year = self.month.year;
                self.values.month = self.month.month;
                self.values.day = self.month.day;
            }
            PickerOpen::Time => {
                let (hour, minute, second) = self.time.value();
                self.inline.set_index(u16::from(hour));
                self.values.minute = minute;
                self.values.second = second;
            }
            PickerOpen::None => {}
        }
        self.dialog = PickerOpen::None;
    }

    fn tap_dialog(&mut self, tag: u16) {
        match tag {
            TAG_DIALOG_OK => self.commit(),
            TAG_DIALOG_CANCEL => self.dialog = PickerOpen::None,
            TAG_PREV if self.dialog == PickerOpen::Month => {
                date_picker::shift_month(&mut self.month, -1);
            }
            TAG_NEXT if self.dialog == PickerOpen::Month => {
                date_picker::shift_month(&mut self.month, 1);
            }
            _ if self.dialog == PickerOpen::Month => {
                let days = calendar::days_in_month(self.month.year, self.month.month);
                if let Some(day) = tag
                    .checked_sub(TAG_DAY)
                    .filter(|day| *day <= u16::from(days))
                {
                    self.month.day = day as u8;
                }
            }
            _ => {}
        }
    }

    fn dialog_tag(&self, x: i32, y: i32) -> Option<u16> {
        let body = picker::frame(self.overlay).body;
        if self.dialog == PickerOpen::Month {
            if let Some(day) = date_picker::day_at(body, self.month, x, y) {
                return Some(TAG_DAY + u16::from(day));
            }
            if let Some(direction) = date_picker::nav_at(body, x, y) {
                return Some(if direction < 0 { TAG_PREV } else { TAG_NEXT });
            }
        }
        match picker::action_at(self.overlay, x, y) {
            Some(true) => Some(TAG_DIALOG_OK),
            Some(false) => Some(TAG_DIALOG_CANCEL),
            None => None,
        }
    }

    fn inline_point(&self, x: u16, y: u16) -> Option<(u8, i16)> {
        let area = self.scroll.area();
        let (x, y) = (i32::from(x), i32::from(y));
        if !area.rect.contains(x, y) {
            return None;
        }
        let content = y + i32::from(area.offset);
        let column = wheel::column_at(self.wheel, 1, x, content)?;
        Some((
            column,
            content.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
        ))
    }

    fn wheel_rect(&self, area: u8, column: u8) -> Rect {
        match area {
            AREA_INLINE => self.wheel,
            AREA_DATE => date_picker::column_rect(picker::frame(self.overlay).body, column),
            AREA_TIME => {
                let body = picker::frame(self.overlay).body;
                time_picker::column_rect(body, self.time.columns(), column)
            }
            _ => Rect::default(),
        }
    }

    fn wheel_row_h(&self, area: u8, column: u8) -> i16 {
        wheel::metrics(self.wheel_rect(area, column)).row_h
    }

    fn wheel_view(&self, area: u8, column: u8) -> Option<wheel::WheelView> {
        match area {
            AREA_INLINE => Some(self.inline.view()),
            AREA_DATE => self.date.views().get(usize::from(column)).copied(),
            AREA_TIME => self.time.view().wheels.get(usize::from(column)).copied(),
            _ => None,
        }
    }

    fn wheel_press(&mut self, area: u8, column: u8) {
        match area {
            AREA_INLINE => self.inline.press(),
            AREA_DATE => self.date.press(column),
            AREA_TIME => self.time.press(column),
            _ => {}
        }
    }

    fn wheel_drag_by(&mut self, area: u8, column: u8, dy: i16) {
        let row_h = self.wheel_row_h(area, column);
        match area {
            AREA_INLINE => self.inline.drag(dy, row_h),
            AREA_DATE => self.date.drag(column, dy, row_h),
            AREA_TIME => self.time.drag(column, dy, row_h),
            _ => {}
        }
    }

    fn wheel_release(&mut self, area: u8, column: u8, speed: i16) {
        let row_h = self.wheel_row_h(area, column);
        match area {
            AREA_INLINE => self.inline.release(speed, row_h),
            AREA_DATE => self.date.release(column, speed, row_h),
            AREA_TIME => self.time.release(column, speed, row_h),
            _ => {}
        }
    }

    fn wheel_tap(&mut self, area: u8, column: u8, y: i16) {
        let rect = self.wheel_rect(area, column);
        let Some(view) = self.wheel_view(area, column) else {
            return;
        };
        let Some(offset) = wheel::row_at(rect, view, rect.x + rect.w / 2, i32::from(y)) else {
            return;
        };
        match area {
            AREA_INLINE => self.inline.tap_row(offset),
            AREA_DATE => self.date.tap_row(column, offset),
            AREA_TIME => self.time.tap_row(column, offset),
            _ => {}
        }
    }

    fn wheel_step(&mut self, id: u8, area: u8, column: u8, y: i16, at_ms: u32) {
        if let Some(drag) = self.wheel_drag.as_mut().filter(|drag| drag.id == id) {
            let dy = drag.drag.step(y, at_ms);
            let (area, column) = (drag.area, drag.column);
            self.wheel_drag_by(area, column, dy);
        } else {
            self.wheel_drag = Some(WheelDrag {
                id,
                area,
                column,
                drag: Drag::new(id, 0, y, at_ms),
            });
            self.wheel_press(area, column);
        }
    }

    fn wheel_end(&mut self, id: u8, y: i16, at_ms: u32) {
        let Some(mut drag) = self.wheel_drag.take_if(|drag| drag.id == id) else {
            return;
        };
        let (area, column) = (drag.area, drag.column);
        if drag.drag.moved {
            let dy = drag.drag.step(y, at_ms);
            self.wheel_drag_by(area, column, dy);
            let speed = drag.drag.release(Some(at_ms));
            self.wheel_release(area, column, speed);
        } else {
            self.wheel_tap(area, column, y);
        }
    }

    fn wheel_end_tag(&self, id: u8, y: u16, at_ms: u32) -> Option<GalleryIntent> {
        let drag = self.wheel_drag.filter(|drag| drag.id == id)?;
        let y = match drag.area {
            AREA_INLINE => {
                let offset = self.scroll.area().offset;
                (i32::from(y) + i32::from(offset)).clamp(i32::from(i16::MIN), i32::from(i16::MAX))
                    as i16
            }
            _ => i16::try_from(y).unwrap_or(i16::MAX),
        };
        Some(GalleryIntent::WheelEnd {
            id,
            area: drag.area,
            column: drag.column,
            y,
            at_ms,
        })
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
                if self.dialog == PickerOpen::None {
                    self.tap_tag(tag);
                } else {
                    self.tap_dialog(tag);
                }
            }
            GalleryIntent::Point { id, tag, x, y } if tag == TAG_PICKER => {
                self.picker_drag = Some(id);
                self.sample_picker(x, y);
            }
            GalleryIntent::Wheel {
                id,
                area,
                column,
                y,
                at_ms,
            } => self.wheel_step(id, area, column, y, at_ms),
            GalleryIntent::WheelEnd { id, y, at_ms, .. } => self.wheel_end(id, y, at_ms),
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
        if self.dialog != PickerOpen::None {
            let (area, columns) = match self.dialog {
                PickerOpen::Date => (AREA_DATE, date_picker::COLUMNS),
                PickerOpen::Time => (AREA_TIME, self.time.columns()),
                _ => (0, 0),
            };
            let body = picker::frame(self.overlay).body;
            match op.event {
                InputEvent::Gesture(GestureEvent::Press { id, x, y }) if columns > 0 => {
                    let column = wheel::column_at(body, columns, i32::from(x), i32::from(y))?;
                    return gallery(GalleryIntent::Wheel {
                        id,
                        area,
                        column,
                        y: i16::try_from(y).unwrap_or(i16::MAX),
                        at_ms: 0,
                    });
                }
                InputEvent::Gesture(GestureEvent::Drag { id, y, at_ms, .. }) if columns > 0 => {
                    let drag = self.wheel_drag.filter(|drag| drag.id == id)?;
                    return gallery(GalleryIntent::Wheel {
                        id,
                        area: drag.area,
                        column: drag.column,
                        y: i16::try_from(y).unwrap_or(i16::MAX),
                        at_ms,
                    });
                }
                InputEvent::Gesture(GestureEvent::Tap {
                    id, x, y, at_ms, ..
                }) => {
                    if let Some(intent) = self.wheel_end_tag(id, y, at_ms) {
                        return gallery(intent);
                    }
                    return self
                        .dialog_tag(i32::from(x), i32::from(y))
                        .and_then(|tag| gallery(GalleryIntent::Tap { tag }));
                }
                InputEvent::Gesture(GestureEvent::Swipe {
                    id, end_y, at_ms, ..
                }) => {
                    if let Some(intent) = self.wheel_end_tag(id, end_y, at_ms) {
                        return gallery(intent);
                    }
                    return None;
                }
                _ => return None,
            }
        }
        if let Some(drag) = self.wheel_drag {
            match op.event {
                InputEvent::Gesture(GestureEvent::Drag { id, y, at_ms, .. }) if drag.id == id => {
                    let offset = self.scroll.area().offset;
                    let y = (i32::from(y) + i32::from(offset))
                        .clamp(i32::from(i16::MIN), i32::from(i16::MAX))
                        as i16;
                    return gallery(GalleryIntent::Wheel {
                        id,
                        area: drag.area,
                        column: drag.column,
                        y,
                        at_ms,
                    });
                }
                InputEvent::Gesture(GestureEvent::Tap { id, y, at_ms, .. }) if drag.id == id => {
                    return self.wheel_end_tag(id, y, at_ms).and_then(gallery);
                }
                InputEvent::Gesture(GestureEvent::Swipe {
                    id, end_y, at_ms, ..
                }) if drag.id == id => {
                    return self.wheel_end_tag(id, end_y, at_ms).and_then(gallery);
                }
                _ => {}
            }
        }
        match op.event {
            InputEvent::Gesture(GestureEvent::Press { id, x, y }) => {
                if let Some((column, content)) = self.inline_point(x, y) {
                    return gallery(GalleryIntent::Wheel {
                        id,
                        area: AREA_INLINE,
                        column,
                        y: content,
                        at_ms: 0,
                    });
                }
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
        self.inline.tick(step_ms);
        self.date.tick(step_ms);
        self.time.tick(step_ms);
    }

    fn on_hide(&mut self) {
        self.scroll.reset();
        self.picker_drag = None;
        self.wheel_drag = None;
        self.dialog = PickerOpen::None;
    }
}

fn declare(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
    let lang = snapshot.lang;
    let w = snapshot.pages.planner;
    let theme = *Theme::of(snapshot.theme);
    let mut date = [0u8; 12];
    let mut time = [0u8; 10];
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
            ui.custom_state(
                Style::new()
                    .width(CONTENT_W)
                    .height(calendar::HEIGHT)
                    .tag(TAG_CALENDAR),
                CALENDAR_SLOT,
                month_state(w.year, w.month, w.day),
            );
            page::caption(ui, Msg::CapTimePicker.bytes(lang), &theme);
            ui.custom_state(
                Style::new().width(WHEEL_W).height(WHEEL_H).tag(TAG_WHEEL),
                WHEEL_SLOT,
                w.inline.state(),
            );
            page::caption(ui, Msg::CapDatetime.bytes(lang), &theme);
            ui.row(Style::new().gap(theme.gap), |ui| {
                ui.add(
                    Button::new(Msg::Date.bytes(lang))
                        .width((CONTENT_W - theme.gap) / 2)
                        .tag(Action::DateOpen.tag()),
                );
                ui.add(
                    Button::new(Msg::Time.bytes(lang))
                        .width((CONTENT_W - theme.gap) / 2)
                        .tag(Action::TimeOpen.tag()),
                );
            });
            page::reading(
                ui,
                Msg::Date.bytes(lang),
                date_text(w.year, w.month, w.day, &mut date),
            );
            page::reading(
                ui,
                Msg::Time.bytes(lang),
                time_text(w.hour, w.minute, w.second, &mut time),
            );
            page::caption(ui, Msg::CapRating.bytes(lang), &theme);
            ui.custom_state(
                Style::new()
                    .width(rating::width(5))
                    .height(rating::HEIGHT)
                    .tag(TAG_RATING),
                RATING_SLOT,
                u32::from(w.rating),
            );
            page::caption(ui, Msg::CapColorPicker.bytes(lang), &theme);
            ui.custom_state(
                Style::new()
                    .width(color_picker::WIDTH)
                    .height(color_picker::HEIGHT)
                    .tag(TAG_PICKER),
                PICKER_SLOT,
                (u32::from(w.hue) << 16) | (u32::from(w.sat) << 8) | u32::from(w.val),
            );
        });
    });
    declare_dialog(snapshot, ui);
}

/// The card and one slot per body column: the card keeps its pixels while a wheel turns, and
/// each column names only the rows it draws.
fn declare_dialog(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
    let w = snapshot.pages.planner;
    let open = w.dialog.open;
    ui.custom_state(
        dialog_style(DIALOG_RECT),
        DIALOG_SLOT,
        (open as u32) | ((snapshot.lang as u32) << 8),
    );
    let body = picker::frame(DIALOG_RECT).body;
    match open {
        PickerOpen::Time => {
            let columns = w.dialog.time.columns;
            for column in 0..columns {
                let rect = time_picker::column_rect(body, columns, column);
                ui.custom_state(
                    dialog_style(rect).cover(),
                    DIALOG_COLUMN + column,
                    w.dialog.time.wheels[usize::from(column)].state(),
                );
            }
        }
        PickerOpen::Date => {
            for column in 0..date_picker::COLUMNS {
                let rect = date_picker::column_rect(body, column);
                ui.custom_state(
                    dialog_style(rect).cover(),
                    DIALOG_COLUMN + column,
                    w.dialog.date[usize::from(column)].state(),
                );
            }
        }
        PickerOpen::Month => {
            ui.custom_state(
                dialog_style(body).cover(),
                DIALOG_COLUMN,
                month_state(
                    w.dialog.month.year,
                    w.dialog.month.month,
                    w.dialog.month.day,
                ),
            );
        }
        PickerOpen::None => {}
    }
}

fn dialog_style(rect: Rect) -> Style {
    Style::new()
        .absolute()
        .left(rect.x as i16)
        .top(rect.y as i16)
        .width(rect.w as i16)
        .height(rect.h as i16)
}

fn month_state(year: u16, month: u8, day: u8) -> u32 {
    (u32::from(year) << 16) | (u32::from(month) << 8) | u32::from(day)
}

fn date_text(year: u16, month: u8, day: u8, buf: &mut [u8; 12]) -> &[u8] {
    let mut digits = [0u8; 6];
    let text = format_u16(year, &mut digits);
    let mut at = text.len().min(buf.len());
    buf[..at].copy_from_slice(&text[..at]);
    for part in [month, day] {
        if at >= buf.len() {
            break;
        }
        buf[at] = b'-';
        at += 1;
        at = push_two(buf, at, part);
    }
    &buf[..at]
}

fn time_text(hour: u8, minute: u8, second: u8, buf: &mut [u8; 10]) -> &[u8] {
    let mut at = push_two(buf, 0, hour);
    for part in [minute, second] {
        if at >= buf.len() {
            break;
        }
        buf[at] = b':';
        at += 1;
        at = push_two(buf, at, part);
    }
    &buf[..at]
}

fn push_two(buf: &mut [u8], at: usize, value: u8) -> usize {
    if at + 2 > buf.len() {
        return at;
    }
    buf[at] = b'0' + value / 10;
    buf[at + 1] = b'0' + value % 10;
    at + 2
}

fn custom(snapshot: &FrameSnapshot, screen: Screen, slot: u8, canvas: &mut Canvas<'_>, rect: Rect) {
    let theme = *Theme::of(screen.theme);
    let w = snapshot.pages.planner;
    match slot {
        CALENDAR_SLOT => calendar::draw(canvas, rect, w.year, w.month, Some(w.day), &theme),
        WHEEL_SLOT => wheel::draw(canvas, rect, w.inline, time_picker::HOUR_LABELS, &theme),
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
        DIALOG_SLOT => draw_card(snapshot, canvas, rect, &theme),
        DIALOG_COLUMN.. => draw_dialog_body(snapshot, canvas, rect, slot - DIALOG_COLUMN, &theme),
    }
}

fn draw_card(snapshot: &FrameSnapshot, canvas: &mut Canvas<'_>, rect: Rect, theme: &Theme) {
    let w = snapshot.pages.planner;
    let lang = snapshot.lang;
    let title = match w.dialog.open {
        PickerOpen::None => return,
        PickerOpen::Date | PickerOpen::Month => Msg::PickerDate.bytes(lang),
        PickerOpen::Time => Msg::PickerTime.bytes(lang),
    };
    picker::draw(
        canvas,
        rect,
        title,
        Msg::Cancel.bytes(lang),
        Msg::Ok.bytes(lang),
        theme,
    );
}

/// One dialog column, card surface first: a frame that repaints only this column starts from
/// the ground, and the column owns every pixel it covers.
fn draw_dialog_body(
    snapshot: &FrameSnapshot,
    canvas: &mut Canvas<'_>,
    rect: Rect,
    column: u8,
    theme: &Theme,
) {
    let w = snapshot.pages.planner;
    let body = picker::frame(DIALOG_RECT).body;
    canvas.push_clip(rect);
    canvas.set_color(Some(theme.surface));
    canvas.fill_rect(rect);
    match w.dialog.open {
        PickerOpen::Time => time_picker::draw_column(canvas, body, w.dialog.time, column, theme),
        PickerOpen::Date => date_picker::draw_column(canvas, body, w.dialog.date, column, theme),
        PickerOpen::Month => date_picker::draw_month(canvas, body, w.dialog.month, theme),
        PickerOpen::None => {}
    }
    canvas.set_color(None);
    canvas.pop_clip();
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
    use crate::shared::ui::widgets::WheelView;
    use alloc::vec::Vec;
    use iot_core::drivers::surface::{DamageRect, FrameDamage};
    use iot_core::ui::{BoxConstraints, FramePrint, PX, Source};

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

    fn press_and_tap(page: &mut PlannerPage, x: u16, y: u16) {
        send(page, GestureEvent::Press { id: 0, x, y });
        send(
            page,
            GestureEvent::Tap {
                id: 0,
                x,
                y,
                held_ms: 40,
                at_ms: 40,
            },
        );
    }

    fn press_drag_swipe(page: &mut PlannerPage, x: u16, start: u16, end: u16) {
        send(page, GestureEvent::Press { id: 0, x, y: start });
        send(
            page,
            GestureEvent::Drag {
                id: 0,
                x,
                y: end,
                at_ms: 20,
            },
        );
        send(
            page,
            GestureEvent::Swipe {
                id: 0,
                direction: iot_core::drivers::input::SwipeDirection::Up,
                x,
                y: start,
                end_x: x,
                end_y: end,
                held_ms: 40,
                distance_px: start.abs_diff(end),
                at_ms: 40,
            },
        );
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
        let expected: Vec<u16> = [
            Action::Prev.tag(),
            Action::Next.tag(),
            TAG_CALENDAR,
            TAG_WHEEL,
            Action::DateOpen.tag(),
            Action::TimeOpen.tag(),
            TAG_RATING,
            TAG_PICKER,
        ]
        .to_vec();
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
        let (x, y) = (
            u16::try_from(cell.x + 4).unwrap(),
            u16::try_from(cell.y + 4 - i32::from(offset)).unwrap(),
        );
        press_and_tap(&mut page, x, y);
        assert_eq!(page.values.day, 15);
    }

    #[test]
    fn a_tap_on_the_inline_wheel_picks_its_row() {
        let mut page = PlannerPage::default();
        let area = page.scroll.area();
        let target = page.wheel.y + page.wheel.h / 2 - (area.rect.y + area.rect.h / 2);
        page.scroll.park(i16::try_from(target.max(0)).unwrap_or(0));
        let offset = page.scroll.area().offset;
        let m = wheel::metrics(page.wheel);
        let x = u16::try_from(page.wheel.x + page.wheel.w / 2).unwrap();
        let y = u16::try_from(m.centre + i32::from(m.row_h) - i32::from(offset)).unwrap();
        press_and_tap(&mut page, x, y);
        assert_eq!(page.inline.index(), 10, "one row below nine");
    }

    #[test]
    fn a_swipe_on_the_inline_wheel_keeps_the_rows_it_crossed() {
        let mut page = PlannerPage::default();
        let area = page.scroll.area();
        let target = page.wheel.y + page.wheel.h / 2 - (area.rect.y + area.rect.h / 2);
        page.scroll.park(i16::try_from(target.max(0)).unwrap_or(0));
        let offset = page.scroll.area().offset;
        let m = wheel::metrics(page.wheel);
        let x = u16::try_from(page.wheel.x + page.wheel.w / 2).unwrap();
        let start = u16::try_from(m.centre - i32::from(offset)).unwrap();
        let end = u16::try_from(m.centre - i32::from(offset) - i32::from(m.row_h)).unwrap();
        press_drag_swipe(&mut page, x, start, end);
        assert_eq!(page.inline.index(), 10, "the lift keeps the crossed row");
        assert_eq!(page.snapshot().hour, 10, "the reading follows the wheel");
    }

    /// The damage a fresh frame of `after` needs against `before`, from the framework's
    /// fingerprints: the page names no rectangle itself.
    fn damage_between(before: &FrameSnapshot, after: &FrameSnapshot) -> FrameDamage {
        let tight = BoxConstraints::tight(PANEL_COLUMNS as i32 * PX, CONTENT_BOTTOM as i32 * PX);
        let mut first = FramePrint::new();
        {
            let mut ui = headless();
            declare(before, &mut ui);
            let mut ignore = |_: u8, _: &mut Canvas<'_>, _: Rect| {};
            ui.paint_measured(
                tight,
                &mut first,
                Source::Full,
                FrameDamage::NONE,
                &mut ignore,
            );
        }
        let mut next = FramePrint::new();
        let mut ui = headless();
        declare(after, &mut ui);
        let mut ignore = |_: u8, _: &mut Canvas<'_>, _: Rect| {};
        let mut probe = next;
        ui.paint_measured(
            tight,
            &mut probe,
            Source::Full,
            FrameDamage::NONE,
            &mut ignore,
        );
        assert!(
            format!("{first:?}") != format!("{probe:?}"),
            "no fingerprint moved"
        );
        let mut ui = headless();
        declare(after, &mut ui);
        ui.paint_measured(
            tight,
            &mut next,
            Source::Diff {
                prev: Some(&first),
                moved: true,
            },
            FrameDamage::NONE,
            &mut ignore,
        )
    }

    #[test]
    fn a_wheel_frame_repaints_only_what_moved() {
        let mut page = PlannerPage::default();
        page.park(300);
        let base = page.snapshot();
        let frame = |dialog: PickerOpen, minute: u16, hour: u8| {
            let mut planner = base;
            planner.dialog.open = dialog;
            planner.dialog.time = time_picker::TimeView {
                wheels: [
                    WheelView {
                        position: 9 * 256,
                        count: 24,
                        wrap: true,
                        moving: false,
                    },
                    WheelView {
                        position: i32::from(minute) * 256,
                        count: 60,
                        wrap: true,
                        moving: false,
                    },
                    WheelView {
                        position: 30 * 256,
                        count: 60,
                        wrap: true,
                        moving: false,
                    },
                ],
                columns: 3,
            };
            planner.hour = hour;
            planner.inline = WheelView {
                position: i32::from(hour) * 256,
                count: 24,
                wrap: true,
                moving: false,
            };
            let mut pages = PageSnapshot::default();
            pages.planner = planner;
            FrameSnapshot {
                page: PageId::Planner,
                pages,
                ..FrameSnapshot::default()
            }
        };

        let body = picker::frame(DIALOG_RECT).body;
        let column = time_picker::column_rect(body, 3, 1);
        let dialog = damage_between(
            &frame(PickerOpen::Time, 5, 9),
            &frame(PickerOpen::Time, 6, 9),
        );
        assert_eq!(
            dialog.rects(),
            &[DamageRect::new(
                column.x as u16,
                column.y as u16,
                column.w as u16,
                column.h as u16
            )],
            "only the minute column's pixels moved"
        );

        let inline = damage_between(
            &frame(PickerOpen::None, 5, 9),
            &frame(PickerOpen::None, 5, 10),
        );
        assert_eq!(inline.rects().len(), 2, "the wheel and the reading line");
        let area = base.area.rect;
        for rect in inline.rects() {
            assert!(
                area.contains(i32::from(rect.x), i32::from(rect.y)),
                "the wheel's marks stay inside the viewport: {rect:?}"
            );
        }
        let painted: u32 = inline
            .rects()
            .iter()
            .map(|rect| u32::from(rect.w) * u32::from(rect.h))
            .sum();
        assert!(
            painted < (PANEL_COLUMNS * CONTENT_BOTTOM) as u32 / 4,
            "the frame is a corner of the page, not the page"
        );

        assert!(
            damage_between(
                &frame(PickerOpen::None, 5, 9),
                &frame(PickerOpen::Time, 5, 9)
            )
            .is_full(),
            "opening the dialog changes the tree, so every pixel is painted"
        );
    }

    #[test]
    fn a_swipe_on_a_date_wheel_moves_that_column() {
        let mut page = PlannerPage::default();
        tap(&mut page, Action::DateOpen.tag());
        let body = picker::frame(page.overlay).body;
        let month = date_picker::column_rect(body, 1);
        let m = wheel::metrics(month);
        let x = u16::try_from(month.x + month.w / 2).unwrap();
        let start = u16::try_from(m.centre + i32::from(m.row_h)).unwrap();
        let end = u16::try_from(m.centre).unwrap();
        press_drag_swipe(&mut page, x, start, end);
        assert_eq!(page.date.date().1, 12, "one row up from November");
        tap(&mut page, TAG_DIALOG_OK);
        assert_eq!(page.values.month, 12, "OK commits the date");
    }

    #[test]
    fn a_swipe_on_a_dialog_wheel_moves_that_column() {
        let mut page = PlannerPage::default();
        tap(&mut page, Action::TimeOpen.tag());
        let body = picker::frame(page.overlay).body;
        let minute = time_picker::column_rect(body, 3, 1);
        let m = wheel::metrics(minute);
        let x = u16::try_from(minute.x + minute.w / 2).unwrap();
        let start = u16::try_from(m.centre + i32::from(m.row_h)).unwrap();
        let end = u16::try_from(m.centre).unwrap();
        press_drag_swipe(&mut page, x, start, end);
        assert_eq!(page.time.value().1, 6, "one row up from five");
        assert_eq!(page.snapshot().dialog.time.wheels[1].index(), 6);
        tap(&mut page, TAG_DIALOG_OK);
        assert_eq!(page.values.minute, 6, "OK commits the wheel");
    }

    #[test]
    fn the_stars_and_the_month_buttons_change_their_values() {
        let mut page = PlannerPage::default();
        tap(&mut page, TAG_RATING + 2);
        assert_eq!(page.values.rating, 3);
        tap(&mut page, TAG_RATING + 4);
        assert_eq!(page.values.rating, 5);
    }

    #[test]
    fn the_date_dialog_opens_on_wheels_at_panel_size_and_commits_on_ok() {
        let mut page = PlannerPage::default();
        tap(&mut page, Action::DateOpen.tag());
        assert_eq!(
            page.dialog,
            PickerOpen::Date,
            "the panel body is too short for a grid"
        );
        let body = picker::frame(page.overlay).body;
        let month = date_picker::column_rect(body, 1);
        let m = wheel::metrics(month);
        let x = u16::try_from(month.x + month.w / 2).unwrap();
        let y = u16::try_from(m.centre + i32::from(m.row_h)).unwrap();
        press_and_tap(&mut page, x, y);
        assert_eq!(page.date.date().1, 12, "the month wheel turned once");
        tap(&mut page, TAG_DIALOG_OK);
        assert_eq!(page.dialog, PickerOpen::None);
        assert_eq!(page.values.month, 12);
    }

    #[test]
    fn the_time_dialog_opens_on_three_columns_and_cancels_without_a_commit() {
        let mut page = PlannerPage::default();
        let before = (page.values.minute, page.values.second);
        tap(&mut page, Action::TimeOpen.tag());
        assert_eq!(page.dialog, PickerOpen::Time);
        let body = picker::frame(page.overlay).body;
        let minute = time_picker::column_rect(body, 3, 1);
        let m = wheel::metrics(minute);
        let x = u16::try_from(minute.x + minute.w / 2).unwrap();
        let y = u16::try_from(m.centre + i32::from(m.row_h)).unwrap();
        press_and_tap(&mut page, x, y);
        assert_eq!(page.time.value().1, 6);
        tap(&mut page, TAG_DIALOG_CANCEL);
        assert_eq!(page.dialog, PickerOpen::None);
        assert_eq!((page.values.minute, page.values.second), before);
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
