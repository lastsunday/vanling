//! The Menu page: every page the board can back, one button away, three to a row.

use iot_core::drivers::input::{GestureEvent, InputEvent, SwipeDirection};
use iot_core::ui::panel::{CONTENT_BOTTOM, PANEL_COLUMNS};
use iot_core::ui::{BoxConstraints, PX, Rect, Screen, ScrollId, Style, Ui};

use crate::shared::intent::{BusinessIntent, InputIntent, MenuIntent, NavigationIntent};
use crate::shared::kit::chrome;
use crate::shared::kit::i18n::Msg;
use crate::shared::kit::scroll::{BAR, Drag, STEP_LINES, ScrollConfig, ScrollState, fling_speed};
use crate::shared::kit::state::State;
use crate::shared::kit::widgets::Button;
use crate::shared::model::{Command, DeviceState, Effects};
use crate::shared::route::PageId;
use crate::shared::snapshot::{
    FrameSnapshot, MenuSnapshot, PageSnapshot, ScrollAreaView, ScrollShift,
};

use super::{Descriptor, Repaint};

const AREA: ScrollId = ScrollId::new(1);
const COLUMNS: usize = 3;
const BUTTON_W: i16 = 98;
const BUTTON_H: i16 = 44;
const GAP: i16 = 6;
const COUNT: usize = 9;
const ROWS: usize = COUNT.div_ceil(COLUMNS);

const SCROLL: ScrollConfig = ScrollConfig {
    bounce: true,
    bar: true,
};

/// A scroll frame blits the pixels already in the buffer, so it fits the loop's 20 ms cadence.
const REPAINT_MS: u64 = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuPage {
    scroll: ScrollState<1>,
    drag: Option<Drag>,
}

impl Default for MenuPage {
    fn default() -> Self {
        let mut ui = headless();
        declare(&FrameSnapshot::default(), &mut ui);
        finish(&mut ui);
        let mut scroll = ScrollState::default();
        scroll.measure(&ui, [AREA], SCROLL);
        Self { scroll, drag: None }
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

    /// The button a panel point lands on, in the grid's own order.
    fn hit(&self, x: i32, y: i32) -> Option<usize> {
        let area = self.scroll.areas()[0];
        if !area.rect.contains(x, y) {
            return None;
        }
        let local_x = x - area.rect.x;
        let local_y = y - area.rect.y + i32::from(area.offset);
        if local_x < 0 || local_y < 0 {
            return None;
        }
        let pitch_x = i32::from(BUTTON_W + GAP);
        let pitch_y = i32::from(BUTTON_H + GAP);
        let column = usize::try_from(local_x / pitch_x).ok()?;
        let row = usize::try_from(local_y / pitch_y).ok()?;
        if column >= COLUMNS
            || row >= ROWS
            || local_x % pitch_x >= i32::from(BUTTON_W)
            || local_y % pitch_y >= i32::from(BUTTON_H)
        {
            return None;
        }
        let index = row * COLUMNS + column;
        (index < COUNT).then_some(index)
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
            MenuIntent::Press { area } => {
                // A finger stops a glide, but claims no drag: a tap that never moves has to
                // reach the button underneath. The first drag sample claims instead.
                self.scroll.scroll(area, 0, 0);
            }
            MenuIntent::Drag { id, x, y, at_ms } => {
                let y = i16::try_from(y).unwrap_or(i16::MAX);
                let Some(drag) = self.drag.as_mut().filter(|drag| drag.id == id) else {
                    if let Some(area) = self.scroll.area_at(i32::from(x), i32::from(y)) {
                        self.claim(id, area, y, at_ms);
                    }
                    return (None, Effects::default());
                };
                let delta = drag.step(y, at_ms);
                self.scroll.drag(drag.area, delta);
            }
            MenuIntent::Release {
                id,
                area,
                lines,
                speed,
                at_ms,
            } => {
                if self.drag.as_ref().is_some_and(|drag| drag.id == id) {
                    let drag = self.drag.take().expect("the slot was found");
                    let speed = drag.release(at_ms);
                    self.scroll.fling(drag.area, speed);
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
                let page = entry(self.hit(i32::from(x), i32::from(y))?)?;
                page.is_backed(state.backed)
                    .then(|| BusinessIntent::Navigation(NavigationIntent::Go(page.code())))
            }
            InputEvent::Gesture(GestureEvent::Press { x, y, .. }) => {
                let area = area(x, y)?;
                Some(BusinessIntent::Menu(MenuIntent::Press { area }))
            }
            InputEvent::Gesture(GestureEvent::Drag { id, x, y, at_ms }) => {
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

fn declare(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
    let lang = snapshot.lang;
    let backed = snapshot.backed;
    chrome::page(ui, |ui| {
        chrome::title(ui, Msg::MenuTitle.bytes(lang));
        chrome::scroller(ui, AREA, snapshot.pages.menu.area, |ui| {
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
        PageId::Ambient => Msg::PageAmbient,
        PageId::Attitude => Msg::PageAttitude,
        PageId::Audio => Msg::PageAudio,
        PageId::Speaker => Msg::PageSpeaker,
        PageId::Camera => Msg::PageCamera,
        PageId::Network => Msg::PageNetwork,
        PageId::Splash => Msg::Brand,
        PageId::Env => Msg::PageEnv,
        PageId::Text => Msg::PageText,
        PageId::Widgets => Msg::PageWidgets,
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

    fn centre(page: &MenuPage, index: usize) -> (u16, u16) {
        let area = page.scroll.areas()[0].rect;
        let column = index % COLUMNS;
        let row = index / COLUMNS;
        (
            (area.x + column as i32 * i32::from(BUTTON_W + GAP) + i32::from(BUTTON_W) / 2) as u16,
            (area.y + row as i32 * i32::from(BUTTON_H + GAP) + i32::from(BUTTON_H) / 2) as u16,
        )
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
    fn a_button_centre_maps_to_the_page_drawn_there() {
        let page = MenuPage::default();
        for (index, expected) in entries().enumerate() {
            let (x, y) = centre(&page, index);
            assert_eq!(page.hit(i32::from(x), i32::from(y)), Some(index));
            assert_eq!(entry(index), Some(expected));
        }
    }

    #[test]
    fn the_gaps_between_buttons_are_not_buttons() {
        let page = MenuPage::default();
        let area = page.scroll.areas()[0].rect;
        assert_eq!(page.hit(area.x, area.y), Some(0));
        assert_eq!(page.hit(area.x + i32::from(BUTTON_W), area.y), None);
        assert_eq!(page.hit(area.x, area.y + i32::from(BUTTON_H)), None);
        assert_eq!(page.hit(area.right() - 1, area.bottom() - 1), None);
        assert_eq!(page.hit(area.x - 1, area.y), None);
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
        let index = entries().position(|page| page == PageId::Env).unwrap();
        let (x, y) = centre(&page, index);
        let press = page
            .bind(&op(GestureEvent::Press { id: 0, x, y }), &wired)
            .expect("the press lands");
        apply(&mut page, press);
        assert_eq!(
            page.bind(&tap(x, y), &wired),
            Some(BusinessIntent::Navigation(NavigationIntent::Go(
                PageId::Env.code()
            ))),
            "a tap is press then tap on the wire; the press must not swallow it"
        );
    }

    #[test]
    fn a_page_the_board_cannot_back_is_listed_but_does_not_navigate() {
        let page = MenuPage::default();
        let camera = entries().position(|page| page == PageId::Camera).unwrap();
        let (x, y) = centre(&page, camera);
        assert_eq!(page.bind(&tap(x, y), &DeviceState::default()), None);
        let wired = DeviceState {
            backed: ALL,
            ..DeviceState::default()
        };
        assert_eq!(
            page.bind(&tap(x, y), &wired),
            Some(BusinessIntent::Navigation(NavigationIntent::Go(
                PageId::Camera.code()
            )))
        );
    }

    #[test]
    fn every_name_fits_a_button() {
        use iot_core::ui::font::Face;
        let face = Face::new();
        for page in PageId::ALL {
            for lang in crate::shared::kit::i18n::Lang::ALL {
                let name = page_name(page).text(*lang);
                let width: usize = name.chars().map(|ch| face.advance(ch)).sum();
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
            for lang in crate::shared::kit::i18n::Lang::ALL {
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
                let mut panel = crate::shared::kit::panel::Panel::new(canvas, Screen::default());
                let snapshot = FrameSnapshot {
                    backed,
                    ..FrameSnapshot::default()
                };
                panel.view(
                    &snapshot,
                    MenuPage::view,
                    crate::shared::kit::panel::no_custom,
                );
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
        for lang in crate::shared::kit::i18n::Lang::ALL {
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
