//! One list with a refresh edge above and a load-more edge below: one scroll region, two latches.
//!
//! A release past [`PULL_MIN`] asks to refresh; a release at the end asks for the next page. The
//! page owns the data and the wait: [`Feed`] records which edge is running, and the page answers
//! with [`Feed::loaded`] when its work returns.

use iot_core::ui::panel::TOUCH_MIN;
use iot_core::ui::{Canvas, Color, Rect};

use super::scroll::{ScrollConfig, ScrollState};
use super::theme::Theme;
use super::widgets::swipe_action::SwipeAction;

pub const ROW_H: i16 = TOUCH_MIN as i16;
pub const FOOTER_H: i16 = 26;
/// How far the top edge must be pulled for a release to refresh.
pub const PULL_MIN: i16 = 28;
/// How close to the end a release has to be for it to ask for more.
pub const LOAD_AHEAD: i16 = 24;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Refresh {
    #[default]
    Idle,
    Running,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum More {
    #[default]
    Idle,
    Running,
    End,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Refresh,
    More,
}

/// The list's travel, in px, for `rows` rows and the footer that always closes the list.
pub const fn content(rows: u16) -> i16 {
    let rows = if rows > i16::MAX as u16 {
        i16::MAX
    } else {
        rows as i16
    };
    rows.saturating_mul(ROW_H).saturating_add(FOOTER_H)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feed {
    scroll: ScrollState<1>,
    refresh: Refresh,
    more: More,
    rows: u16,
    /// The row count the last load was asked for; a change is what lets a second one through.
    asked_at: u16,
}

impl Default for Feed {
    fn default() -> Self {
        Self {
            scroll: ScrollState::default(),
            refresh: Refresh::Idle,
            more: More::Idle,
            rows: 0,
            asked_at: u16::MAX,
        }
    }
}

impl Feed {
    /// Reads the list's box out of the declaration; the rows are painted, not declared.
    pub fn measure(&mut self, rect: Rect, config: ScrollConfig) {
        self.scroll.measure_box(rect, content(self.rows), config);
    }

    pub fn area(&self) -> iot_core::ui::ScrollInfo {
        self.scroll.areas()[0]
    }

    pub fn config(&self) -> ScrollConfig {
        self.scroll.config()
    }

    pub fn rows(&self) -> u16 {
        self.rows
    }

    /// The rows changed: the region's travel follows, and an offset past the new end comes back.
    pub fn set_rows(&mut self, rows: u16) {
        self.rows = rows;
        self.scroll.set_content(0, content(rows));
    }

    pub fn drag(&mut self, delta: i16) {
        self.scroll.drag(0, delta);
    }

    pub fn scroll(&mut self, lines: i8, speed: u16) {
        self.scroll.scroll(0, lines, speed);
    }

    /// The finger lifted: the top edge wins over the bottom one, so one lift asks for one edge.
    pub fn release(&mut self, speed: i16) -> Option<Edge> {
        let pulled = self.pull();
        self.scroll.fling(0, speed);
        if self.refresh == Refresh::Idle && pulled >= PULL_MIN {
            self.refresh = Refresh::Running;
            return Some(Edge::Refresh);
        }
        if matches!(self.more, More::Idle | More::Failed)
            && self.asked_at != self.rows
            && self.at_end()
        {
            self.more = More::Running;
            self.asked_at = self.rows;
            return Some(Edge::More);
        }
        None
    }

    /// The finger let go without asking for an edge: a horizontal swipe still stops a glide.
    pub fn calm(&mut self) {
        self.scroll.fling(0, 0);
    }

    /// The page's answer: the rows it now has, and whether that was the last page.
    pub fn loaded(&mut self, rows: u16, end: bool) {
        self.set_rows(rows);
        self.refresh = Refresh::Idle;
        self.more = if end { More::End } else { More::Idle };
    }

    /// The page's answer when the load failed: a refresh just ends, a page waits for another pull.
    pub fn failed(&mut self) {
        self.refresh = Refresh::Idle;
        if self.more == More::Running {
            self.more = More::Failed;
            self.asked_at = u16::MAX;
        }
    }

    pub fn pull(&self) -> i16 {
        self.scroll.pull(0)
    }

    /// Whether a release right now would refresh.
    pub fn armed(&self) -> bool {
        self.pull() >= PULL_MIN
    }

    pub fn refresh(&self) -> Refresh {
        self.refresh
    }

    pub fn more(&self) -> More {
        self.more
    }

    pub fn offset(&self) -> i16 {
        self.scroll.areas()[0].offset
    }

    pub fn moving(&self) -> bool {
        self.scroll.moving() || self.refresh == Refresh::Running || self.more == More::Running
    }

    pub fn tick(&mut self, step_ms: u16) {
        self.scroll.tick(step_ms);
    }

    pub fn reset(&mut self) {
        self.scroll.reset();
        self.refresh = Refresh::Idle;
        self.more = More::Idle;
        self.asked_at = u16::MAX;
    }

    fn at_end(&self) -> bool {
        let area = self.area();
        i32::from(area.max()) - i32::from(area.offset) <= i32::from(LOAD_AHEAD)
    }
}

/// One visible row's copy; the page answers from its own data.
#[derive(Clone, Copy, Debug)]
pub struct Row<'a> {
    pub title: &'a [u8],
    pub detail: &'a [u8],
    pub mark: char,
}

pub trait Source {
    fn len(&self) -> u16;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn row(&self, index: u16) -> Row<'_>;
}

/// The words the two edges and the empty list print.
#[derive(Clone, Copy, Debug)]
pub struct Words<'a> {
    pub refresh: &'a [u8],
    pub load: &'a [u8],
    pub end: &'a [u8],
    pub failed: &'a [u8],
    pub empty: &'a [u8],
    pub delete: &'a [u8],
}

pub struct FeedView<'a> {
    pub rows: &'a dyn Source,
    pub offset: i16,
    pub pull: i16,
    pub refresh: Refresh,
    pub more: More,
    pub selected: Option<u16>,
    /// The row the finger slid left, and how far the action band is uncovered.
    pub reveal: Option<(u16, i16)>,
    pub words: Words<'a>,
}

pub fn draw(view: &FeedView<'_>, canvas: &mut Canvas<'_>, rect: Rect, theme: &Theme) {
    let top = rect.y - i32::from(view.offset);
    let len = view.rows.len();
    canvas.push_clip(rect);
    for index in 0..len {
        let band = Rect::new(
            rect.x,
            top + i32::from(index) * i32::from(ROW_H),
            rect.w,
            i32::from(ROW_H),
        );
        if band.bottom() <= rect.y || band.y >= rect.bottom() {
            continue;
        }
        let row = view.rows.row(index);
        let reveal = match view.reveal {
            Some((shown, by)) if shown == index => by,
            _ => 0,
        };
        SwipeAction {
            title: row.title,
            detail: row.detail,
            mark: row.mark,
            selected: view.selected == Some(index),
        }
        .draw(canvas, band, reveal, view.words.delete, theme);
    }

    let footer = Rect::new(
        rect.x,
        top + i32::from(len) * i32::from(ROW_H),
        rect.w,
        i32::from(FOOTER_H),
    );
    if footer.bottom() > rect.y && footer.y < rect.bottom() {
        let (word, fg) = match view.more {
            More::Idle => (view.words.load, theme.muted),
            More::Running => (view.words.load, theme.accent),
            More::End => (view.words.end, theme.muted),
            More::Failed => (view.words.failed, theme.danger),
        };
        centered(canvas, footer, word, fg);
    }

    if view.rows.is_empty() {
        let box_ = Rect::new(rect.x, rect.y + i32::from(view.pull), rect.w, rect.h);
        centered(canvas, box_, view.words.empty, theme.muted);
    }
    canvas.pop_clip();

    if view.pull > 0 || view.refresh == Refresh::Running {
        let pull = view.pull.max(0);
        let cx = rect.x + rect.w / 2;
        let cy = if view.pull > 0 {
            rect.y + i32::from(pull) / 2
        } else {
            rect.y + 14
        };
        let radius = 5 + i32::from(pull) / 8;
        canvas.set_color(Some(theme.muted));
        canvas.draw_circle(cx as isize, cy as isize, radius as isize);
        canvas.set_color(Some(theme.accent));
        canvas.draw_circle(cx as isize, cy as isize, (radius - 2).max(1) as isize);
        if view.refresh == Refresh::Running {
            canvas.set_color(Some(theme.accent));
            canvas.draw_text(
                view.words.refresh,
                (cx + radius + 6).max(0) as usize,
                (cy - 6).max(0) as usize,
            );
        }
    }
    canvas.set_color(None);
}

fn centered(canvas: &mut Canvas<'_>, rect: Rect, run: &[u8], fg: Color) {
    let width = canvas.measure(run) as i32;
    canvas.set_color(Some(fg));
    canvas.draw_text(
        run,
        (rect.x + (rect.w - width) / 2).max(0) as usize,
        (rect.y + (rect.h - 12) / 2).max(0) as usize,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: ScrollConfig = ScrollConfig {
        bounce: true,
        bar: false,
        pull: 44,
    };

    fn feed(rows: u16) -> Feed {
        let mut feed = Feed::default();
        feed.measure(Rect::new(0, 0, 100, 100), CONFIG);
        feed.set_rows(rows);
        feed
    }

    #[test]
    fn a_pull_past_the_threshold_asks_to_refresh() {
        let mut feed = feed(8);
        feed.drag(-60);
        assert!(feed.armed());
        assert_eq!(feed.release(0), Some(Edge::Refresh));
        assert_eq!(feed.refresh(), Refresh::Running);
        assert_eq!(feed.release(0), None, "one lift asks for one edge");
    }

    #[test]
    fn a_short_pull_comes_home_without_asking() {
        let mut feed = feed(8);
        feed.drag(-10);
        assert!(!feed.armed());
        assert_eq!(feed.release(0), None);
        for _ in 0..100 {
            feed.tick(20);
        }
        assert_eq!(feed.pull(), 0);
    }

    #[test]
    fn reaching_the_end_asks_for_more_once_per_row_count() {
        let mut feed = feed(8);
        feed.scroll(30, 0);
        assert_eq!(feed.release(0), Some(Edge::More));
        assert_eq!(
            feed.release(0),
            None,
            "the same row count is not asked twice"
        );
        feed.loaded(16, false);
        feed.scroll(40, 0);
        assert_eq!(feed.release(0), Some(Edge::More));
    }

    #[test]
    fn the_last_page_ends_the_footer() {
        let mut feed = feed(8);
        feed.loaded(16, true);
        assert_eq!(feed.more(), More::End);
        feed.scroll(30, 0);
        assert_eq!(feed.release(0), None, "an ended list asks for nothing");
    }

    #[test]
    fn a_failed_page_can_be_asked_again() {
        let mut feed = feed(8);
        feed.scroll(30, 0);
        assert_eq!(feed.release(0), Some(Edge::More));
        feed.failed();
        assert_eq!(feed.more(), More::Failed);
        feed.scroll(10, 0);
        assert_eq!(feed.release(0), Some(Edge::More));
    }

    #[test]
    fn a_row_count_past_the_axis_caps_the_travel() {
        assert_eq!(content(u16::MAX), i16::MAX);
    }

    #[test]
    fn fewer_rows_bring_a_scrolled_list_back_inside() {
        let mut feed = feed(20);
        feed.scroll(20, 0);
        assert!(feed.offset() > 0);
        feed.set_rows(1);
        assert_eq!(feed.offset(), 0);
    }
}
