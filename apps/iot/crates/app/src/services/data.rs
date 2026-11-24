//! The data-widget page: a table whose rows pick, a chart that drives the gauge, a sliding row,
//! and the overlay family.

use iot_core::drivers::input::{GestureEvent, InputEvent};
use iot_core::ui::panel::{CONTENT_BOTTOM, PANEL_COLUMNS};
use iot_core::ui::text::format_u16;
use iot_core::ui::{Canvas, Rect, Screen, ScrollId, Style, Ui};

use crate::shared::i18n::Msg;
use crate::shared::intent::{BusinessIntent, GalleryIntent, InputIntent};
use crate::shared::model::{Command, DeviceState, Effects};
use crate::shared::page;
use crate::shared::route::PageId;
use crate::shared::snapshot::{DataSnapshot, FrameSnapshot, PageSnapshot, ScrollShift};
use crate::shared::surface::{finish, headless};
use crate::shared::ui::scroll::{BAR, ScrollConfig};
use crate::shared::ui::tag::TagTable;
use crate::shared::ui::theme::Theme;
use crate::shared::ui::widgets::{
    Button, Divider, EmptyState, EmptyTone, PopupMenu, REVEAL_MAX, Sheet, SheetEdge, SwipeAction,
    Table, chart, dialog, gauge, table,
};

use super::gallery::{REPAINT_MS, Scroller};
use super::{Descriptor, Repaint};

const AREA: ScrollId = ScrollId::new(1);
const CONTENT_W: i16 = PANEL_COLUMNS as i16 - page::MARGIN - 2 * BAR;

const TABLE_SLOT: u8 = 0;
const CHART_SLOT: u8 = 1;
const GAUGE_SLOT: u8 = 2;
const SWIPE_SLOT: u8 = 3;
const DIALOG_SLOT: u8 = 4;

const TABLE_H: i16 = table::HEADER_H + 4 * table::ROW_H;
const CHART_H: i16 = page::TOUCH * 2;
const GAUGE_H: i16 = page::TOUCH * 2;

/// Tags the custom slots compute for themselves: their sub-cells are not declared nodes.
const TAG_TABLE: u16 = 60;
const TAG_CHART: u16 = 70;
const TAG_GAUGE: u16 = 80;
const TAG_DIALOG_OK: u16 = 90;
const TAG_DIALOG_CANCEL: u16 = 91;

const SCROLL: ScrollConfig = ScrollConfig {
    bounce: true,
    bar: true,
    pull: 0,
};

/// One control on the page. The discriminant is the tag the declaration draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
enum Action {
    Swipe = 1,
    Empty,
    DialogOpen,
    Menu0,
    Menu1,
    Menu2,
    Sheet0,
    Sheet1,
}

impl Action {
    const ALL: [Self; 8] = [
        Self::Swipe,
        Self::Empty,
        Self::DialogOpen,
        Self::Menu0,
        Self::Menu1,
        Self::Menu2,
        Self::Sheet0,
        Self::Sheet1,
    ];

    const fn tag(self) -> u16 {
        self as u16
    }

    fn from_tag(tag: u16) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.tag() == tag)
    }
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
    tags: TagTable<{ Action::ALL.len() }>,
    table: Rect,
    chart_rect: Rect,
    gauge_rect: Rect,
    dialog: Rect,
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
            tags: TagTable::measure(&ui),
            table: ui.paint_rect(TABLE_SLOT).unwrap_or_default(),
            chart_rect: ui.paint_rect(CHART_SLOT).unwrap_or_default(),
            gauge_rect: ui.paint_rect(GAUGE_SLOT).unwrap_or_default(),
            dialog: ui.paint_rect(DIALOG_SLOT).unwrap_or_default(),
            values: DataSnapshot {
                gauge: BARS[0],
                ..DataSnapshot::default()
            },
        }
    }
}

impl DataPage {
    fn tag_at(&self, x: i32, y: i32) -> u16 {
        self.scroll
            .tag_at(|x, y, offset| self.tags.at(x, y, offset), x, y)
    }

    fn snapshot(&self) -> DataSnapshot {
        let mut view = self.values;
        view.area = self.scroll.view();
        view.moving = self.scroll.moving();
        view
    }

    /// The cell of a custom mark a panel point picks, in content coordinates.
    fn mark_at(&self, x: i32, y: i32) -> u16 {
        let row = |rect: Rect, header: i32| -> Option<u8> {
            if !rect.contains(x, y) || y < rect.y + header {
                return None;
            }
            u8::try_from((y - rect.y - header) / i32::from(table::ROW_H)).ok()
        };
        if let Some(row) = row(self.table, i32::from(table::HEADER_H)) {
            return TAG_TABLE + u16::from(row);
        }
        if self.chart_rect.contains(x, y) {
            let count = i32::try_from(BARS.len()).unwrap_or(1).max(1);
            let column = (x - self.chart_rect.x) * count / self.chart_rect.w.max(1);
            return TAG_CHART + u16::try_from(column.clamp(0, count - 1)).unwrap_or(0);
        }
        if self.gauge_rect.contains(x, y) {
            return TAG_GAUGE;
        }
        0
    }

    fn tap(&mut self, tag: u16) {
        if tag == TAG_GAUGE {
            self.values.gauge = if self.values.gauge >= 100 {
                0
            } else {
                self.values.gauge + 25
            };
            return;
        }
        if let Some(row) = tag
            .checked_sub(TAG_TABLE)
            .filter(|row| usize::from(*row) < ROWS.len())
        {
            self.values.table = Some(u8::try_from(row).unwrap_or(0));
            return;
        }
        if let Some(column) = tag
            .checked_sub(TAG_CHART)
            .filter(|column| usize::from(*column) < BARS.len())
        {
            let column = u8::try_from(column).unwrap_or(0);
            self.values.bar = Some(column);
            self.values.gauge = BARS[usize::from(column)];
            return;
        }
        if let Some(action) = Action::from_tag(tag) {
            self.tap_action(action);
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
            Action::Menu0 => self.values.menu = 0,
            Action::Menu1 => self.values.menu = 1,
            Action::Menu2 => self.values.menu = 2,
            Action::Sheet0 => self.values.sheet = 0,
            Action::Sheet1 => self.values.sheet = 1,
        }
    }
}

impl super::Service for DataPage {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        _state: &mut DeviceState,
    ) -> (Option<Command>, Effects) {
        let BusinessIntent::Gallery { wire, intent } = intent else {
            return (None, Effects::default());
        };
        if *wire != PageId::Data.code() {
            return (None, Effects::default());
        }
        match *intent {
            GalleryIntent::Tap { tag } => match tag {
                TAG_DIALOG_OK | TAG_DIALOG_CANCEL => self.values.dialog = false,
                _ => self.tap(tag),
            },
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
    fn bind(&self, op: &InputIntent, _state: &DeviceState) -> Option<BusinessIntent> {
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
            let tag = match dialog::action_at(self.dialog, i32::from(x), i32::from(y)) {
                Some(true) => TAG_DIALOG_OK,
                Some(false) | None => TAG_DIALOG_CANCEL,
            };
            return tap(GalleryIntent::Tap { tag });
        }
        if let InputEvent::Gesture(GestureEvent::Tap { x, y, .. }) = op.event {
            let area = self.scroll.area();
            if area.rect.contains(i32::from(x), i32::from(y)) {
                let content_y = i32::from(y) + i32::from(area.offset);
                let tag = self.mark_at(i32::from(x), content_y);
                if tag != 0 {
                    return tap(GalleryIntent::Tap { tag });
                }
            }
            if let Some(action) = Action::from_tag(self.tag_at(i32::from(x), i32::from(y))) {
                return tap(GalleryIntent::Tap { tag: action.tag() });
            }
        }
        self.scroll.bind(
            op,
            PageId::Data.code(),
            |x, y| self.tag_at(x, y),
            |tag| tag == Action::Swipe.tag(),
        )
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
            .with_repaints(Repaint::WhenOrOverlay(data_moved))
            .with_scroll(data_shift)
    }

    fn tick(&mut self, step_ms: u16) {
        self.scroll.tick(step_ms);
    }

    fn on_hide(&mut self) {
        self.scroll.reset();
        self.values.dialog = false;
    }
}

fn declare(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
    let lang = snapshot.lang;
    let w = snapshot.pages.data;
    let theme = Theme::PANEL;
    page::body(ui, |ui| {
        page::title(ui, Msg::DataTitle.bytes(lang));
        page::scroller(ui, AREA, w.area, |ui| {
            section(ui, Msg::SectionData.bytes(lang), &theme);
            ui.custom(Style::new().width(CONTENT_W).height(TABLE_H), TABLE_SLOT);
            let rule = ui.color(theme.muted);
            ui.text(Msg::TapHint.bytes(lang), Style::new().nowrap().color(rule));
            ui.custom(Style::new().width(CONTENT_W).height(CHART_H), CHART_SLOT);
            ui.text(Msg::TapBar.bytes(lang), Style::new().nowrap().color(rule));
            ui.custom(Style::new().width(CONTENT_W).height(GAUGE_H), GAUGE_SLOT);
            ui.custom(
                Style::new()
                    .width(CONTENT_W)
                    .height(page::TOUCH)
                    .tag(Action::Swipe.tag()),
                SWIPE_SLOT,
            );
            ui.add(
                EmptyState::new(Msg::Empty.bytes(lang), Msg::TapHint.bytes(lang))
                    .tone(if w.empty_error {
                        EmptyTone::Error
                    } else {
                        EmptyTone::Empty
                    })
                    .width(CONTENT_W)
                    .tag(Action::Empty.tag()),
            );

            section(ui, Msg::SectionOverlay.bytes(lang), &theme);
            ui.add(
                Button::new(Msg::Dialog.bytes(lang))
                    .width(CONTENT_W)
                    .tag(Action::DialogOpen.tag()),
            );
            ui.add(
                PopupMenu::new(
                    [
                        Msg::Name.bytes(lang),
                        Msg::Mail.bytes(lang),
                        Msg::Save.bytes(lang),
                    ],
                    [
                        Action::Menu0.tag(),
                        Action::Menu1.tag(),
                        Action::Menu2.tag(),
                    ],
                    w.menu,
                )
                .width(CONTENT_W)
                .theme(&theme),
            );
            ui.add(
                Sheet::new(
                    Msg::Detail.bytes(lang),
                    [Msg::Name.bytes(lang), Msg::Mail.bytes(lang)],
                    [Action::Sheet0.tag(), Action::Sheet1.tag()],
                    w.sheet,
                )
                .edge(SheetEdge::Bottom)
                .width(CONTENT_W)
                .theme(&theme),
            );
        });
        ui.custom(
            Style::new()
                .absolute()
                .width(PANEL_COLUMNS as i16 - page::MARGIN)
                .height(CONTENT_BOTTOM as i16 - 8),
            DIALOG_SLOT,
        );
    });
}

fn section(ui: &mut Ui<'_, '_>, run: &[u8], theme: &Theme) {
    ui.add(Divider::new().width(CONTENT_W));
    let ink = ui.color(theme.muted);
    ui.text(run, Style::new().nowrap().color(ink));
}

fn custom(
    snapshot: &FrameSnapshot,
    _screen: Screen,
    slot: u8,
    canvas: &mut Canvas<'_>,
    rect: Rect,
) {
    let theme = Theme::PANEL;
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
                canvas.set_color(Some(theme.ink));
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
            canvas.set_color(Some(theme.ink));
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
                title: Msg::Detail.bytes(lang),
                body: Msg::TapHint.bytes(lang),
                ok: Msg::Ok.bytes(lang),
                cancel: Msg::Cancel.bytes(lang),
            }
            .draw(canvas, rect, &theme);
        }
        _ => {}
    }
}

fn data_moved(prev: &FrameSnapshot, next: &FrameSnapshot) -> bool {
    prev.lang != next.lang || prev.pages.data != next.pages.data
}

fn data_shift(prev: &FrameSnapshot, next: &FrameSnapshot) -> Option<ScrollShift> {
    super::gallery::shift(
        PageId::Data,
        prev,
        next,
        |pages| pages.data.area,
        |pages| pages.data.moving,
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

    fn tap(page: &mut DataPage, tag: u16) {
        let mut state = DeviceState::default();
        page.apply(
            &BusinessIntent::Gallery {
                wire: PageId::Data.code(),
                intent: GalleryIntent::Tap { tag },
            },
            &mut state,
        );
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
        let drawn: Vec<u16> = page.tags.iter().map(|(tag, _)| tag).collect();
        let expected: Vec<u16> = Action::ALL.iter().map(|action| action.tag()).collect();
        assert_eq!(drawn, expected);
    }

    #[test]
    fn a_row_a_bar_and_the_dial_each_pick_their_own() {
        let mut page = DataPage::default();
        let row = page.table.y + i32::from(table::HEADER_H) + i32::from(table::ROW_H) * 2 + 4;
        let tag = page.mark_at(page.table.x + 4, row);
        assert_eq!(tag, TAG_TABLE + 2);
        tap(&mut page, tag);
        assert_eq!(page.values.table, Some(2));

        let tag = page.mark_at(page.chart_rect.x + 4, page.chart_rect.y + 4);
        assert_eq!(tag, TAG_CHART);
        tap(&mut page, tag);
        assert_eq!(page.values.bar, Some(0));
        assert_eq!(page.values.gauge, BARS[0]);

        let before = page.values.gauge;
        tap(&mut page, TAG_GAUGE);
        assert_eq!(page.values.gauge, (before + 25).min(100));
    }

    #[test]
    fn the_dial_cycles_back_to_zero_after_a_full_sweep() {
        let mut page = DataPage::default();
        page.values.gauge = 100;
        tap(&mut page, TAG_GAUGE);
        assert_eq!(page.values.gauge, 0);
    }

    fn apply(page: &mut DataPage, intent: GalleryIntent) {
        let mut state = DeviceState::default();
        page.apply(
            &BusinessIntent::Gallery {
                wire: PageId::Data.code(),
                intent,
            },
            &mut state,
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
        tap(&mut page, TAG_DIALOG_CANCEL);
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
        let shift = data_shift(&before, &after).expect("a pure scroll");
        assert_eq!(shift.shifts()[0], after.pages.data.area.offset);
    }
}
