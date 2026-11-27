//! The feed demo: one list that pulls to refresh, reaches its end to load more, and slides a
//! row left to delete it. The data is a stub: the component under test owns the edges.

use iot_core::drivers::input::{GestureEvent, InputEvent, SwipeDirection};
use iot_core::ui::panel::PANEL_COLUMNS;
use iot_core::ui::{Canvas, Rect, Screen, Style, Ui};

use crate::shared::i18n::Msg;
use crate::shared::intent::{BusinessIntent, InputIntent, ListIntent};
use crate::shared::model::{Command, DeviceState, Effects};
use crate::shared::page;
use crate::shared::route::PageId;
use crate::shared::snapshot::{
    FrameSnapshot, ListSnapshot, PageSnapshot, ScrollAreaView, ScrollShift,
};
use crate::shared::surface::{finish, headless};
use crate::shared::ui::feed::{self, Edge, Feed, Row, Source};
use crate::shared::ui::scroll::{BAR, Drag, STEP_LINES, ScrollConfig, fling_speed};
use crate::shared::ui::theme::Theme;
use crate::shared::ui::widgets::REVEAL_MAX;

use super::gallery::REPAINT_MS;
use super::{Descriptor, Repaint};

const FEED_SLOT: u8 = 0;
const TOTAL: u16 = 34;
const PAGE: u16 = 8;
/// Just over half a second of fake work per edge, so the indicator has time to be seen.
const WORK_MS: u16 = 600;

const SCROLL: ScrollConfig = ScrollConfig {
    bounce: true,
    bar: false,
    pull: 44,
};

const TITLES: [&[u8]; 8] = [
    b"ALPHA", b"BRAVO", b"CHARLIE", b"DELTA", b"ECHO", b"FOXTROT", b"GOLF", b"HOTEL",
];
const DETAILS: [&[u8]; 4] = [
    b"TAP TO OPEN",
    b"HOLD TO MOVE",
    b"SWIPE TO DELETE",
    b"PINNED",
];
const MARKS: [char; 4] = ['#', '*', '+', '@'];

/// The stub data: generated rows, a loaded prefix, and the ones a swipe removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Demo {
    loaded: u16,
    removed: u64,
}

impl Default for Demo {
    fn default() -> Self {
        Self {
            loaded: PAGE.min(TOTAL),
            removed: 0,
        }
    }
}

impl Demo {
    fn present(&self, index: u16) -> bool {
        index < self.loaded && self.removed & (1u64 << index) == 0
    }

    /// The source index behind a visible row.
    fn source(&self, visible: u16) -> Option<u16> {
        let mut seen = 0;
        for index in 0..self.loaded {
            if self.present(index) {
                if seen == visible {
                    return Some(index);
                }
                seen += 1;
            }
        }
        None
    }

    fn remove(&mut self, visible: u16) -> bool {
        match self.source(visible) {
            Some(index) => {
                self.removed |= 1u64 << index;
                true
            }
            None => false,
        }
    }

    fn len(&self) -> u16 {
        (0..self.loaded)
            .filter(|index| self.present(*index))
            .count() as u16
    }
}

impl Source for Demo {
    fn len(&self) -> u16 {
        Demo::len(self)
    }

    fn row(&self, index: u16) -> Row<'_> {
        let source = self.source(index).unwrap_or(0);
        let step = usize::from(source);
        Row {
            title: TITLES[step % TITLES.len()],
            detail: DETAILS[step % DETAILS.len()],
            mark: MARKS[step % MARKS.len()],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Finger {
    id: u8,
    x0: i32,
    y0: i16,
    row: Option<u16>,
    base: i16,
    horizontal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListPage {
    feed: Feed,
    demo: Demo,
    drag: Option<Drag>,
    finger: Option<Finger>,
    reveal: Option<(u16, i16)>,
    selected: Option<u16>,
    wait: Option<Edge>,
    wait_ms: u16,
}

impl Default for ListPage {
    fn default() -> Self {
        let mut ui = headless();
        declare(&FrameSnapshot::default(), &mut ui);
        finish(&mut ui);
        let rect = ui.paint_rect(FEED_SLOT).unwrap_or_default();
        let demo = Demo::default();
        let mut feed = Feed::default();
        feed.measure(rect, SCROLL);
        feed.set_rows(demo.len());
        Self {
            feed,
            demo,
            drag: None,
            finger: None,
            reveal: None,
            selected: None,
            wait: None,
            wait_ms: 0,
        }
    }
}

impl ListPage {
    fn snapshot(&self) -> ListSnapshot {
        let area = self.feed.area();
        let max = area.max();
        ListSnapshot {
            area: ScrollAreaView {
                rect: area.rect,
                header: Rect::default(),
                gutter: Rect::new(
                    area.rect.right() + i32::from(BAR),
                    area.rect.y,
                    i32::from(BAR),
                    area.rect.h,
                ),
                offset: area.offset,
                max,
                bar: false,
            },
            moving: self.moving(),
            rows: self.feed.rows(),
            loaded: self.demo.loaded,
            removed: self.demo.removed,
            selected: self.selected,
            reveal: self.reveal,
            pull: self.feed.pull(),
            refresh: self.feed.refresh(),
            more: self.feed.more(),
        }
    }

    fn moving(&self) -> bool {
        self.feed.moving() || self.wait.is_some()
    }

    fn row_at(&self, y: i32) -> Option<u16> {
        let area = self.feed.area();
        let content_y = y - area.rect.y + i32::from(area.offset);
        if content_y < 0 {
            return None;
        }
        let row = u16::try_from(content_y / i32::from(feed::ROW_H)).ok()?;
        (row < self.demo.len()).then_some(row)
    }

    fn press(&mut self, id: u8, x: u16, y: u16) {
        if self.finger.is_some() {
            return;
        }
        let y = i16::try_from(y).unwrap_or(i16::MAX);
        let row = self.row_at(i32::from(y));
        let base = match self.reveal {
            Some((shown, by)) if Some(shown) == row => by,
            _ => 0,
        };
        self.finger = Some(Finger {
            id,
            x0: i32::from(x),
            y0: y,
            row,
            base,
            horizontal: false,
        });
        self.drag = Some(Drag::new(id, 0, y, 0));
    }

    fn drag_to(&mut self, id: u8, x: u16, y: u16, at_ms: u32) {
        let Some(finger) = self.finger.as_mut().filter(|finger| finger.id == id) else {
            self.press(id, x, y);
            return;
        };
        let y = i16::try_from(y).unwrap_or(i16::MAX);
        let dx = i32::from(x) - finger.x0;
        let dy = i32::from(y) - i32::from(finger.y0);
        if !finger.horizontal && (dx.abs() >= 5 || dy.abs() >= 5) {
            finger.horizontal = dx.abs() > dy.abs();
        }
        if finger.horizontal {
            let row = finger.row;
            let by = (i32::from(finger.base) - dx).clamp(0, i32::from(REVEAL_MAX));
            self.reveal = row.map(|row| (row, by as i16));
            self.feed.calm();
        } else if let Some(drag) = self.drag.as_mut().filter(|drag| drag.id == id) {
            let delta = drag.step(y, at_ms);
            self.feed.drag(delta);
        }
    }

    fn release(&mut self, id: u8, lines: i8, speed: u16, at_ms: Option<u32>) {
        let Some(finger) = self.finger.take().filter(|finger| finger.id == id) else {
            if lines != 0 || speed != 0 {
                self.feed.scroll(lines, speed);
                if let Some(edge) = self.feed.release(0) {
                    self.start(edge);
                }
            }
            return;
        };
        if finger.horizontal {
            let keep = self.reveal.is_some_and(|(_, by)| by >= REVEAL_MAX / 2);
            if !keep {
                self.reveal = None;
            }
            self.feed.calm();
        } else {
            let speed = self.drag.take().map_or(0, |drag| drag.release(at_ms));
            if lines != 0 {
                self.feed.scroll(lines, speed.unsigned_abs());
            }
            if let Some(edge) = self.feed.release(speed) {
                self.start(edge);
            }
        }
        self.drag = None;
    }

    fn tap(&mut self, x: u16, y: u16) {
        self.finger = None;
        self.drag = None;
        let Some(row) = self.row_at(i32::from(y)) else {
            self.selected = None;
            self.reveal = None;
            return;
        };
        if let Some((shown, by)) = self.reveal
            && shown == row
            && by > 0
        {
            let area = self.feed.area();
            if i32::from(x) >= area.rect.right() - i32::from(by) {
                if self.demo.remove(row) {
                    self.feed.set_rows(self.demo.len());
                    self.reveal = None;
                    self.selected = None;
                }
                return;
            }
        }
        self.selected = Some(row);
    }

    fn step(&mut self, step_ms: u16) {
        self.feed.tick(step_ms);
        if let Some(edge) = self.wait {
            self.wait_ms = self.wait_ms.saturating_sub(step_ms);
            if self.wait_ms == 0 {
                self.wait = None;
                self.finish(edge);
            }
        }
    }

    fn start(&mut self, edge: Edge) {
        self.wait = Some(edge);
        self.wait_ms = WORK_MS;
    }

    fn finish(&mut self, edge: Edge) {
        match edge {
            Edge::Refresh => {
                self.demo.loaded = PAGE.min(TOTAL);
                self.demo.removed = 0;
                self.feed.loaded(self.demo.len(), false);
                self.reveal = None;
                self.selected = None;
            }
            Edge::More => {
                self.demo.loaded = (self.demo.loaded + PAGE).min(TOTAL);
                self.feed.loaded(self.demo.len(), self.demo.loaded >= TOTAL);
            }
        }
    }
}

impl super::Service for ListPage {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        _state: &mut DeviceState,
        _layout: &iot_core::ui::Layout,
    ) -> (Option<Command>, Effects) {
        let BusinessIntent::List(intent) = intent else {
            return (None, Effects::default());
        };
        match *intent {
            ListIntent::Press { id, x, y } => self.press(id, x, y),
            ListIntent::Drag { id, x, y, at_ms } => self.drag_to(id, x, y, at_ms),
            ListIntent::Release {
                id,
                lines,
                speed,
                at_ms,
                ..
            } => self.release(id, lines, speed, at_ms),
            ListIntent::Scroll { lines, speed } => self.feed.scroll(lines, speed),
            ListIntent::Tap { x, y } => self.tap(x, y),
        }
        (None, Effects::default())
    }
}

impl super::LocalPage for ListPage {
    fn bind(
        &self,
        op: &InputIntent,
        _state: &DeviceState,
        _layout: &iot_core::ui::Layout,
    ) -> Option<BusinessIntent> {
        let list = |intent| Some(BusinessIntent::List(intent));
        match op.event {
            InputEvent::Gesture(GestureEvent::Tap { x, y, .. }) => list(ListIntent::Tap { x, y }),
            InputEvent::Gesture(GestureEvent::Press { id, x, y }) => {
                list(ListIntent::Press { id, x, y })
            }
            InputEvent::Gesture(GestureEvent::Drag { id, x, y, at_ms }) => {
                list(ListIntent::Drag { id, x, y, at_ms })
            }
            InputEvent::Gesture(GestureEvent::Swipe {
                id,
                direction,
                x,
                y,
                held_ms,
                distance_px,
                at_ms,
                ..
            }) => {
                let (lines, speed) = match direction {
                    SwipeDirection::Up => (STEP_LINES, fling_speed(distance_px, held_ms)),
                    SwipeDirection::Down => (-STEP_LINES, fling_speed(distance_px, held_ms)),
                    _ => (0, 0),
                };
                list(ListIntent::Release {
                    id,
                    x,
                    y,
                    lines,
                    speed,
                    at_ms: Some(at_ms),
                })
            }
            InputEvent::Gesture(GestureEvent::LongPress { id, x, y, .. }) => {
                list(ListIntent::Release {
                    id,
                    x,
                    y,
                    lines: 0,
                    speed: 0,
                    at_ms: None,
                })
            }
            _ => None,
        }
    }

    fn publish(&self, out: &mut PageSnapshot) {
        out.list = self.snapshot();
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        declare(snapshot, ui);
    }

    fn descriptor() -> Descriptor {
        Descriptor::new(Self::view)
            .with_custom(custom)
            .with_repaint_ms(REPAINT_MS)
            .with_repaints(Repaint::WhenOrOverlay(list_moved))
            .with_scroll(list_shift)
    }

    fn tick(&mut self, step_ms: u16) {
        self.step(step_ms);
    }

    fn on_hide(&mut self) {
        self.feed.reset();
        self.drag = None;
        self.finger = None;
        self.reveal = None;
        self.selected = None;
        self.wait = None;
    }
}

fn declare(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
    let lang = snapshot.lang;
    page::body(ui, |ui| {
        page::title(ui, Msg::ListTitle.bytes(lang));
        ui.custom(
            Style::new()
                .grow(1)
                .basis(0)
                .stretch()
                .width(PANEL_COLUMNS as i16 - page::MARGIN),
            FEED_SLOT,
        );
    });
}

fn custom(snapshot: &FrameSnapshot, screen: Screen, slot: u8, canvas: &mut Canvas<'_>, rect: Rect) {
    if slot != FEED_SLOT {
        return;
    }
    let lang = snapshot.lang;
    let w = snapshot.pages.list;
    let view = feed::FeedView {
        rows: &Demo {
            loaded: w.loaded,
            removed: w.removed,
        },
        offset: w.area.offset,
        pull: w.pull,
        refresh: w.refresh,
        more: w.more,
        selected: w.selected,
        reveal: w.reveal,
        words: feed::Words {
            refresh: Msg::Refresh.bytes(lang),
            load: Msg::Loading.bytes(lang),
            end: Msg::Ended.bytes(lang),
            failed: Msg::Failed.bytes(lang),
            empty: Msg::Empty.bytes(lang),
            delete: Msg::Delete.bytes(lang),
        },
    };
    feed::draw(&view, canvas, rect, Theme::of(screen.theme));
}

fn list_moved(prev: &FrameSnapshot, next: &FrameSnapshot) -> bool {
    prev.lang != next.lang || prev.pages.list != next.pages.list
}

fn list_shift(prev: &FrameSnapshot, next: &FrameSnapshot) -> Option<ScrollShift> {
    super::gallery::shift(
        PageId::List,
        prev,
        next,
        |pages| pages.list.area,
        |pages| pages.list.moving,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::ui::feed::{More, Refresh};

    #[test]
    fn the_feed_starts_with_one_page() {
        let page = ListPage::default();
        assert_eq!(page.demo.loaded, PAGE);
        assert_eq!(page.feed.rows(), PAGE);
        assert_eq!(page.demo.len(), PAGE);
    }

    #[test]
    fn a_pull_down_asks_to_refresh_and_the_work_resets_the_page() {
        let mut page = ListPage::default();
        let rect = page.feed.area().rect;
        let x = (rect.x + rect.w / 2) as u16;
        page.press(0, x, rect.y.max(0) as u16 + 4);
        page.drag_to(0, x, (rect.y + 120) as u16, 40);
        page.release(0, 0, 0, Some(40));
        assert_eq!(page.feed.refresh(), Refresh::Running);
        page.step(WORK_MS);
        assert_eq!(page.feed.refresh(), Refresh::Idle);
        assert_eq!(page.demo.loaded, PAGE);
        assert_eq!(page.feed.rows(), PAGE);
    }

    #[test]
    fn reaching_the_end_loads_the_next_page_and_then_ends() {
        let mut page = ListPage::default();
        page.feed.scroll(60, 0);
        page.release(0, STEP_LINES, 0, None);
        assert_eq!(page.feed.more(), More::Running);
        page.step(WORK_MS);
        assert_eq!(page.demo.loaded, 2 * PAGE);
        while page.demo.loaded < TOTAL {
            page.feed.scroll(60, 0);
            page.release(0, STEP_LINES, 0, None);
            page.step(WORK_MS);
        }
        assert_eq!(page.demo.loaded, TOTAL);
        assert_eq!(page.feed.more(), More::End);
    }

    #[test]
    fn a_left_slide_uncovers_the_band_and_a_tap_there_deletes() {
        let mut page = ListPage::default();
        let rect = page.feed.area().rect;
        let y = (rect.y + 20) as u16;
        page.press(0, (rect.x + rect.w - 10) as u16, y);
        page.drag_to(0, (rect.x + rect.w - 80) as u16, y, 40);
        let (row, by) = page.reveal.expect("the row slid");
        assert_eq!(row, 0);
        assert_eq!(by, 70);
        page.tap((rect.right() - 4) as u16, y);
        assert_eq!(page.demo.len(), PAGE - 1, "the row is gone");
        assert!(page.reveal.is_none());
    }

    #[test]
    fn a_tap_selects_the_row_under_it() {
        let mut page = ListPage::default();
        let rect = page.feed.area().rect;
        page.tap((rect.x + 10) as u16, (rect.y + 60) as u16);
        assert_eq!(page.selected, Some(1));
    }
}
