//! The Text page: the face's coverage, a wrapped paragraph, and the glyph rows — the last two
//! as independent scroll regions.
//!
//! Both regions are named in the declaration and measured once at construction: the panel is a
//! fixed size and the content is static, so a headless layout is the same layout the page will
//! draw. The paragraph is broken once there too, and every line is declared as its own nowrap
//! run, so a frame never walks the face to break it again. A finger drags a region live; the
//! lift flings it. The header prints where that region currently sits.

use crate::shared::intent::{BusinessIntent, InputIntent, TextIntent};
use crate::shared::kit::scroll::{
    FLING_MAX_PX_S, STEP_LINES, ScrollConfig, ScrollState, fling_speed,
};
use crate::shared::model::{Command, DeviceState, Effects};
use crate::shared::snapshot::{
    FrameSnapshot, PageSnapshot, ScrollAreaView, TEXT_AREAS, TEXT_LINES, TEXT_ROWS, TextLine,
    TextSnapshot,
};
use iot_core::drivers::input::{GestureEvent, InputEvent, SwipeDirection};
use iot_core::ui::font::Face;
use iot_core::ui::panel::{CONTENT_BOTTOM, PANEL_COLUMNS};
use iot_core::ui::text::format_u16;
use iot_core::ui::{BoxConstraints, PX, Screen, ScrollId, Style, Ui};

use super::{Descriptor, Repaint};

const WRAP: ScrollId = ScrollId::new(1);
const GLYPHS: ScrollId = ScrollId::new(2);
const AREAS: [ScrollId; TEXT_AREAS] = [WRAP, GLYPHS];

/// The page's one switch for both regions: whether an edge gives, and whether a thumb is
/// drawn. Both are layout-neutral, so flipping either needs no re-measure.
const SCROLL: ScrollConfig = ScrollConfig {
    bounce: true,
    bar: true,
};

/// The thumb's width, and the air between it and the scroll box.
const BAR: i16 = 2;
const MIN_THUMB: i32 = 8;

/// Frames without a drag sample after which a held finger counts as stopped, not flung.
const STILL_STEPS: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Drag {
    id: u8,
    area: u8,
    y: i16,
    at_ms: u32,
    /// Offset pixels per second, positive when the content moves up.
    velocity: i16,
    idle: u8,
    moved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPage {
    scroll: ScrollState<TEXT_AREAS>,
    text: TextSnapshot,
    drag: Option<Drag>,
}

impl Default for TextPage {
    fn default() -> Self {
        // The region's width does not depend on its content, so one empty pass finds it and
        // the second pass measures the runs that width broke.
        let width = {
            let mut ui = headless();
            declare(&FrameSnapshot::default(), &mut ui);
            finish(&mut ui);
            ui.scroll_info(WRAP).map_or(0, |area| area.rect.w)
        };
        let text = text_layout(width);
        let mut snapshot = FrameSnapshot::default();
        snapshot.pages.text = text;
        let mut ui = headless();
        declare(&snapshot, &mut ui);
        finish(&mut ui);
        let mut scroll = ScrollState::default();
        scroll.measure(&ui, AREAS, SCROLL);
        Self {
            scroll,
            text,
            drag: None,
        }
    }
}

impl super::Service for TextPage {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        _state: &mut DeviceState,
    ) -> (Option<Command>, Effects) {
        let BusinessIntent::Text(intent) = intent else {
            return (None, Effects::default());
        };
        match *intent {
            TextIntent::Press { id, area, y } => {
                if let Some(old) = self.drag.take() {
                    self.scroll.fling(old.area, 0);
                }
                self.scroll.drag(area, 0);
                self.drag = Some(Drag {
                    id,
                    area,
                    y,
                    at_ms: 0,
                    velocity: 0,
                    idle: 0,
                    moved: false,
                });
            }
            TextIntent::Drag { id, x: _, y, at_ms } => {
                let Some(drag) = self.drag.as_mut() else {
                    return (None, Effects::default());
                };
                if drag.id != id {
                    return (None, Effects::default());
                }
                let y = i16::try_from(y).unwrap_or(i16::MAX);
                let delta = i32::from(drag.y) - i32::from(y);
                if drag.at_ms != 0 {
                    let dt = at_ms.saturating_sub(drag.at_ms).max(1);
                    let instant = (delta * 1000 / dt as i32)
                        .clamp(-i32::from(FLING_MAX_PX_S), i32::from(FLING_MAX_PX_S))
                        as i16;
                    drag.velocity = if drag.velocity == 0 {
                        instant
                    } else {
                        (drag.velocity + instant) / 2
                    };
                }
                self.scroll.drag(
                    drag.area,
                    delta.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
                );
                drag.y = y;
                drag.at_ms = at_ms;
                drag.idle = 0;
                drag.moved = true;
            }
            TextIntent::Release {
                id,
                area,
                lines,
                speed,
            } => {
                if let Some(drag) = self.drag
                    && drag.id == id
                {
                    self.drag = None;
                    if drag.moved {
                        let speed = if drag.idle <= STILL_STEPS {
                            drag.velocity
                        } else {
                            0
                        };
                        self.scroll.fling(drag.area, speed);
                    } else {
                        self.scroll.fling(drag.area, 0);
                    }
                    return (None, Effects::default());
                }
                self.scroll
                    .scroll(area, lines, if lines == 0 { 0 } else { speed });
            }
            TextIntent::Scroll { area, lines, speed } => {
                self.scroll.scroll(area, lines, speed);
            }
        }
        (None, Effects::default())
    }
}

impl super::LocalPage for TextPage {
    fn bind(&self, op: &InputIntent, _state: &DeviceState) -> Option<BusinessIntent> {
        let area = |x: u16, y: u16| self.scroll.area_at(i32::from(x), i32::from(y));
        match op.event {
            InputEvent::Gesture(GestureEvent::Press { id, x, y }) => {
                let area = area(x, y)?;
                Some(BusinessIntent::Text(TextIntent::Press {
                    id,
                    area,
                    y: i16::try_from(y).unwrap_or(i16::MAX),
                }))
            }
            InputEvent::Gesture(GestureEvent::Drag { id, x, y, at_ms }) => {
                Some(BusinessIntent::Text(TextIntent::Drag { id, x, y, at_ms }))
            }
            InputEvent::Gesture(GestureEvent::Swipe {
                id,
                direction,
                x,
                y,
                held_ms,
                distance_px,
                ..
            }) => {
                let (lines, speed) = match direction {
                    SwipeDirection::Up => (STEP_LINES, fling_speed(distance_px, held_ms)),
                    SwipeDirection::Down => (-STEP_LINES, fling_speed(distance_px, held_ms)),
                    _ => (0, 0),
                };
                let area = area(x, y)?;
                Some(BusinessIntent::Text(TextIntent::Release {
                    id,
                    area,
                    lines,
                    speed,
                }))
            }
            InputEvent::Gesture(GestureEvent::Tap { id, x, y, .. })
            | InputEvent::Gesture(GestureEvent::LongPress { id, x, y, .. }) => {
                let area = area(x, y)?;
                Some(BusinessIntent::Text(TextIntent::Release {
                    id,
                    area,
                    lines: 0,
                    speed: 0,
                }))
            }
            _ => None,
        }
    }

    fn publish(&self, out: &mut PageSnapshot) {
        let config = self.scroll.config();
        out.text.lines = self.text.lines;
        out.text.line_count = self.text.line_count;
        out.text.rows = self.text.rows;
        out.text.missing = self.text.missing;
        out.text.ascii = self.text.ascii;
        for (view, area) in out.text.areas.iter_mut().zip(self.scroll.areas().iter()) {
            let max = area.max();
            *view = ScrollAreaView {
                rect: area.rect,
                offset: area.offset,
                max,
                bar: config.bar && max > 0,
            };
        }
        out.text.moving = self.scroll.moving();
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        declare(snapshot, ui);
    }

    fn descriptor() -> Descriptor {
        Descriptor::new(Self::view).with_repaints(Repaint::WhenOrOverlay(text_moved))
    }

    fn tick(&mut self, step_ms: u16) {
        if let Some(drag) = self.drag.as_mut() {
            drag.idle = drag.idle.saturating_add(1);
        }
        self.scroll.tick(step_ms);
    }

    fn on_hide(&mut self) {
        self.scroll.reset();
        self.drag = None;
    }
}

fn headless() -> Ui<'static, 'static> {
    Ui::headless(PANEL_COLUMNS, CONTENT_BOTTOM, Screen::default())
}

fn finish(ui: &mut Ui<'_, '_>) {
    ui.layout(BoxConstraints::tight(
        PANEL_COLUMNS as i32 * PX,
        CONTENT_BOTTOM as i32 * PX,
    ));
}

fn text_layout(width: i32) -> TextSnapshot {
    let face = Face::new();
    let width = usize::try_from(width.max(1)).unwrap_or(1);
    let paragraph = iot_core::ui::typeset::wrap(WRAP_RUN, width, TEXT_LINES);
    let mut text = TextSnapshot::default();
    for (slot, line) in text
        .lines
        .iter_mut()
        .zip(paragraph.lines[..paragraph.count].iter())
    {
        *slot = TextLine {
            start: line.start as u16,
            end: line.end as u16,
            width: line.width as u16,
        };
    }
    text.line_count = paragraph.count as u8;
    for (slot, row) in text.rows.iter_mut().zip(ROWS) {
        *slot = face.measure(row) as u16;
    }
    text.missing = missing(&face);
    text.ascii = ascii_complete(&face);
    text
}

fn text_moved(prev: &FrameSnapshot, next: &FrameSnapshot) -> bool {
    prev.pages.text != next.pages.text
}

fn declare(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
    let text = snapshot.pages.text;
    crate::shared::kit::chrome::page(ui, |ui| {
        coverage(ui, text);
        region(ui, WRAP, b"WRAP", text.areas[0], |ui| {
            for line in &text.lines[..usize::from(text.line_count)] {
                let run = &WRAP_RUN[usize::from(line.start)..usize::from(line.end)];
                ui.text(run, Style::new().nowrap().width(width_of(line.width)));
            }
        });
        region(ui, GLYPHS, b"GLYPHS", text.areas[1], |ui| {
            for (row, width) in ROWS.iter().zip(text.rows) {
                ui.text(row, Style::new().nowrap().width(width_of(width)));
            }
        });
    });
}

fn width_of(width: u16) -> i16 {
    i16::try_from(width).unwrap_or(i16::MAX)
}

fn coverage(ui: &mut Ui<'_, '_>, text: TextSnapshot) {
    let mut buf = [0u8; 48];
    let mut digits = [0u8; 6];
    let mut n = append(&mut buf, 0, b"TEXT 12PX 6/12 ASCII ");
    let ascii: &[u8] = if text.ascii { b"95/95" } else { b"---" };
    n = append(&mut buf, n, ascii);
    n = append(&mut buf, n, b" MISS ");
    n = append(&mut buf, n, format_u16(text.missing, &mut digits));
    ui.text(&buf[..n], Style::new().nowrap());
}

/// One region: a fixed header, then the scroll box and its gutter. The region columns grow by
/// `basis(0)` so two of them split the leftover evenly instead of by their content heights.
fn region(
    ui: &mut Ui<'_, '_>,
    id: ScrollId,
    label: &[u8],
    view: ScrollAreaView,
    body: impl FnOnce(&mut Ui<'_, '_>),
) {
    ui.column(Style::new().grow(1).basis(0).stretch().gap(1), |ui| {
        header(ui, label, view);
        ui.row(Style::new().grow(1).basis(0).stretch().gap(BAR), |ui| {
            ui.scroll(
                Style::new()
                    .grow(1)
                    .basis(0)
                    .scroll_area(id)
                    .scroll_offset(view.offset),
                body,
            );
            ui.stack(Style::new().width(BAR), |ui| {
                if view.bar {
                    thumb(ui, view);
                }
            });
        });
    });
}

fn header(ui: &mut Ui<'_, '_>, label: &[u8], view: ScrollAreaView) {
    let mut buf = [0u8; 24];
    let mut digits = [0u8; 6];
    let mut n = append(&mut buf, 0, label);
    n = append(&mut buf, n, b" ");
    let offset = u16::try_from(view.offset.max(0)).unwrap_or(0);
    n = append(&mut buf, n, format_u16(offset, &mut digits));
    n = append(&mut buf, n, b"/");
    let max = u16::try_from(view.max).unwrap_or(0);
    n = append(&mut buf, n, format_u16(max, &mut digits));
    ui.text(&buf[..n], Style::new().nowrap());
}

/// A thumb beside the scroll box: its length is the viewport's share of the content, and its
/// position is the offset's share of the travel. A bordered 2 px box reads solid.
fn thumb(ui: &mut Ui<'_, '_>, view: ScrollAreaView) {
    let viewport = view.rect.h;
    let max = i32::from(view.max);
    if viewport <= 0 || max <= 0 {
        return;
    }
    let floor = MIN_THUMB.min(viewport);
    let height = (viewport * viewport / (viewport + max)).clamp(floor, viewport);
    let travel = viewport - height;
    let top = travel * i32::from(view.offset).clamp(0, max) / max;
    ui.node(
        Style::new()
            .absolute()
            .top(i16::try_from(top).unwrap_or(0))
            .width(BAR)
            .height(i16::try_from(height).unwrap_or(0))
            .bordered(),
    );
}

fn append(buf: &mut [u8], n: usize, part: &[u8]) -> usize {
    let take = part.len().min(buf.len().saturating_sub(n));
    buf[n..n + take].copy_from_slice(&part[..take]);
    n + take
}

/// A long mixed run for the wrap region: every class the breaker meets, in one paragraph,
/// including a `\n` and a `\r\n` hard break.
const WRAP_RUN: &[u8] = "面板文字排版：这一段检验自动换行。The quick brown fox jumps over the lazy dog, 0123456789. 中文与 English 混排时，断点要落在词与词之间，不能把一个汉字或一个单词劈成两半。标点：句号。逗号，问号？叹号！都应避开行首；左引号「和左括号（不能留在行尾。窄屏阅读靠每行对齐到同一列，段落行距保持十二像素。A swipe scrolls this region: the offset moves, the edge gives, and the thumb follows. 繁體：網路設定、無線路由器、中華電信。希腊 αβγδε，西里尔 абвгд，假名 あいうえお。最后一行以完整的句子结束，方便确认换行没有吞字。换行符：第一行\n第二行\r\n第三行。".as_bytes();

/// The page's standing rows, one per line the panel has room for. Widths are asserted below.
const ROWS: [&[u8]; TEXT_ROWS] = [
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZ",
    b"abcdefghijklmnopqrstuvwxyz",
    b"0123456789 `~!@#$%^&*()",
    b"_+-=[]{}|\\;:'\",<.>/?   ",
    "i=6 M=6 W=6 0=6 中=12 ，=12 …=12".as_bytes(),
    b"iiiiiiiiiiiiiiiiiiii|",
    b"MMMMMMMMMMMMMMMMMMMM|",
    b"00000000000000000000|",
    "中文中文中文中文中文|".as_bytes(),
    "简体：中文网络设置测试无线路由器".as_bytes(),
    "繁體：中文網路設定測試無線路由器".as_bytes(),
    "繁體：臺灣中華電信我的最愛分享".as_bytes(),
    "混合：中A文B中C文D 12345 abc".as_bytes(),
    "标点：，。！？；：、“”‘’《》…—·「」（）：".as_bytes(),
    "希腊 αβγδε 西里尔 абвгд".as_bytes(),
    "假名 あいうえお アイウエオ".as_bytes(),
    "符号 □=缺 …=省略 ←↑→↓ ┌─┐".as_bytes(),
    "漢字：中文繁體简体 网络網路 成功".as_bytes(),
];

fn ascii_complete(face: &Face) -> bool {
    (0x20..=0x7E).all(|code| face.has(char::from_u32(code).expect("ascii")))
}

fn missing(face: &Face) -> u16 {
    let mut count = 0u16;
    for run in ROWS.iter().copied().chain([WRAP_RUN]) {
        for ch in core::str::from_utf8(run).unwrap_or_default().chars() {
            if !ch.is_control() && !face.has(ch) {
                count = count.saturating_add(1);
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{LocalPage, Service};
    use crate::shared::kit::scroll::LINE_PX;

    fn gesture(event: GestureEvent) -> InputIntent {
        InputIntent {
            source: 0,
            event: InputEvent::Gesture(event),
        }
    }

    fn swipe(direction: SwipeDirection, x: u16, y: u16) -> InputIntent {
        gesture(GestureEvent::Swipe {
            id: 0,
            direction,
            x,
            y,
            end_x: x,
            end_y: y,
            held_ms: 100,
            distance_px: 120,
        })
    }

    fn center(area: &iot_core::ui::ScrollInfo) -> (u16, u16) {
        (
            u16::try_from(area.rect.x + area.rect.w / 2).expect("a panel x"),
            u16::try_from(area.rect.y + area.rect.h / 2).expect("a panel y"),
        )
    }

    fn apply(page: &mut TextPage, intent: TextIntent) {
        let mut state = DeviceState::default();
        page.apply(&BusinessIntent::Text(intent), &mut state);
    }

    #[test]
    fn the_sample_text_breaks_at_both_kinds_of_newline() {
        let page = TextPage::default();
        let lines = &page.text.lines[..usize::from(page.text.line_count)];
        let rendered: alloc::vec::Vec<&str> = lines
            .iter()
            .map(|line| {
                core::str::from_utf8(&WRAP_RUN[usize::from(line.start)..usize::from(line.end)])
                    .expect("the run is UTF-8")
            })
            .collect();
        assert!(rendered.iter().any(|line| line.contains("第一行")));
        assert!(rendered.iter().any(|line| line.contains("第二行")));
        assert!(rendered.iter().any(|line| line.contains("第三行")));
        assert!(
            !rendered
                .iter()
                .any(|line| line.contains('\n') || line.contains('\r')),
            "a control character never reaches a line"
        );
    }

    #[test]
    fn the_row_count_matches_the_snapshot() {
        assert_eq!(ROWS.len(), TEXT_ROWS);
    }

    #[test]
    fn every_row_fits_the_glyph_region() {
        let page = TextPage::default();
        let face = Face::new();
        let width = usize::try_from(page.scroll.areas()[1].rect.w).expect("a panel width");
        assert!(width > 0);
        for row in ROWS {
            assert!(
                face.measure(row) <= width,
                "{:?} is {} px",
                core::str::from_utf8(row).unwrap_or("?"),
                face.measure(row)
            );
        }
    }

    #[test]
    fn both_regions_have_somewhere_to_go() {
        let page = TextPage::default();
        for area in page.scroll.areas() {
            assert!(area.max() > 0, "region {:?} fits its box", area.id);
        }
    }

    #[test]
    fn the_regions_scroll_independently() {
        let mut page = TextPage::default();
        page.scroll.scroll(0, STEP_LINES, 0);
        let wrap = page.scroll.areas()[0].offset;
        assert_eq!(wrap, i16::from(STEP_LINES) * LINE_PX);
        assert_eq!(page.scroll.areas()[1].offset, 0);
        page.scroll.scroll(1, STEP_LINES, 0);
        assert_eq!(
            page.scroll.areas()[1].offset,
            i16::from(STEP_LINES) * LINE_PX
        );
        assert_eq!(page.scroll.areas()[0].offset, wrap);
    }

    #[test]
    fn a_swipe_inside_a_region_asks_that_region_to_release() {
        let page = TextPage::default();
        let (x, y) = center(&page.scroll.areas()[1]);
        assert_eq!(
            page.bind(&swipe(SwipeDirection::Up, x, y), &DeviceState::default()),
            Some(BusinessIntent::Text(TextIntent::Release {
                id: 0,
                area: 1,
                lines: STEP_LINES,
                speed: 1_200,
            }))
        );
    }

    #[test]
    fn a_horizontal_swipe_releases_without_stepping_and_an_outside_one_is_ignored() {
        let page = TextPage::default();
        let (x, y) = center(&page.scroll.areas()[0]);
        assert_eq!(
            page.bind(&swipe(SwipeDirection::Left, x, y), &DeviceState::default()),
            Some(BusinessIntent::Text(TextIntent::Release {
                id: 0,
                area: 0,
                lines: 0,
                speed: 0,
            }))
        );
        assert_eq!(
            page.bind(&swipe(SwipeDirection::Up, 0, 0), &DeviceState::default()),
            None
        );
    }

    #[test]
    fn a_drag_moves_the_region_with_the_finger() {
        let mut page = TextPage::default();
        let (x, y) = center(&page.scroll.areas()[1]);
        let press = page
            .bind(
                &gesture(GestureEvent::Press { id: 0, x, y }),
                &DeviceState::default(),
            )
            .expect("a press inside the region");
        apply(&mut page, expect_text(press));
        let drag = page
            .bind(
                &gesture(GestureEvent::Drag {
                    id: 0,
                    x,
                    y: y - 24,
                    at_ms: 100,
                }),
                &DeviceState::default(),
            )
            .expect("a drag");
        apply(&mut page, expect_text(drag));
        assert_eq!(page.scroll.areas()[1].offset, 24, "the content follows up");
    }

    #[test]
    fn a_release_after_a_drag_flings_without_stepping() {
        let mut page = TextPage::default();
        let (x, y) = center(&page.scroll.areas()[1]);
        apply(
            &mut page,
            TextIntent::Press {
                id: 0,
                area: 1,
                y: i16::try_from(y).unwrap(),
            },
        );
        apply(
            &mut page,
            TextIntent::Drag {
                id: 0,
                x,
                y: y - 20,
                at_ms: 100,
            },
        );
        apply(
            &mut page,
            TextIntent::Drag {
                id: 0,
                x,
                y: y - 40,
                at_ms: 120,
            },
        );
        let moved = page.scroll.areas()[1].offset;
        apply(
            &mut page,
            TextIntent::Release {
                id: 0,
                area: 1,
                lines: STEP_LINES,
                speed: 1_600,
            },
        );
        assert!(page.scroll.areas()[1].offset >= moved, "no step was added");
        assert!(page.scroll.moving(), "the release flings");
    }

    #[test]
    fn a_fast_swipe_without_a_drag_still_steps() {
        let mut page = TextPage::default();
        apply(
            &mut page,
            TextIntent::Release {
                id: 0,
                area: 1,
                lines: STEP_LINES,
                speed: 0,
            },
        );
        assert_eq!(
            page.scroll.areas()[1].offset,
            i16::from(STEP_LINES) * LINE_PX
        );
    }

    #[test]
    fn a_tap_after_a_drag_does_not_step() {
        let mut page = TextPage::default();
        let (x, y) = center(&page.scroll.areas()[1]);
        apply(
            &mut page,
            TextIntent::Press {
                id: 0,
                area: 1,
                y: i16::try_from(y).unwrap(),
            },
        );
        apply(
            &mut page,
            TextIntent::Drag {
                id: 0,
                x,
                y: y - 10,
                at_ms: 100,
            },
        );
        let moved = page.scroll.areas()[1].offset;
        apply(
            &mut page,
            TextIntent::Release {
                id: 0,
                area: 1,
                lines: 0,
                speed: 0,
            },
        );
        assert_eq!(page.scroll.areas()[1].offset, moved);
    }

    #[test]
    fn hiding_the_page_drops_the_offsets_and_the_glide() {
        let mut page = TextPage::default();
        page.scroll.scroll(0, STEP_LINES, 1_600);
        assert!(page.scroll.moving());
        page.on_hide();
        assert_eq!(page.scroll.areas()[0].offset, 0);
        assert!(!page.scroll.moving());
    }

    #[test]
    fn the_sample_rows_have_no_missing_glyphs() {
        assert_eq!(missing(&Face::new()), 0);
        assert!(ascii_complete(&Face::new()));
    }

    #[test]
    fn the_declaration_fits_the_node_arena() {
        let page = TextPage::default();
        let mut snapshot = FrameSnapshot::default();
        snapshot.pages.text = page.text;
        let mut ui = headless();
        declare(&snapshot, &mut ui);
        assert!(
            ui.node_count() <= 48,
            "the text page declared {} nodes",
            ui.node_count()
        );
    }

    #[test]
    fn the_ascii_rows_are_one_cell_wide() {
        let face = Face::new();
        for row in &ROWS[..4] {
            for ch in core::str::from_utf8(row).unwrap_or_default().chars() {
                if ch != ' ' {
                    assert_eq!(face.advance(ch), 6, "{ch} is not one cell");
                }
            }
        }
    }

    #[test]
    fn the_bars_land_on_the_same_column() {
        let face = Face::new();
        let ends: alloc::vec::Vec<usize> = ROWS[5..9]
            .iter()
            .map(|row| face.measure(&row[..row.len() - 1]))
            .collect();
        assert_eq!(ends, [120, 120, 120, 120]);
    }

    fn expect_text(intent: BusinessIntent) -> TextIntent {
        match intent {
            BusinessIntent::Text(intent) => intent,
            other => panic!("expected a text intent, got {other:?}"),
        }
    }
}
