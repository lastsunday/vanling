//! The records-widget page: a tree, a timeline, a year heatmap and a list whose rows move by
//! their handle. Each mark is one canvas slot whose draw and hit test share one geometry.

use iot_core::drivers::input::{GestureEvent, InputEvent};
use iot_core::ui::panel::PANEL_COLUMNS;
use iot_core::ui::{Canvas, Rect, Screen, ScrollId, Style, Ui};

use crate::shared::i18n::Msg;
use crate::shared::intent::{BusinessIntent, GalleryIntent, InputIntent};
use crate::shared::model::{Command, DeviceState, Effects};
use crate::shared::page;
use crate::shared::route::PageId;
use crate::shared::snapshot::{FrameSnapshot, PageSnapshot, RecordsSnapshot, ScrollShift};
use crate::shared::surface::{finish, headless};
use crate::shared::ui::scroll::{BAR, ScrollConfig};
use crate::shared::ui::tag::TagTable;
use crate::shared::ui::theme::Theme;
use crate::shared::ui::widgets::{
    Heatmap, ReorderableList, Timeline, TimelineEntry, TreeRow, TreeView, heatmap,
    reorderable_list, timeline, tree_view,
};

use super::gallery::{REPAINT_MS, Scroller};
use super::{Descriptor, Repaint};

const AREA: ScrollId = ScrollId::new(1);
const CONTENT_W: i16 = PANEL_COLUMNS as i16 - page::MARGIN - 2 * BAR;

const TREE_SLOT: u8 = 0;
const TIMELINE_SLOT: u8 = 1;
const HEATMAP_SLOT: u8 = 2;
const LIST_SLOT: u8 = 3;

/// Sub-cell tags the page computes for itself: a tree row, a timeline entry, a heatmap cell,
/// and a reorder row.
const TAG_TREE: u16 = 100;
const TAG_TIMELINE: u16 = 110;
const TAG_HEATMAP: u16 = 200;
const TAG_LIST: u16 = 600;

const TREE_ROWS: usize = 6;
const ENTRY_ROWS: usize = 4;
const LIST_ROWS: usize = 5;

const TREE_DEPTH: [u8; TREE_ROWS] = [0, 1, 1, 0, 1, 1];
const TREE_OPEN: [bool; TREE_ROWS] = [true, false, false, true, false, false];
const TREE_LEAF: [bool; TREE_ROWS] = [false, true, true, false, true, true];
const TREE_LABEL: [Msg; TREE_ROWS] = [
    Msg::Name,
    Msg::Mail,
    Msg::Save,
    Msg::Details,
    Msg::Value,
    Msg::BodyCopy,
];

const TIMES: [&[u8]; ENTRY_ROWS] = [b"09:00", b"10:30", b"12:15", b"18:40"];
const ENTRY_LABEL: [Msg; ENTRY_ROWS] = [Msg::Name, Msg::Mail, Msg::Save, Msg::Details];

const LIST_LABEL: [Msg; LIST_ROWS] = [Msg::Name, Msg::Mail, Msg::Save, Msg::Details, Msg::Value];
const LIST_DETAIL: [Msg; LIST_ROWS] = [
    Msg::Detail,
    Msg::Detail,
    Msg::Detail,
    Msg::Detail,
    Msg::Detail,
];

const SCROLL: ScrollConfig = ScrollConfig {
    bounce: true,
    bar: true,
    pull: 0,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordsPage {
    scroll: Scroller,
    tags: TagTable<4>,
    tree: Rect,
    timeline: Rect,
    heatmap: Rect,
    list: Rect,
    /// The finger holding a reorder handle.
    drag: Option<u8>,
    values: RecordsSnapshot,
}

impl Default for RecordsPage {
    fn default() -> Self {
        let mut ui = headless();
        declare(&FrameSnapshot::default(), &mut ui);
        finish(&mut ui);
        let mut scroll = Scroller::default();
        scroll.measure(&ui, AREA, SCROLL);
        Self {
            scroll,
            tags: TagTable::measure(&ui),
            tree: ui.paint_rect(TREE_SLOT).unwrap_or_default(),
            timeline: ui.paint_rect(TIMELINE_SLOT).unwrap_or_default(),
            heatmap: ui.paint_rect(HEATMAP_SLOT).unwrap_or_default(),
            list: ui.paint_rect(LIST_SLOT).unwrap_or_default(),
            drag: None,
            values: RecordsSnapshot::default(),
        }
    }
}

impl RecordsPage {
    fn tag_at(&self, x: i32, y: i32) -> u16 {
        self.scroll
            .tag_at(|x, y, offset| self.tags.at(x, y, offset), x, y)
    }

    fn snapshot(&self) -> RecordsSnapshot {
        let mut view = self.values;
        view.area = self.scroll.view();
        view.moving = self.scroll.moving();
        view
    }

    fn list_at(&self, x: i32, y: i32) -> Option<u8> {
        let area = self.scroll.area();
        if !area.rect.contains(x, y) {
            return None;
        }
        reorderable_list::handle_at(self.list, LIST_ROWS, x, y + i32::from(area.offset))
    }

    fn target_at(&self, y: u16) -> u8 {
        let content = i32::from(y) + i32::from(self.scroll.area().offset);
        let row = (content - self.list.y) / i32::from(reorderable_list::ROW_H);
        row.clamp(0, LIST_ROWS as i32 - 1) as u8
    }

    fn tap_tag(&mut self, tag: u16) {
        if let Some(row) = tag
            .checked_sub(TAG_TREE)
            .filter(|row| *row < TREE_ROWS as u16)
        {
            self.values.tree = Some(row as u8);
            return;
        }
        if let Some(row) = tag
            .checked_sub(TAG_TIMELINE)
            .filter(|row| *row < ENTRY_ROWS as u16)
        {
            self.values.entry = Some(row as u8);
            return;
        }
        if let Some(cell) = tag
            .checked_sub(TAG_HEATMAP)
            .filter(|cell| *cell < heatmap::CELLS)
        {
            self.values.cell = Some(cell);
        }
    }

    fn finish_reorder(&mut self) {
        if let (Some(from), Some(to)) = (self.values.lifted, self.values.target) {
            let from = usize::from(from).min(LIST_ROWS - 1);
            let to = usize::from(to).min(LIST_ROWS - 1);
            if from < to {
                self.values.order[from..=to].rotate_left(1);
            } else if to < from {
                self.values.order[to..=from].rotate_right(1);
            }
        }
        self.drag = None;
        self.values.lifted = None;
        self.values.target = None;
    }
}

impl super::Service for RecordsPage {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        _state: &mut DeviceState,
    ) -> (Option<Command>, Effects) {
        let BusinessIntent::Gallery { wire, intent } = intent else {
            return (None, Effects::default());
        };
        if *wire != PageId::Records.code() {
            return (None, Effects::default());
        }
        let mut handled = false;
        match *intent {
            GalleryIntent::Tap { tag } => {
                if self.drag.is_some() {
                    self.finish_reorder();
                    handled = true;
                } else {
                    self.tap_tag(tag);
                }
            }
            GalleryIntent::Press { id, tag, .. } if tag >= TAG_LIST => {
                let row = u8::try_from(tag - TAG_LIST)
                    .unwrap_or(0)
                    .min(LIST_ROWS as u8 - 1);
                self.drag = Some(id);
                self.values.lifted = Some(row);
                self.values.target = Some(row);
                handled = true;
            }
            GalleryIntent::Drag { id, y, .. } if self.drag == Some(id) => {
                self.values.target = Some(self.target_at(y));
                handled = true;
            }
            _ => {}
        }
        if !handled {
            self.scroll.apply(*intent);
        }
        (None, Effects::default())
    }
}

impl super::LocalPage for RecordsPage {
    fn bind(&self, op: &InputIntent, _state: &DeviceState) -> Option<BusinessIntent> {
        let gallery = |intent| {
            Some(BusinessIntent::Gallery {
                wire: PageId::Records.code(),
                intent,
            })
        };
        match op.event {
            InputEvent::Gesture(GestureEvent::Press { id, x, y }) => {
                if let Some(row) = self.list_at(i32::from(x), i32::from(y)) {
                    return gallery(GalleryIntent::Press {
                        id,
                        tag: TAG_LIST + u16::from(row),
                        x,
                        y,
                    });
                }
            }
            InputEvent::Gesture(GestureEvent::Drag { id, x, y, at_ms }) => {
                if self.drag == Some(id) {
                    return gallery(GalleryIntent::Drag { id, x, y, at_ms });
                }
            }
            InputEvent::Gesture(GestureEvent::Tap { id, .. })
            | InputEvent::Gesture(GestureEvent::Swipe { id, .. })
            | InputEvent::Gesture(GestureEvent::LongPress { id, .. })
                if self.drag == Some(id) =>
            {
                return gallery(GalleryIntent::Tap { tag: TAG_LIST });
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
                if let Some(row) = tree_view::row_at(self.tree, TREE_ROWS, x, content) {
                    return gallery(GalleryIntent::Tap {
                        tag: TAG_TREE + u16::from(row),
                    });
                }
                if let Some(row) = timeline::row_at(self.timeline, ENTRY_ROWS, x, content) {
                    return gallery(GalleryIntent::Tap {
                        tag: TAG_TIMELINE + u16::from(row),
                    });
                }
                if let Some(cell) = heatmap::cell_at(self.heatmap, x, content) {
                    return gallery(GalleryIntent::Tap {
                        tag: TAG_HEATMAP + cell,
                    });
                }
            }
        }
        self.scroll.bind(
            op,
            PageId::Records.code(),
            |x, y| self.tag_at(x, y),
            |_| false,
        )
    }

    fn publish(&self, out: &mut PageSnapshot) {
        out.records = self.snapshot();
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        declare(snapshot, ui);
    }

    fn descriptor() -> Descriptor {
        Descriptor::new(Self::view)
            .with_custom(custom)
            .with_repaint_ms(REPAINT_MS)
            .with_repaints(Repaint::WhenOrOverlay(records_moved))
            .with_scroll(records_shift)
    }

    fn tick(&mut self, step_ms: u16) {
        self.scroll.tick(step_ms);
    }

    fn on_hide(&mut self) {
        self.scroll.reset();
        self.drag = None;
        self.values.lifted = None;
        self.values.target = None;
    }
}

fn declare(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
    let lang = snapshot.lang;
    let w = snapshot.pages.records;
    let theme = *Theme::of(snapshot.theme);
    page::body(ui, |ui| {
        page::title(ui, Msg::RecordsTitle.bytes(lang));
        page::scroller(ui, AREA, w.area, |ui| {
            page::caption(ui, Msg::CapTree.bytes(lang), &theme);
            ui.custom(
                Style::new()
                    .width(CONTENT_W)
                    .height(tree_view::height(TREE_ROWS))
                    .tag(TAG_TREE),
                TREE_SLOT,
            );
            page::caption(ui, Msg::CapTimeline.bytes(lang), &theme);
            ui.custom(
                Style::new()
                    .width(CONTENT_W)
                    .height(timeline::height(ENTRY_ROWS))
                    .tag(TAG_TIMELINE),
                TIMELINE_SLOT,
            );
            page::caption(ui, Msg::CapHeatmap.bytes(lang), &theme);
            ui.custom(
                Style::new()
                    .width(heatmap::WIDTH)
                    .height(heatmap::HEIGHT)
                    .tag(TAG_HEATMAP),
                HEATMAP_SLOT,
            );
            page::caption(ui, Msg::CapReorder.bytes(lang), &theme);
            ui.custom(
                Style::new()
                    .width(CONTENT_W)
                    .height(reorderable_list::height(LIST_ROWS))
                    .tag(TAG_LIST),
                LIST_SLOT,
            );
        });
    });
}

fn custom(snapshot: &FrameSnapshot, screen: Screen, slot: u8, canvas: &mut Canvas<'_>, rect: Rect) {
    let theme = *Theme::of(screen.theme);
    let lang = snapshot.lang;
    let w = snapshot.pages.records;
    match slot {
        TREE_SLOT => {
            let rows: [TreeRow<'_>; TREE_ROWS] = core::array::from_fn(|index| TreeRow {
                depth: TREE_DEPTH[index],
                label: TREE_LABEL[index].bytes(lang),
                open: TREE_OPEN[index],
                leaf: TREE_LEAF[index],
            });
            TreeView {
                rows: &rows,
                selected: w.tree,
            }
            .draw(canvas, rect, &theme);
        }
        TIMELINE_SLOT => {
            let entries: [TimelineEntry<'_>; ENTRY_ROWS] =
                core::array::from_fn(|index| TimelineEntry {
                    time: TIMES[index],
                    text: ENTRY_LABEL[index].bytes(lang),
                    lit: w.entry == Some(index as u8),
                });
            Timeline { entries: &entries }.draw(canvas, rect, &theme);
        }
        HEATMAP_SLOT => heatmap::draw(
            canvas,
            rect,
            Heatmap {
                seed: w.seed,
                picked: w.cell,
                less: Msg::Less.bytes(lang),
                more: Msg::More.bytes(lang),
            },
            &theme,
        ),
        LIST_SLOT => {
            let rows: [reorderable_list::Row<'_>; LIST_ROWS] = core::array::from_fn(|index| {
                let original = usize::from(w.order[index]).min(LIST_ROWS - 1);
                reorderable_list::Row {
                    label: LIST_LABEL[original].bytes(lang),
                    detail: LIST_DETAIL[original].bytes(lang),
                }
            });
            ReorderableList {
                rows: &rows,
                lifted: w.lifted,
                target: w.target,
            }
            .draw(canvas, rect, &theme);
        }
        _ => {}
    }
}

fn records_moved(prev: &FrameSnapshot, next: &FrameSnapshot) -> bool {
    prev.lang != next.lang || prev.pages.records != next.pages.records
}

fn records_shift(prev: &FrameSnapshot, next: &FrameSnapshot) -> Option<ScrollShift> {
    super::gallery::shift(
        PageId::Records,
        prev,
        next,
        |pages| pages.records.area,
        |pages| pages.records.moving,
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

    fn tap(page: &mut RecordsPage, tag: u16) {
        let mut state = DeviceState::default();
        page.apply(
            &BusinessIntent::Gallery {
                wire: PageId::Records.code(),
                intent: GalleryIntent::Tap { tag },
            },
            &mut state,
        );
    }

    fn send(page: &mut RecordsPage, event: GestureEvent) {
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
            RecordsPage::view(
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
    fn every_mark_is_drawn_once() {
        let page = RecordsPage::default();
        let drawn: Vec<u16> = page.tags.iter().map(|(tag, _)| tag).collect();
        assert_eq!(drawn, [TAG_TREE, TAG_TIMELINE, TAG_HEATMAP, TAG_LIST]);
    }

    #[test]
    fn a_tap_picks_the_tree_row_the_entry_and_the_cell() {
        let mut page = RecordsPage::default();
        tap(&mut page, TAG_TREE + 3);
        assert_eq!(page.values.tree, Some(3));
        tap(&mut page, TAG_TIMELINE + 2);
        assert_eq!(page.values.entry, Some(2));
        tap(&mut page, TAG_HEATMAP + 41);
        assert_eq!(page.values.cell, Some(41));
    }

    #[test]
    fn a_handle_drag_moves_a_row_both_ways() {
        let mut page = RecordsPage::default();
        let area = page.scroll.area();
        let target = page.list.y + page.list.h / 2 - (area.rect.y + area.rect.h / 2);
        page.scroll.park(i16::try_from(target.max(0)).unwrap_or(0));
        let offset = page.scroll.area().offset;
        let handle_x = u16::try_from(page.list.right() - 4).unwrap();
        let list_y = page.list.y;
        let row_y = |row: i32| {
            u16::try_from(list_y + row * i32::from(reorderable_list::ROW_H) + 4 - i32::from(offset))
                .unwrap()
        };
        send(
            &mut page,
            GestureEvent::Press {
                id: 0,
                x: handle_x,
                y: row_y(0),
            },
        );
        assert_eq!(page.values.lifted, Some(0));
        send(
            &mut page,
            GestureEvent::Drag {
                id: 0,
                x: handle_x,
                y: row_y(3),
                at_ms: 20,
            },
        );
        assert_eq!(page.values.target, Some(3));
        send(
            &mut page,
            GestureEvent::Tap {
                id: 0,
                x: handle_x,
                y: row_y(3),
                held_ms: 40,
                at_ms: 40,
            },
        );
        assert_eq!(page.values.order, [1, 2, 3, 0, 4], "row 0 moved down");
        assert_eq!(page.values.lifted, None);

        send(
            &mut page,
            GestureEvent::Press {
                id: 1,
                x: handle_x,
                y: row_y(4),
            },
        );
        send(
            &mut page,
            GestureEvent::Drag {
                id: 1,
                x: handle_x,
                y: row_y(1),
                at_ms: 20,
            },
        );
        send(
            &mut page,
            GestureEvent::Tap {
                id: 1,
                x: handle_x,
                y: row_y(1),
                held_ms: 40,
                at_ms: 40,
            },
        );
        assert_eq!(page.values.order, [1, 4, 2, 3, 0], "row 4 moved up");
    }

    #[test]
    fn a_drag_off_the_handle_still_scrolls() {
        let mut page = RecordsPage::default();
        let area = page.scroll.area();
        let x = u16::try_from(page.list.x + 8).unwrap();
        let from = u16::try_from(area.rect.y + area.rect.h - 6).unwrap();
        send(&mut page, GestureEvent::Press { id: 0, x, y: from });
        send(
            &mut page,
            GestureEvent::Drag {
                id: 0,
                x,
                y: from - 30,
                at_ms: 20,
            },
        );
        assert!(page.scroll.area().offset > 0, "the label side scrolls");
        assert_eq!(page.values.lifted, None);
    }

    #[test]
    fn a_scroll_frame_moves_only_the_gallery() {
        let mut page = RecordsPage::default();
        let before = FrameSnapshot {
            page: PageId::Records,
            pages: PageSnapshot {
                records: page.snapshot(),
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
            page: PageId::Records,
            pages: PageSnapshot {
                records: page.snapshot(),
                ..PageSnapshot::default()
            },
            ..FrameSnapshot::default()
        };
        assert!(after.pages.records.area.offset > 0);
        let shift = records_shift(&before, &after).expect("a pure scroll");
        assert_eq!(shift.shifts()[0], after.pages.records.area.offset);
    }
}
