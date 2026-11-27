//! The data-widget page: a table whose rows pick, a chart that drives the gauge, a sliding row,
//! and the overlay family.

use iot_core::drivers::input::{GestureEvent, InputEvent};
use iot_core::ui::panel::{CONTENT_BOTTOM, PANEL_COLUMNS};
use iot_core::ui::text::format_u16;
use iot_core::ui::{Canvas, Layout, Rect, Screen, ScrollId, Style, Ui};

use crate::shared::i18n::{Lang, Msg};
use crate::shared::intent::{BusinessIntent, GalleryIntent, InputIntent};
use crate::shared::model::{Command, DeviceState, Effects};
use crate::shared::page::{self, CONTENT_W};
use crate::shared::route::PageId;
use crate::shared::snapshot::{DataSnapshot, FrameSnapshot, PageSnapshot};
use crate::shared::surface::{finish, headless};
use crate::shared::ui::scroll::ScrollConfig;
use crate::shared::ui::tag::Tag;
use crate::shared::ui::theme::{ButtonKind, Theme};
use crate::shared::ui::widgets::{
    Button, EmptyState, EmptyTone, PopupMenu, REVEAL_MAX, Sheet, SheetEdge, SwipeAction, Table,
    chart, dialog, gauge, popup_menu, sheet, table,
};

use super::gallery::{REPAINT_MS, Scroller};
use super::{Descriptor, Repaint};

const AREA: ScrollId = ScrollId::new(1);
/// The full-panel rect all three overlay slots declare; the hit tests read the same constant.
const OVERLAY: Rect = Rect::new(0, 0, PANEL_COLUMNS as i32, CONTENT_BOTTOM as i32);

const TABLE_SLOT: u8 = 0;
const CHART_SLOT: u8 = 1;
const GAUGE_SLOT: u8 = 2;
const SWIPE_SLOT: u8 = 3;
const DIALOG_SLOT: u8 = 4;
const MENU_SLOT: u8 = 5;
const SHEET_SLOT: u8 = 6;

const TABLE_H: i16 = table::HEADER_H + 4 * table::ROW_H;
const CHART_H: i16 = page::TOUCH * 2;
const GAUGE_H: i16 = page::TOUCH * 2;

const SCROLL: ScrollConfig = ScrollConfig {
    bounce: true,
    bar: true,
    pull: 0,
};

/// One control; the discriminant is the tag it reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
enum Action {
    Swipe = 1,
    Empty,
    DialogOpen,
    MenuOpen,
    Menu0,
    Menu1,
    Menu2,
    SheetOpen,
    Sheet0,
    Sheet1,
    /// A table row: the row travels in the intent's `value`.
    Table = 60,
    /// A chart column: the column travels in the intent's `value`.
    Chart = 70,
    Gauge = 80,
    DialogOk = 90,
    DialogCancel,
    Dismiss,
}

impl Tag for Action {
    const ALL: &'static [Self] = &[
        Self::Swipe,
        Self::Empty,
        Self::DialogOpen,
        Self::MenuOpen,
        Self::Menu0,
        Self::Menu1,
        Self::Menu2,
        Self::SheetOpen,
        Self::Sheet0,
        Self::Sheet1,
        Self::Table,
        Self::Chart,
        Self::Gauge,
        Self::DialogOk,
        Self::DialogCancel,
        Self::Dismiss,
    ];

    fn tag(self) -> u16 {
        self as u16
    }
}

impl Action {
    /// The controls the tree draws; the menu items and sheet rows are hit-tested inside their
    /// overlay slots instead of declared.
    #[cfg(test)]
    const DECLARED: &'static [Self] = &[
        Self::Table,
        Self::Chart,
        Self::Gauge,
        Self::Swipe,
        Self::Empty,
        Self::DialogOpen,
        Self::MenuOpen,
        Self::SheetOpen,
    ];
}

const MENU_ITEMS: [Msg; 3] = [Msg::Name, Msg::Mail, Msg::Save];
const MENU_ITEMS_ACTION: [Action; 3] = [Action::Menu0, Action::Menu1, Action::Menu2];
const SHEET_ACTIONS: [Action; 2] = [Action::Sheet0, Action::Sheet1];
const SHEET_ROWS: [Msg; 2] = [Msg::Name, Msg::Mail];

fn menu_item(index: u8, lang: Lang) -> &'static [u8] {
    MENU_ITEMS[usize::from(index).min(MENU_ITEMS.len() - 1)].bytes(lang)
}

fn sheet_row(index: u8, lang: Lang) -> &'static [u8] {
    SHEET_ROWS[usize::from(index).min(SHEET_ROWS.len() - 1)].bytes(lang)
}

/// The samples the three data marks draw.
const BARS: [u8; 6] = [30, 55, 20, 80, 45, 65];
const CELLS: [&[u8]; 3] = [b"CPU", b"MEM", b"NET"];
const ROWS: [[&[u8]; 3]; 4] = [
    [b"12", b"40%", b"up"],
    [b"13", b"42%", b"up"],
    [b"14", b"38%", b"idle"],
    [b"15", b"51%", b"dn"],
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPage {
    scroll: Scroller,
    /// The trigger's bottom edge in panel pixels, taken when the menu opens.
    anchor: i16,
    values: DataSnapshot,
}

impl Default for DataPage {
    fn default() -> Self {
        let mut ui = headless();
        declare(&FrameSnapshot::default(), &mut ui);
        finish(&mut ui);
        let mut scroll = Scroller::default();
        scroll.measure(&ui, AREA, SCROLL);
        Self {
            scroll,
            anchor: 0,
            values: DataSnapshot {
                gauge: BARS[0],
                ..DataSnapshot::default()
            },
        }
    }
}

impl DataPage {
    /// The trigger's bottom edge in panel pixels, so the menu hangs under it.
    fn menu_anchor(&self) -> i16 {
        self.anchor
    }

    fn snapshot(&self) -> DataSnapshot {
        let mut view = self.values;
        view.area = self.scroll.view();
        view.moving = self.scroll.moving();
        view.menu_anchor = self.menu_anchor();
        view
    }

    /// The cell of a custom mark a panel point picks.
    fn mark_at(&self, layout: &Layout, x: i32, y: i32) -> Option<(u16, u16)> {
        let row = |rect: Rect, header: i32| -> Option<u8> {
            if !rect.contains(x, y) || y < rect.y + header {
                return None;
            }
            u8::try_from((y - rect.y - header) / i32::from(table::ROW_H)).ok()
        };
        if let Some(row) = layout
            .slot(TABLE_SLOT)
            .and_then(|rect| row(rect, i32::from(table::HEADER_H)))
        {
            return Some((Action::Table.tag(), u16::from(row)));
        }
        if let Some(chart) = layout.slot(CHART_SLOT)
            && chart.contains(x, y)
        {
            let count = i32::try_from(BARS.len()).unwrap_or(1).max(1);
            let column = (x - chart.x) * count / chart.w.max(1);
            let column = u16::try_from(column.clamp(0, count - 1)).unwrap_or(0);
            return Some((Action::Chart.tag(), column));
        }
        if layout
            .slot(GAUGE_SLOT)
            .is_some_and(|gauge| gauge.contains(x, y))
        {
            return Some((Action::Gauge.tag(), 0));
        }
        None
    }

    fn tap(&mut self, tag: u16, value: u16) {
        match Action::from_tag(tag) {
            Some(Action::Gauge) => {
                self.values.gauge = if self.values.gauge >= 100 {
                    0
                } else {
                    (self.values.gauge + 25).min(100)
                };
            }
            Some(Action::Table) if usize::from(value) < ROWS.len() => {
                self.values.table = Some(u8::try_from(value).unwrap_or(0));
            }
            Some(Action::Chart) if usize::from(value) < BARS.len() => {
                let column = u8::try_from(value).unwrap_or(0);
                self.values.bar = Some(column);
                self.values.gauge = BARS[usize::from(column)];
            }
            Some(action) => self.tap_action(action),
            None => {}
        }
    }

    fn tap_action(&mut self, action: Action) {
        match action {
            Action::Swipe => {
                self.values.reveal = if self.values.reveal == 0 {
                    REVEAL_MAX
                } else {
                    0
                };
            }
            Action::Empty => self.values.empty_error = !self.values.empty_error,
            Action::DialogOpen => self.values.dialog = true,
            Action::MenuOpen => self.values.menu_open = true,
            Action::Menu0 => {
                self.values.menu = 0;
                self.values.menu_open = false;
            }
            Action::Menu1 => {
                self.values.menu = 1;
                self.values.menu_open = false;
            }
            Action::Menu2 => {
                self.values.menu = 2;
                self.values.menu_open = false;
            }
            Action::SheetOpen => self.values.sheet_open = true,
            Action::Sheet0 => {
                self.values.sheet = 0;
                self.values.sheet_open = false;
            }
            Action::Sheet1 => {
                self.values.sheet = 1;
                self.values.sheet_open = false;
            }
            Action::Table | Action::Chart | Action::Gauge => {}
            Action::DialogOk | Action::DialogCancel | Action::Dismiss => self.close_overlays(),
        }
    }

    fn close_overlays(&mut self) {
        self.values.dialog = false;
        self.values.menu_open = false;
        self.values.sheet_open = false;
    }
}

impl super::Service for DataPage {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        _state: &mut DeviceState,
        layout: &iot_core::ui::Layout,
    ) -> (Option<Command>, Effects) {
        let BusinessIntent::Gallery { wire, intent } = intent else {
            return (None, Effects::default());
        };
        if *wire != PageId::Data.code() {
            return (None, Effects::default());
        }
        match *intent {
            GalleryIntent::Tap { tag, value } => {
                if tag == Action::MenuOpen.tag() {
                    self.anchor = layout
                        .rect(tag)
                        .map_or(0, |rect| i16::try_from(rect.bottom()).unwrap_or(0));
                }
                self.tap(tag, value);
            }
            GalleryIntent::Slide { dx, .. } => self.values.reveal = (-dx).clamp(0, REVEAL_MAX),
            GalleryIntent::SlideEnd { dx, .. } => {
                self.values.reveal = if dx <= -REVEAL_MAX / 2 { REVEAL_MAX } else { 0 };
            }
            _ => {}
        }
        self.scroll.apply(*intent);
        (None, Effects::default())
    }
}

impl super::LocalPage for DataPage {
    fn bind(
        &self,
        op: &InputIntent,
        _state: &DeviceState,
        layout: &Layout,
    ) -> Option<BusinessIntent> {
        let tap = |intent| {
            Some(BusinessIntent::Gallery {
                wire: PageId::Data.code(),
                intent,
            })
        };
        if self.values.dialog {
            let InputEvent::Gesture(GestureEvent::Tap { x, y, .. }) = op.event else {
                return None;
            };
            let tag = match dialog::action_at(OVERLAY, i32::from(x), i32::from(y)) {
                Some(true) => Action::DialogOk.tag(),
                Some(false) | None => Action::DialogCancel.tag(),
            };
            return tap(GalleryIntent::Tap { tag, value: 0 });
        }
        if self.values.menu_open || self.values.sheet_open {
            let InputEvent::Gesture(GestureEvent::Tap { x, y, .. }) = op.event else {
                return None;
            };
            let (x, y) = (i32::from(x), i32::from(y));
            let tag = if self.values.menu_open {
                popup_menu::item_at(popup_menu::card(OVERLAY, self.menu_anchor()), x, y)
                    .and_then(|row| MENU_ITEMS_ACTION.get(usize::from(row)).copied())
            } else {
                sheet::row_at(sheet::card(OVERLAY, SheetEdge::Bottom), x, y)
                    .and_then(|row| SHEET_ACTIONS.get(usize::from(row)).copied())
            };
            return tap(GalleryIntent::Tap {
                tag: tag.unwrap_or(Action::Dismiss).tag(),
                value: 0,
            });
        }
        if let InputEvent::Gesture(GestureEvent::Tap { x, y, .. }) = op.event
            && !self.scroll.dragging()
        {
            let area = self.scroll.area();
            if area.rect.contains(i32::from(x), i32::from(y))
                && let Some((tag, value)) = self.mark_at(layout, i32::from(x), i32::from(y))
            {
                return tap(GalleryIntent::Tap { tag, value });
            }
        }
        self.scroll.bind(op, PageId::Data.code(), layout)
    }

    fn publish(&self, out: &mut PageSnapshot) {
        out.data = self.snapshot();
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        declare(snapshot, ui);
    }

    fn descriptor() -> Descriptor {
        Descriptor::new(Self::view)
            .with_custom(custom)
            .with_repaint_ms(REPAINT_MS)
            .with_repaints(Repaint::PageOrOverlay)
            .with_scroll()
    }

    fn tick(&mut self, step_ms: u16) {
        self.scroll.tick(step_ms);
    }

    fn on_hide(&mut self) {
        self.scroll.reset();
        self.close_overlays();
    }
}

fn declare(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
    let lang = snapshot.lang;
    let w = snapshot.pages.data;
    let theme = *Theme::of(snapshot.theme);
    page::body(ui, |ui| {
        page::title(ui, Msg::DataTitle.bytes(lang));
        page::scroller(ui, AREA, w.area, |ui| {
            page::caption(ui, Msg::CapTable.bytes(lang), &theme);
            ui.custom(
                Style::new()
                    .width(CONTENT_W)
                    .height(TABLE_H)
                    .tag(Action::Table.tag()),
                TABLE_SLOT,
            );
            page::caption(ui, Msg::CapChart.bytes(lang), &theme);
            ui.custom(
                Style::new()
                    .width(CONTENT_W)
                    .height(CHART_H)
                    .tag(Action::Chart.tag()),
                CHART_SLOT,
            );
            page::caption(ui, Msg::CapGauge.bytes(lang), &theme);
            ui.custom(
                Style::new()
                    .width(CONTENT_W)
                    .height(GAUGE_H)
                    .tag(Action::Gauge.tag()),
                GAUGE_SLOT,
            );
            page::caption(ui, Msg::CapSwipe.bytes(lang), &theme);
            ui.custom(
                Style::new()
                    .width(CONTENT_W)
                    .height(page::TOUCH)
                    .tag(Action::Swipe.tag())
                    .slide(),
                SWIPE_SLOT,
            );
            page::caption(ui, Msg::CapEmpty.bytes(lang), &theme);
            ui.add(
                EmptyState::new(Msg::Empty.bytes(lang), Msg::TapRetry.bytes(lang))
                    .tone(if w.empty_error {
                        EmptyTone::Error
                    } else {
                        EmptyTone::Empty
                    })
                    .width(CONTENT_W)
                    .tag(Action::Empty.tag()),
            );

            page::section(ui, Msg::SectionOverlay.bytes(lang), &theme);
            page::caption(ui, Msg::CapDialog.bytes(lang), &theme);
            ui.add(
                Button::new(Msg::Dialog.bytes(lang))
                    .width(CONTENT_W)
                    .tag(Action::DialogOpen.tag()),
            );
            page::caption(ui, Msg::CapMenu.bytes(lang), &theme);
            ui.add(
                Button::new(Msg::MenuTitle.bytes(lang))
                    .kind(ButtonKind::Outlined)
                    .width(CONTENT_W)
                    .tag(Action::MenuOpen.tag()),
            );
            page::reading(ui, Msg::MenuTitle.bytes(lang), menu_item(w.menu, lang));
            page::caption(ui, Msg::CapSheet.bytes(lang), &theme);
            ui.add(
                Button::new(Msg::SortBy.bytes(lang))
                    .kind(ButtonKind::Outlined)
                    .width(CONTENT_W)
                    .tag(Action::SheetOpen.tag()),
            );
            page::reading(ui, Msg::SortBy.bytes(lang), sheet_row(w.sheet, lang));
        });
    });
    for slot in [MENU_SLOT, SHEET_SLOT, DIALOG_SLOT] {
        ui.custom(
            Style::new()
                .absolute()
                .left(OVERLAY.x as i16)
                .top(OVERLAY.y as i16)
                .width(OVERLAY.w as i16)
                .height(OVERLAY.h as i16),
            slot,
        );
    }
}

fn custom(snapshot: &FrameSnapshot, screen: Screen, slot: u8, canvas: &mut Canvas<'_>, rect: Rect) {
    let theme = *Theme::of(screen.theme);
    let w = snapshot.pages.data;
    let lang = snapshot.lang;
    match slot {
        TABLE_SLOT => {
            if let Some(row) = w.table {
                let top =
                    rect.y + i32::from(table::HEADER_H) + i32::from(row) * i32::from(table::ROW_H);
                canvas.set_color(Some(theme.selection));
                canvas.fill_rect(Rect::new(rect.x, top, rect.w, i32::from(table::ROW_H)));
            }
            Table {
                headers: CELLS,
                rows: ROWS,
            }
            .draw(canvas, rect, &theme);
        }
        CHART_SLOT => {
            chart::draw(canvas, rect, &BARS, &theme);
            if let Some(column) = w.bar {
                let count = i32::try_from(BARS.len()).unwrap_or(1).max(1);
                let slot_w = rect.w / count;
                let left = rect.x + i32::from(column) * slot_w;
                canvas.set_color(Some(theme.foreground));
                canvas.draw_line(
                    left as isize,
                    (rect.bottom() - 1) as isize,
                    (left + slot_w) as isize,
                    (rect.bottom() - 1) as isize,
                );
            }
        }
        GAUGE_SLOT => {
            gauge::draw(canvas, rect, w.gauge, &theme);
            let mut buf = [0u8; 6];
            let text = format_u16(u16::from(w.gauge), &mut buf);
            let width = canvas.measure(text) as i32;
            canvas.set_color(Some(theme.foreground));
            canvas.draw_text(
                text,
                (rect.x + (rect.w - width) / 2).max(0) as usize,
                rect.y.max(0) as usize,
            );
        }
        SWIPE_SLOT => SwipeAction {
            title: Msg::Row.bytes(lang),
            detail: Msg::Detail.bytes(lang),
            mark: '#',
            selected: w.reveal > 0,
        }
        .draw(canvas, rect, w.reveal, Msg::Delete.bytes(lang), &theme),
        DIALOG_SLOT if w.dialog => {
            dialog::Dialog {
                title: Msg::Confirm.bytes(lang),
                body: Msg::DialogBody.bytes(lang),
                ok: Msg::Ok.bytes(lang),
                cancel: Msg::Cancel.bytes(lang),
            }
            .draw(canvas, rect, &theme);
        }
        MENU_SLOT if w.menu_open => PopupMenu {
            items: MENU_ITEMS.map(|item| item.bytes(lang)),
            selected: w.menu,
        }
        .draw(canvas, rect, w.menu_anchor, &theme),
        SHEET_SLOT if w.sheet_open => Sheet {
            title: Msg::SortBy.bytes(lang),
            rows: SHEET_ROWS.map(|row| row.bytes(lang)),
            selected: w.sheet,
        }
        .draw(canvas, rect, SheetEdge::Bottom, &theme),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{LocalPage, Service};
    use crate::shared::i18n::Lang;
    use crate::shared::ui::scroll::LINE_PX as SCROLL_LINE;
    use alloc::vec::Vec;
    use iot_core::ui::panel::CONTENT_BOTTOM;
    use iot_core::ui::{Layout, PX};

    fn layout(page: &DataPage) -> Layout {
        crate::services::layout_of(page, PageId::Data)
    }

    fn tap(page: &mut DataPage, tag: u16) {
        tap_value(page, tag, 0);
    }

    fn tap_value(page: &mut DataPage, tag: u16, value: u16) {
        let mut state = DeviceState::default();
        let intent = BusinessIntent::Gallery {
            wire: PageId::Data.code(),
            intent: GalleryIntent::Tap { tag, value },
        };
        page.apply(&intent, &mut state, &layout(page));
    }

    #[test]
    fn the_whole_gallery_fits_one_declaration() {
        for lang in Lang::ALL {
            let mut ui = headless();
            DataPage::view(
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
    fn every_declared_control_is_drawn_once() {
        let page = DataPage::default();
        let drawn: Vec<u16> = layout(&page)
            .tags()
            .map(|(tag, _)| tag)
            .filter(|tag| Action::from_tag(*tag).is_some())
            .collect();
        let expected: Vec<u16> = Action::DECLARED.iter().map(|action| action.tag()).collect();
        assert_eq!(drawn, expected);
    }

    #[test]
    fn every_canvas_mark_carries_its_own_tag() {
        let mut page = DataPage::default();
        for (tag, slot) in [
            (Action::Table.tag(), TABLE_SLOT),
            (Action::Chart.tag(), CHART_SLOT),
            (Action::Gauge.tag(), GAUGE_SLOT),
        ] {
            scroll_to(&mut page, slot);
            let layout = layout(&page);
            let rect = layout.slot(slot).expect("a mark");
            let visible = rect.intersect(page.scroll.area().rect);
            assert_eq!(
                layout.at(visible.x + 2, visible.y + 2),
                Some(tag),
                "the arena claims the scroll unless the mark is tagged"
            );
        }
    }

    /// Brings a mark to the middle of the gallery, the way a drag would.
    fn scroll_to(page: &mut DataPage, slot: u8) {
        let rect = layout(page).slot(slot).expect("a mark");
        let area = page.scroll.area();
        let rows = rect.y + rect.h / 2 - (area.rect.y + area.rect.h / 2);
        let lines = i8::try_from((rows / i32::from(SCROLL_LINE)).clamp(-127, 127)).unwrap_or(0);
        page.scroll.apply(GalleryIntent::Scroll {
            area: 0,
            lines,
            speed: 0,
        });
    }

    fn tap_at(page: &mut DataPage, x: i32, y: i32) {
        let (x, y) = (u16::try_from(x).unwrap_or(0), u16::try_from(y).unwrap_or(0));
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

    /// The panel sends `Press` on touch-down and `Tap` on the lift; a mark only answers the
    /// second if its press was remembered rather than claimed by the scroll.
    #[test]
    fn a_press_then_a_tap_reaches_the_table_the_chart_and_the_dial() {
        let mut page = DataPage::default();
        let table = layout(&page).slot(TABLE_SLOT).expect("a table");
        let x = table.x + 4;
        let row = table.y + i32::from(table::HEADER_H) + i32::from(table::ROW_H) + 4;
        tap_at(&mut page, x, row);
        assert_eq!(page.values.table, Some(1));

        for slot in [CHART_SLOT, GAUGE_SLOT] {
            let area = page.scroll.area();
            let mark = layout(&page).slot(slot).expect("a mark");
            let content = mark.y + mark.h / 2 + i32::from(area.offset);
            let target = content - (area.rect.y + area.rect.h / 2);
            page.scroll.park(i16::try_from(target.max(0)).unwrap_or(0));
            let mark = layout(&page).slot(slot).expect("a mark");
            tap_at(&mut page, mark.x + mark.w / 2, mark.y + mark.h / 2);
        }
        assert_eq!(page.values.bar, Some(3), "the chart picked its middle bar");
        assert_eq!(
            page.values.gauge,
            (BARS[3] + 25).min(100),
            "the dial took the lift"
        );
    }

    #[test]
    fn the_menu_hangs_under_its_trigger_and_picks_an_item() {
        let mut page = DataPage::default();
        tap(&mut page, Action::MenuOpen.tag());
        assert!(page.values.menu_open);
        let trigger = layout(&page)
            .rect(Action::MenuOpen.tag())
            .expect("a trigger");
        page.scroll
            .park(i16::try_from(trigger.bottom() - 60).unwrap_or(0));
        let anchor = page.snapshot().menu_anchor;
        let card = popup_menu::card(OVERLAY, anchor);
        assert!(
            card.y <= i32::from(anchor),
            "the menu hangs at or above its trigger when the panel runs out"
        );
        assert!(card.bottom() <= CONTENT_BOTTOM as i32);
        let (x, y) = (card.x + 8, card.y + card.h / 2);
        assert_eq!(popup_menu::item_at(card, x, y), Some(1));
        tap_at(&mut page, x, y);
        assert_eq!(page.values.menu, 1);
        assert!(!page.values.menu_open, "a pick closes the menu");
    }

    #[test]
    fn a_tap_outside_the_menu_closes_it_without_picking() {
        let mut page = DataPage::default();
        tap(&mut page, Action::MenuOpen.tag());
        let before = page.values.menu;
        tap_at(&mut page, 4, 4);
        assert!(!page.values.menu_open);
        assert_eq!(page.values.menu, before);
    }

    #[test]
    fn the_sheet_rises_from_the_bottom_and_picks_a_row() {
        let mut page = DataPage::default();
        tap(&mut page, Action::SheetOpen.tag());
        assert!(page.values.sheet_open);
        let card = sheet::card(OVERLAY, SheetEdge::Bottom);
        assert_eq!(card.bottom(), CONTENT_BOTTOM as i32);
        let y = card.bottom() - sheet::ROW_H / 2 - 2;
        let (x, y) = (card.x + 10, y);
        assert_eq!(sheet::row_at(card, x, y), Some(1));
        tap_at(&mut page, x, y);
        assert_eq!(page.values.sheet, 1);
        assert!(!page.values.sheet_open, "a pick closes the sheet");
    }

    #[test]
    fn every_overlay_slot_covers_the_whole_panel() {
        let mut ui = headless();
        declare(&FrameSnapshot::default(), &mut ui);
        finish(&mut ui);
        for slot in [MENU_SLOT, SHEET_SLOT, DIALOG_SLOT] {
            assert_eq!(ui.paint_rect(slot), Some(OVERLAY), "slot {slot}");
        }
    }

    #[test]
    fn a_row_a_bar_and_the_dial_each_pick_their_own() {
        let mut page = DataPage::default();
        let table = layout(&page).slot(TABLE_SLOT).expect("a table");
        let chart = layout(&page).slot(CHART_SLOT).expect("a chart");
        let row = table.y + i32::from(table::HEADER_H) + i32::from(table::ROW_H) * 2 + 4;
        let mark = page.mark_at(&layout(&page), table.x + 4, row);
        assert_eq!(mark, Some((Action::Table.tag(), 2)));
        tap_value(&mut page, Action::Table.tag(), 2);
        assert_eq!(page.values.table, Some(2));

        let mark = page.mark_at(&layout(&page), chart.x + 4, chart.y + 4);
        assert_eq!(mark, Some((Action::Chart.tag(), 0)));
        tap_value(&mut page, Action::Chart.tag(), 0);
        assert_eq!(page.values.bar, Some(0));
        assert_eq!(page.values.gauge, BARS[0]);

        let before = page.values.gauge;
        tap(&mut page, Action::Gauge.tag());
        assert_eq!(page.values.gauge, (before + 25).min(100));
    }

    #[test]
    fn the_dial_cycles_back_to_zero_after_a_full_sweep() {
        let mut page = DataPage::default();
        page.values.gauge = 100;
        tap(&mut page, Action::Gauge.tag());
        assert_eq!(page.values.gauge, 0);
    }

    fn apply(page: &mut DataPage, intent: GalleryIntent) {
        let mut state = DeviceState::default();
        let layout = layout(page);
        page.apply(
            &BusinessIntent::Gallery {
                wire: PageId::Data.code(),
                intent,
            },
            &mut state,
            &layout,
        );
    }

    #[test]
    fn a_horizontal_slide_unveils_the_row_and_lets_it_snap_back() {
        let mut page = DataPage::default();
        apply(
            &mut page,
            GalleryIntent::Slide {
                id: 0,
                tag: Action::Swipe.tag(),
                dx: -50,
            },
        );
        assert_eq!(page.values.reveal, 50);
        apply(
            &mut page,
            GalleryIntent::SlideEnd {
                id: 0,
                tag: Action::Swipe.tag(),
                dx: -50,
            },
        );
        assert_eq!(page.values.reveal, REVEAL_MAX);
        apply(
            &mut page,
            GalleryIntent::SlideEnd {
                id: 0,
                tag: Action::Swipe.tag(),
                dx: -10,
            },
        );
        assert_eq!(page.values.reveal, 0, "a short release snaps back");
    }

    #[test]
    fn the_finger_and_the_declared_controls_agree() {
        let mut page = DataPage::default();
        tap(&mut page, Action::Menu1.tag());
        assert_eq!(page.values.menu, 1);
        tap(&mut page, Action::Sheet0.tag());
        assert_eq!(page.values.sheet, 0);
        tap(&mut page, Action::Empty.tag());
        assert!(page.values.empty_error);
        tap(&mut page, Action::DialogOpen.tag());
        assert!(page.values.dialog);
        tap(&mut page, Action::DialogCancel.tag());
        assert!(!page.values.dialog);
    }

    #[test]
    fn the_dialog_answers_are_touch_sized_and_inside_the_card() {
        let rect = Rect::new(0, 0, PANEL_COLUMNS as i32 - 10, CONTENT_BOTTOM as i32 - 8);
        let card = dialog::card(rect);
        assert!(card.w <= rect.w && card.h <= rect.h);
        let y = card.bottom() - 4;
        assert_eq!(dialog::action_at(rect, card.x + 4, y), Some(false));
        assert_eq!(dialog::action_at(rect, card.right() - 4, y), Some(true));
        assert_eq!(dialog::action_at(rect, card.x + card.w / 2, card.y), None);
    }

    #[test]
    fn the_dialog_centres_its_card() {
        let card = dialog::card(OVERLAY);
        assert_eq!(card.x + card.w / 2, PANEL_COLUMNS as i32 / 2);
        assert_eq!(card.y + card.h / 2, CONTENT_BOTTOM as i32 / 2);
    }

    #[test]
    fn four_rows_fit_the_panel_without_a_tall_table() {
        assert_eq!(TABLE_H, 128, "a 16 px header over four 28 px rows");
        assert_eq!(table::ROW_H, page::LINE * 2 + 4);
    }

    /// The lift after a drag arrives as a tap; the page must let the arena answer it.
    #[test]
    fn a_dragged_lift_picks_no_mark_and_the_next_drag_still_scrolls() {
        let mut page = DataPage::default();
        page.scroll.park(60);
        let chart = layout(&page).slot(CHART_SLOT).expect("a chart");
        let x = chart.x + chart.w / 2;
        let area = page.scroll.area();
        let from = chart.y + chart.h / 2;
        let (x, from) = (u16::try_from(x).unwrap(), u16::try_from(from).unwrap());
        let to = from - 30;
        let before = area.offset;
        send(&mut page, GestureEvent::Press { id: 0, x, y: from });
        send(
            &mut page,
            GestureEvent::Drag {
                id: 0,
                x,
                y: to,
                at_ms: 20,
            },
        );
        let moved = page.scroll.area().offset;
        assert!(moved > before, "the first drag scrolls");

        send(
            &mut page,
            GestureEvent::Tap {
                id: 0,
                x,
                y: to,
                held_ms: 40,
                at_ms: 40,
            },
        );
        assert!(page.values.bar.is_none(), "a lift is not a pick");
        assert!(!page.scroll.dragging(), "the arena let the finger go");

        send(&mut page, GestureEvent::Press { id: 1, x, y: to });
        send(
            &mut page,
            GestureEvent::Drag {
                id: 1,
                x,
                y: to - 30,
                at_ms: 20,
            },
        );
        assert!(
            page.scroll.area().offset > moved,
            "the next drag still scrolls"
        );
    }

    fn send(page: &mut DataPage, event: GestureEvent) {
        let op = InputIntent {
            source: 0,
            event: InputEvent::Gesture(event),
        };
        let mut state = DeviceState::default();
        let layout = layout(page);
        if let Some(bound) = page.bind(&op, &state, &layout) {
            let BusinessIntent::Gallery { intent, .. } = bound else {
                panic!("not a gallery intent")
            };
            page.apply(
                &BusinessIntent::Gallery {
                    wire: PageId::Data.code(),
                    intent,
                },
                &mut state,
                &layout,
            );
        }
    }

    #[test]
    fn a_scroll_frame_moves_only_the_gallery() {
        let mut page = DataPage::default();
        let before = FrameSnapshot {
            page: PageId::Data,
            pages: PageSnapshot {
                data: page.snapshot(),
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
            page: PageId::Data,
            pages: PageSnapshot {
                data: page.snapshot(),
                ..PageSnapshot::default()
            },
            ..FrameSnapshot::default()
        };
        assert!(after.pages.data.area.offset > 0);
        let shift = crate::services::gallery::shift(&before, &after).expect("a pure scroll");
        assert_eq!(shift.shifts()[0], after.pages.data.area.offset);
    }
}
