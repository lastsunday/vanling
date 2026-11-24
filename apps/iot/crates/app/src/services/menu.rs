//! The Menu page: every page the board can back, one button away, three to a row.

use iot_core::drivers::input::{GestureEvent, InputEvent, SwipeDirection};
use iot_core::ui::panel::TOUCH_MIN;
use iot_core::ui::{Rect, ScrollId, Style, Ui};

use crate::shared::i18n::{Lang, Msg};
use crate::shared::intent::{BusinessIntent, InputIntent, MenuIntent, SettingsIntent};
use crate::shared::model::{Command, DeviceState, Effects};
use crate::shared::page;
use crate::shared::route::PageId;
use crate::shared::snapshot::{
    FrameSnapshot, MenuSnapshot, PageSnapshot, ScrollAreaView, ScrollShift,
};
use crate::shared::surface::{finish, headless};
use crate::shared::ui::scroll::{
    BAR, DRAG_SLOP, Drag, Press, STEP_LINES, ScrollConfig, ScrollState, fling_speed, lift,
};
use crate::shared::ui::state::State;
use crate::shared::ui::tag::TagTable;
use crate::shared::ui::widgets::{Button, Chip};

use super::{Descriptor, Repaint};

const AREA: ScrollId = ScrollId::new(1);
const COLUMNS: usize = 3;
const BUTTON_W: i16 = 98;
const GAP: i16 = 6;
const COUNT: usize = PageId::ALL.len() - 2;
const ROWS: usize = COUNT.div_ceil(COLUMNS);

/// The language chips sit outside the grid, so their tags stay clear of the page codes.
const TAG_LANG_EN: u16 = 100;
const TAG_LANG_ZH: u16 = 101;
const TAGS: usize = COUNT + 2;

const SCROLL: ScrollConfig = ScrollConfig {
    bounce: true,
    bar: true,
    pull: 0,
};

/// A scroll frame blits the pixels already in the buffer, so it fits the loop's 20 ms cadence.
const REPAINT_MS: u64 = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuPage {
    scroll: ScrollState<1>,
    drag: Option<Drag>,
    press: Option<Press>,
    tags: TagTable<TAGS>,
}

impl Default for MenuPage {
    fn default() -> Self {
        let mut ui = headless();
        declare(&FrameSnapshot::default(), &mut ui);
        finish(&mut ui);
        let mut scroll = ScrollState::default();
        scroll.measure(&ui, [AREA], SCROLL);
        Self {
            scroll,
            drag: None,
            press: None,
            tags: TagTable::measure(&ui),
        }
    }
}

impl MenuPage {
    fn claim(&mut self, id: u8, area: u8, y: i16, at_ms: u32) -> bool {
        if self.drag.is_some() {
            return false;
        }
        self.scroll.drag(area, 0);
        self.drag = Some(Drag::new(id, area, y, at_ms));
        true
    }

    /// The point at the centre of a listed page's button.
    #[cfg(test)]
    pub(crate) fn button_centre(&self, page: PageId) -> Option<(u16, u16)> {
        let rect = self.tags.rect(page.code() as u16 + 1)?;
        let offset = i32::from(self.scroll.areas()[0].offset);
        Some((
            u16::try_from(rect.x + rect.w / 2).ok()?,
            u16::try_from(rect.y + rect.h / 2 - offset).ok()?,
        ))
    }

    fn snapshot(&self) -> MenuSnapshot {
        let area = self.scroll.areas()[0];
        let max = area.max();
        MenuSnapshot {
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
                bar: self.scroll.config().bar && max > 0,
            },
            moving: self.scroll.moving(),
        }
    }

    /// The tag of the button or chip a panel point lands on; zero on a gap.
    fn tag_at(&self, x: i32, y: i32) -> u16 {
        for (tag, rect) in self.tags.iter().filter(|(tag, _)| *tag >= TAG_LANG_EN) {
            if rect.contains(x, y) {
                return tag;
            }
        }
        let area = self.scroll.areas()[0];
        if !area.rect.contains(x, y) {
            return 0;
        }
        self.tags.at(x, y, area.offset)
    }
}

impl super::Service for MenuPage {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        _state: &mut DeviceState,
    ) -> (Option<Command>, Effects) {
        let BusinessIntent::Menu(intent) = intent else {
            return (None, Effects::default());
        };
        match *intent {
            // The manager turns a choice into navigation; the page owns no destination.
            MenuIntent::Choose { .. } => {}
            MenuIntent::Press { id, tag, x, y } => {
                // A press over a button is only remembered: a tap that never moves still has
                // to reach that button, while the first drag claims the region beneath it.
                let x = i16::try_from(x).unwrap_or(i16::MAX);
                let y = i16::try_from(y).unwrap_or(i16::MAX);
                self.press = Some(Press { id, tag, x, y });
                if tag == 0
                    && let Some(area) = self.scroll.area_at(i32::from(x), i32::from(y))
                {
                    self.claim(id, area, y, 0);
                    self.press = None;
                }
            }
            MenuIntent::Drag { id, x, y, at_ms } => {
                let y = i16::try_from(y).unwrap_or(i16::MAX);
                if let Some(drag) = self.drag.as_mut().filter(|drag| drag.id == id) {
                    let delta = drag.step(y, at_ms);
                    self.scroll.drag(drag.area, delta);
                } else if let Some(press) = self.press.take().filter(|press| press.id == id) {
                    if let Some(area) = self.scroll.area_at(i32::from(press.x), i32::from(press.y))
                    {
                        self.scroll.drag(area, 0);
                        let mut drag = Drag::new(id, area, press.y, 0);
                        let delta = drag.step(y, at_ms);
                        self.scroll.drag(area, delta);
                        self.drag = Some(drag);
                    }
                } else if let Some(area) = self.scroll.area_at(i32::from(x), i32::from(y)) {
                    self.claim(id, area, y, at_ms);
                }
            }
            MenuIntent::Release {
                id,
                area,
                lines,
                speed,
                at_ms,
            } => {
                self.press = None;
                if let Some((area, speed)) = lift(&mut self.drag, id, at_ms) {
                    self.scroll.fling(area, speed);
                }
                self.scroll
                    .scroll(area, lines, if lines == 0 { 0 } else { speed });
            }
            MenuIntent::Scroll { area, lines, speed } => {
                self.scroll.scroll(area, lines, speed);
            }
        }
        (None, Effects::default())
    }
}

impl super::LocalPage for MenuPage {
    fn bind(&self, op: &InputIntent, state: &DeviceState) -> Option<BusinessIntent> {
        let tracked = self.drag.as_ref().map(|drag| drag.area);
        let area = |x: u16, y: u16| self.scroll.area_at(i32::from(x), i32::from(y));
        match op.event {
            InputEvent::Gesture(GestureEvent::Tap {
                id, x, y, at_ms, ..
            }) => {
                if let Some(area) = tracked {
                    return Some(BusinessIntent::Menu(MenuIntent::Release {
                        id,
                        area,
                        lines: 0,
                        speed: 0,
                        at_ms: Some(at_ms),
                    }));
                }
                let tag = self.tag_at(i32::from(x), i32::from(y));
                if let Some(lang) = match tag {
                    TAG_LANG_EN => Some(Lang::En),
                    TAG_LANG_ZH => Some(Lang::ZhHans),
                    _ => None,
                } {
                    return Some(BusinessIntent::Settings(SettingsIntent::SetLang { lang }));
                }
                let page = PageId::from_code(u8::try_from(tag.checked_sub(1)?).ok()?)?;
                page.is_backed(state.backed)
                    .then_some(BusinessIntent::Menu(MenuIntent::Choose {
                        page: page.code(),
                    }))
            }
            InputEvent::Gesture(GestureEvent::Press { id, x, y }) => {
                let tag = self.tag_at(i32::from(x), i32::from(y));
                if tag == 0 && area(x, y).is_none() {
                    return None;
                }
                Some(BusinessIntent::Menu(MenuIntent::Press { id, tag, x, y }))
            }
            InputEvent::Gesture(GestureEvent::Drag { id, x, y, at_ms }) => {
                if let Some(press) = self.press.filter(|press| press.id == id) {
                    let dx = i32::from(x) - i32::from(press.x);
                    let dy = i32::from(y) - i32::from(press.y);
                    if dx.abs() < i32::from(DRAG_SLOP) && dy.abs() < i32::from(DRAG_SLOP) {
                        return None;
                    }
                }
                Some(BusinessIntent::Menu(MenuIntent::Drag { id, x, y, at_ms }))
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
                let area = tracked.or_else(|| area(x, y))?;
                Some(BusinessIntent::Menu(MenuIntent::Release {
                    id,
                    area,
                    lines,
                    speed,
                    at_ms: Some(at_ms),
                }))
            }
            InputEvent::Gesture(GestureEvent::LongPress { id, x, y, .. }) => {
                let area = tracked.or_else(|| area(x, y))?;
                Some(BusinessIntent::Menu(MenuIntent::Release {
                    id,
                    area,
                    lines: 0,
                    speed: 0,
                    at_ms: None,
                }))
            }
            _ => None,
        }
    }

    fn publish(&self, out: &mut PageSnapshot) {
        out.menu = self.snapshot();
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        declare(snapshot, ui);
    }

    fn descriptor() -> Descriptor {
        Descriptor::new(Self::view)
            .with_repaint_ms(REPAINT_MS)
            .with_repaints(Repaint::WhenOrOverlay(menu_moved))
            .with_scroll(menu_shift)
    }

    fn tick(&mut self, step_ms: u16) {
        self.scroll.tick(step_ms);
    }

    fn on_hide(&mut self) {
        self.scroll.reset();
        self.drag = None;
        self.press = None;
    }
}

fn declare(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
    let lang = snapshot.lang;
    let backed = snapshot.backed;
    page::body(ui, |ui| {
        ui.row(
            Style::new()
                .height(TOUCH_MIN as i16 + GAP)
                .padding(0, 0, 0, GAP)
                .middle()
                .gap(GAP),
            |ui| {
                page::title(ui, Msg::MenuTitle.bytes(lang));
                ui.node(Style::new().grow(1));
                ui.add(
                    Chip::new(Msg::LangEn.bytes(lang))
                        .width(TOUCH_MIN as i16)
                        .height(TOUCH_MIN as i16)
                        .tag(TAG_LANG_EN)
                        .selected(lang == Lang::En),
                );
                ui.add(
                    Chip::new(Msg::LangZh.bytes(lang))
                        .width(TOUCH_MIN as i16)
                        .height(TOUCH_MIN as i16)
                        .tag(TAG_LANG_ZH)
                        .selected(lang == Lang::ZhHans),
                );
            },
        );
        page::scroller(ui, AREA, snapshot.pages.menu.area, |ui| {
            for row in 0..ROWS {
                ui.row(Style::new().gap(GAP), |ui| {
                    for column in 0..COLUMNS {
                        let Some(page) = entry(row * COLUMNS + column) else {
                            continue;
                        };
                        let state = if page.is_backed(backed) {
                            State::NONE
                        } else {
                            State::DISABLED
                        };
                        ui.add(
                            Button::new(page_name(page).bytes(lang))
                                .width(BUTTON_W)
                                .tag(page.code() as u16 + 1)
                                .state(state),
                        );
                    }
                });
            }
        });
    });
}

fn menu_moved(prev: &FrameSnapshot, next: &FrameSnapshot) -> bool {
    prev.lang != next.lang || prev.backed != next.backed || prev.pages.menu != next.pages.menu
}

fn menu_shift(prev: &FrameSnapshot, next: &FrameSnapshot) -> Option<ScrollShift> {
    if prev.page != PageId::Menu || next.page != PageId::Menu {
        return None;
    }
    let areas = [next.pages.menu.area];
    let mut before = prev.pages.menu;
    let mut after = next.pages.menu;
    let shift = after.area.offset.wrapping_sub(before.area.offset);
    before.area.offset = 0;
    after.area.offset = 0;
    if before.area.rect.w <= 0 || before.area.rect.h <= 0 {
        return None;
    }
    let moving_ended = before.moving && !after.moving;
    before.moving = false;
    after.moving = false;
    if before != after {
        return None;
    }
    if !moving_ended && shift == 0 {
        return None;
    }
    Some(ScrollShift::new(&areas, &[shift]))
}

/// The page's own name, in the menu's words.
fn page_name(page: PageId) -> Msg {
    match page {
        PageId::Menu => Msg::PageMenu,
        PageId::Input => Msg::PageInput,
        PageId::Attitude => Msg::PageAttitude,
        PageId::Audio => Msg::PageAudio,
        PageId::Speaker => Msg::PageSpeaker,
        PageId::Camera => Msg::PageCamera,
        PageId::Network => Msg::PageNetwork,
        PageId::Splash => Msg::Brand,
        PageId::Env => Msg::PageEnv,
        PageId::Text => Msg::PageText,
        PageId::Widgets => Msg::PageWidgets,
        PageId::Advanced => Msg::PageAdvanced,
        PageId::Feedback => Msg::PageFeedback,
        PageId::Extended => Msg::PageExtended,
        PageId::Data => Msg::PageData,
        PageId::List => Msg::PageList,
    }
}

/// Every page the menu offers, in the grid's own order. A board that cannot back one still
/// lists it, greyed, so the grid does not change shape with the wiring.
fn entries() -> impl Iterator<Item = PageId> {
    PageId::ALL
        .into_iter()
        .filter(|page| !page.is_transient() && *page != PageId::Menu)
}

fn entry(index: usize) -> Option<PageId> {
    entries().nth(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{LocalPage, Service};
    use iot_core::drivers::board::Backed;
    use iot_core::drivers::motion::MotionCapabilities;
    use iot_core::ui::panel::{CONTENT_BOTTOM, PANEL_COLUMNS};
    use iot_core::ui::{PX, Screen};

    const ALL: Backed = Backed {
        motion: Some(MotionCapabilities::EMPTY),
        audio: true,
        playback: true,
        camera: true,
        network: true,
    };

    fn op(event: GestureEvent) -> InputIntent {
        InputIntent {
            source: 0,
            event: InputEvent::Gesture(event),
        }
    }

    fn tap(x: u16, y: u16) -> InputIntent {
        op(GestureEvent::Tap {
            id: 0,
            x,
            y,
            held_ms: 0,
            at_ms: 0,
        })
    }

    fn page_at(page: &MenuPage, x: i32, y: i32) -> Option<PageId> {
        PageId::from_code(u8::try_from(page.tag_at(x, y).checked_sub(1)?).ok()?)
    }

    /// Brings `content_y` into the middle of the box, so a row below the fold can be touched.
    fn scroll_to(page: &mut MenuPage, content_y: i32) {
        let area = page.scroll.areas()[0];
        let max = i32::from(area.max());
        let target = (content_y - (area.rect.y + area.rect.h / 2)).clamp(0, max);
        let delta = i16::try_from(target - i32::from(area.offset)).unwrap_or(0);
        page.scroll.drag(0, delta);
    }

    #[test]
    fn the_menu_lists_every_page_but_itself_and_the_splash() {
        let listed = entries().collect::<alloc::vec::Vec<_>>();
        assert_eq!(listed.len(), COUNT);
        assert!(!listed.contains(&PageId::Menu));
        assert!(!listed.contains(&PageId::Splash));
        assert!(listed.contains(&PageId::Camera));
    }

    #[test]
    fn a_language_chip_asks_for_its_language() {
        let page = MenuPage::default();
        for (tag, lang) in [(TAG_LANG_EN, Lang::En), (TAG_LANG_ZH, Lang::ZhHans)] {
            let rect = page.tags.rect(tag).expect("the chip is laid out");
            assert_eq!(
                page.bind(
                    &tap((rect.x + 2) as u16, (rect.y + 2) as u16),
                    &DeviceState::default()
                ),
                Some(BusinessIntent::Settings(SettingsIntent::SetLang { lang }))
            );
        }
    }

    #[test]
    fn the_language_chips_are_touch_sized() {
        let page = MenuPage::default();
        for tag in [TAG_LANG_EN, TAG_LANG_ZH] {
            let rect = page.tags.rect(tag).expect("the chip is laid out");
            assert!(
                rect.w >= TOUCH_MIN as i32 && rect.h >= TOUCH_MIN as i32,
                "chip {tag} is {}x{}",
                rect.w,
                rect.h
            );
        }
    }

    #[test]
    fn a_button_centre_maps_to_the_page_drawn_there() {
        let mut page = MenuPage::default();
        for expected in entries() {
            let rect = page
                .tags
                .rect(expected.code() as u16 + 1)
                .expect("the page is listed");
            scroll_to(&mut page, rect.y + rect.h / 2);
            let (x, y) = page.button_centre(expected).expect("the page is listed");
            assert_eq!(page_at(&page, i32::from(x), i32::from(y)), Some(expected));
        }
    }

    #[test]
    fn the_gaps_between_buttons_are_not_buttons() {
        let mut page = MenuPage::default();
        let area = page.scroll.areas()[0].rect;
        assert_eq!(page_at(&page, area.x, area.y), entry(0));
        assert_eq!(page_at(&page, area.x + i32::from(BUTTON_W), area.y), None);
        assert_eq!(
            page_at(&page, area.x, area.y + i32::from(TOUCH_MIN as i16)),
            None
        );
        assert_eq!(page_at(&page, area.x - 1, area.y), None);

        let last = page
            .tags
            .rect(entry(COUNT - 2).expect("a page").code() as u16 + 1)
            .expect("the last row is laid out");
        scroll_to(&mut page, last.y + last.h / 2);
        let area = page.scroll.areas()[0].rect;
        assert_eq!(
            page_at(&page, area.x, area.bottom() - 1),
            entry(COUNT - 2),
            "the last row starts under the first column"
        );
        assert_eq!(
            page_at(
                &page,
                area.x + i32::from(BUTTON_W) + i32::from(GAP),
                area.bottom() - 1
            ),
            entry(COUNT - 1),
            "the last button sits under the second column"
        );
        assert_eq!(
            page_at(&page, area.right() - 1, area.bottom() - 1),
            None,
            "the last row has two buttons, not three"
        );
    }

    fn apply(page: &mut MenuPage, intent: BusinessIntent) {
        let mut state = DeviceState::default();
        page.apply(&intent, &mut state);
    }

    #[test]
    fn a_press_that_never_moves_still_reaches_its_button() {
        let mut page = MenuPage::default();
        let wired = DeviceState {
            backed: ALL,
            ..DeviceState::default()
        };
        let (x, y) = page.button_centre(PageId::Env).expect("`Env` is listed");
        let press = page
            .bind(&op(GestureEvent::Press { id: 0, x, y }), &wired)
            .expect("the press lands");
        apply(&mut page, press);
        assert_eq!(
            page.bind(&tap(x, y), &wired),
            Some(BusinessIntent::Menu(MenuIntent::Choose {
                page: PageId::Env.code()
            })),
            "a tap is press then tap on the wire; the press must not swallow it"
        );
    }

    #[test]
    fn a_page_the_board_cannot_back_is_listed_but_does_not_navigate() {
        let page = MenuPage::default();
        let (x, y) = page
            .button_centre(PageId::Camera)
            .expect("`Camera` is listed");
        assert_eq!(page.bind(&tap(x, y), &DeviceState::default()), None);
        let wired = DeviceState {
            backed: ALL,
            ..DeviceState::default()
        };
        assert_eq!(
            page.bind(&tap(x, y), &wired),
            Some(BusinessIntent::Menu(MenuIntent::Choose {
                page: PageId::Camera.code()
            }))
        );
    }

    #[test]
    fn every_name_fits_a_button() {
        let metrics = iot_core::ui::font::Metrics::vector(16);
        for page in PageId::ALL {
            for lang in crate::shared::i18n::Lang::ALL {
                let name = page_name(page).bytes(*lang);
                let width = metrics.measure(name);
                assert!(
                    width <= usize::try_from(BUTTON_W - 12).unwrap(),
                    "{page:?} in {lang:?} is {width} px wide"
                );
            }
        }
    }

    #[test]
    fn every_page_has_a_name() {
        for page in PageId::ALL {
            for lang in crate::shared::i18n::Lang::ALL {
                assert!(!page_name(page).bytes(*lang).is_empty(), "{page:?}");
            }
        }
    }

    #[test]
    fn a_backed_board_prints_its_pages_and_a_bare_one_does_not() {
        let draw = |backed| {
            let mut frame = alloc::vec![0u8; PANEL_COLUMNS * CONTENT_BOTTOM * 2];
            {
                let canvas =
                    iot_core::ui::Canvas::new(&mut frame, PANEL_COLUMNS, CONTENT_BOTTOM, None);
                let mut panel = crate::shared::surface::Panel::new(canvas, Screen::default());
                let snapshot = FrameSnapshot {
                    backed,
                    ..FrameSnapshot::default()
                };
                panel.view(&snapshot, MenuPage::view, crate::shared::surface::no_custom);
            }
            frame
        };
        assert_ne!(
            draw(ALL),
            draw(Backed::NONE),
            "greyed rows are the menu's point"
        );
    }

    #[test]
    fn the_menu_fits_one_declaration() {
        for lang in crate::shared::i18n::Lang::ALL {
            let mut ui = headless();
            MenuPage::view(
                &FrameSnapshot {
                    lang: *lang,
                    ..FrameSnapshot::default()
                },
                &mut ui,
            );
            assert!(ui.node_count() <= 60, "{lang:?}: {} nodes", ui.node_count());
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
}
