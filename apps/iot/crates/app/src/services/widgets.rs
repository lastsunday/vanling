//! The basic-widgets page: the kit's components grouped under headings, the whole gallery in one
//! scroll box. Controls that can hold a value keep it in this page, not in the device state.

use iot_core::drivers::input::{GestureEvent, InputEvent, SwipeDirection};
use iot_core::ui::panel::{CONTENT_BOTTOM, PANEL_COLUMNS};
use iot_core::ui::{BoxConstraints, Canvas, PX, Rect, Screen, ScrollId, Style, Ui};

use crate::shared::intent::{BusinessIntent, InputIntent, WidgetsIntent};
use crate::shared::kit::chrome;
use crate::shared::kit::i18n::{Lang, Msg};
use crate::shared::kit::scroll::{BAR, Drag, STEP_LINES, ScrollConfig, ScrollState, fling_speed};
use crate::shared::kit::slot::Slot;
use crate::shared::kit::state::State;
use crate::shared::kit::theme::{ButtonKind, Theme};
use crate::shared::kit::widgets::{
    Avatar, Badge, Button, Checkbox, Chip, IconButton, ListTile, ProgressBar, Radio, Select,
    Slider, Spinner, Switch, TextField, Tone, Tooltip,
};
use crate::shared::model::{Command, DeviceState, Effects};
use crate::shared::route::PageId;
use crate::shared::snapshot::{
    FrameSnapshot, PageSnapshot, ScrollAreaView, ScrollShift, WidgetsSnapshot,
};

use super::{Descriptor, Repaint};

const AREA: ScrollId = ScrollId::new(1);

/// The gallery's column: the panel less the page margin and the thumb's gutter.
const CONTENT_W: i16 = PANEL_COLUMNS as i16 - chrome::MARGIN - 2 * BAR;
const GAP: i16 = 6;
const BUTTON: i16 = (CONTENT_W - 2 * GAP) / 3;
const CHIP: i16 = 60;
const TILE_H: i16 = 2 * iot_core::ui::panel::ROW_PITCH as i16;

/// The one custom slot: the list tile, drawn as a cell.
const TILE_SLOT: u8 = 0;

const TAG_FILLED: u16 = 1;
const TAG_OUTLINED: u16 = 2;
const TAG_BARE: u16 = 3;
const TAG_CHIP_A: u16 = 4;
const TAG_CHIP_B: u16 = 5;
const TAG_SWITCH: u16 = 6;
const TAG_CHECK: u16 = 7;
const TAG_RADIO0: u16 = 8;
const TAG_RADIO1: u16 = 9;
const TAG_RADIO2: u16 = 10;
const TAG_SLIDER: u16 = 11;
const TAG_FIELD: u16 = 12;
const TAG_SELECT: u16 = 13;
const TAG_ICON: u16 = 14;
const TAG_TILE: u16 = 15;
const TAGS: usize = 15;

const SEL_FILLED: u8 = 1 << 0;
const SEL_OUTLINED: u8 = 1 << 1;
const SEL_BARE: u8 = 1 << 2;
const SEL_CHIP_A: u8 = 1 << 3;
const SEL_CHIP_B: u8 = 1 << 4;

const SCROLL: ScrollConfig = ScrollConfig {
    bounce: true,
    bar: true,
};

/// A scroll frame blits the pixels already in the buffer, so it fits the loop's 20 ms cadence.
const REPAINT_MS: u64 = 20;
const SPIN_MS: u16 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetsPage {
    scroll: ScrollState<1>,
    drag: Option<Drag>,
    tags: [(u16, Rect); TAGS],
    values: WidgetsSnapshot,
    spin_ms: u16,
}

impl Default for WidgetsPage {
    fn default() -> Self {
        let mut ui = headless();
        declare(&FrameSnapshot::default(), &mut ui);
        finish(&mut ui);
        let mut scroll = ScrollState::default();
        scroll.measure(&ui, [AREA], SCROLL);
        let mut tags = [(0, Rect::default()); TAGS];
        for (tag, rect) in ui.tags() {
            if let Some(slot) = tags.get_mut(usize::from(tag).saturating_sub(1)) {
                *slot = (tag, rect);
            }
        }
        Self {
            scroll,
            drag: None,
            tags,
            values: WidgetsSnapshot {
                slider: 60,
                ..WidgetsSnapshot::default()
            },
            spin_ms: 0,
        }
    }
}

impl WidgetsPage {
    fn claim(&mut self, id: u8, area: u8, y: i16, at_ms: u32) -> bool {
        if self.drag.is_some() {
            return false;
        }
        self.scroll.drag(area, 0);
        self.drag = Some(Drag::new(id, area, y, at_ms));
        true
    }

    fn snapshot(&self) -> WidgetsSnapshot {
        let area = self.scroll.areas()[0];
        let max = area.max();
        let mut view = self.values;
        view.area = ScrollAreaView {
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
        };
        view.moving = self.scroll.moving();
        view
    }

    /// The control a panel point lands on, by tag; zero when none.
    fn tag_at(&self, x: i32, y: i32) -> u16 {
        let area = self.scroll.areas()[0];
        if !area.rect.contains(x, y) {
            return 0;
        }
        let content_y = y + i32::from(area.offset);
        self.tags
            .iter()
            .find(|(tag, rect)| *tag != 0 && rect.contains(x, content_y))
            .map_or(0, |(tag, _)| *tag)
    }

    fn tag_rect(&self, tag: u16) -> Option<Rect> {
        self.tags
            .iter()
            .find(|(candidate, _)| *candidate == tag)
            .map(|(_, rect)| *rect)
    }

    fn toggle(&mut self, tag: u16) {
        match tag {
            TAG_FILLED => self.values.selected ^= SEL_FILLED,
            TAG_OUTLINED => self.values.selected ^= SEL_OUTLINED,
            TAG_BARE => self.values.selected ^= SEL_BARE,
            TAG_CHIP_A => self.values.selected ^= SEL_CHIP_A,
            TAG_CHIP_B => self.values.selected ^= SEL_CHIP_B,
            TAG_SWITCH => self.values.switch = !self.values.switch,
            TAG_CHECK => self.values.check = !self.values.check,
            TAG_RADIO0 => self.values.radio = 0,
            TAG_RADIO1 => self.values.radio = 1,
            TAG_RADIO2 => self.values.radio = 2,
            TAG_FIELD => self.values.focus = !self.values.focus,
            TAG_SELECT => self.values.choice = (self.values.choice + 1) % 3,
            TAG_ICON => self.values.spinning = !self.values.spinning,
            TAG_TILE => self.values.tile = !self.values.tile,
            _ => {}
        }
    }

    fn slide(&mut self, x: u16) {
        let Some(rect) = self.tag_rect(TAG_SLIDER) else {
            return;
        };
        let width = rect.w.max(1);
        let local = (i32::from(x) - rect.x).clamp(0, width);
        self.values.slider = u8::try_from(local * 100 / width).unwrap_or(100);
    }
}

impl super::Service for WidgetsPage {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        _state: &mut DeviceState,
    ) -> (Option<Command>, Effects) {
        let BusinessIntent::Widgets(intent) = intent else {
            return (None, Effects::default());
        };
        match *intent {
            WidgetsIntent::Press { tag } => {
                self.values.pressed = tag;
            }
            WidgetsIntent::Tap { tag } => {
                self.values.pressed = 0;
                self.toggle(tag);
            }
            WidgetsIntent::Slide { tag, x } => {
                if tag == TAG_SLIDER {
                    self.slide(x);
                }
            }
            WidgetsIntent::Grab { id, area, y } => {
                self.values.pressed = 0;
                self.claim(id, area, y, 0);
            }
            WidgetsIntent::Drag { id, x, y, at_ms } => {
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
            WidgetsIntent::Release {
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
            WidgetsIntent::Scroll { area, lines, speed } => {
                self.scroll.scroll(area, lines, speed);
            }
        }
        (None, Effects::default())
    }
}

impl super::LocalPage for WidgetsPage {
    fn bind(&self, op: &InputIntent, _state: &DeviceState) -> Option<BusinessIntent> {
        let tracked = self.drag.as_ref().map(|drag| drag.area);
        let area = |x: u16, y: u16| self.scroll.area_at(i32::from(x), i32::from(y));
        match op.event {
            InputEvent::Gesture(GestureEvent::Tap {
                id, x, y, at_ms, ..
            }) => {
                if let Some(area) = tracked {
                    return Some(BusinessIntent::Widgets(WidgetsIntent::Release {
                        id,
                        area,
                        lines: 0,
                        speed: 0,
                        at_ms: Some(at_ms),
                    }));
                }
                let tag = self.tag_at(i32::from(x), i32::from(y));
                (tag != 0).then_some(BusinessIntent::Widgets(WidgetsIntent::Tap { tag }))
            }
            InputEvent::Gesture(GestureEvent::Press { id, x, y }) => {
                let tag = self.tag_at(i32::from(x), i32::from(y));
                if tag != 0 {
                    return Some(BusinessIntent::Widgets(WidgetsIntent::Press { tag }));
                }
                let area = area(x, y)?;
                Some(BusinessIntent::Widgets(WidgetsIntent::Grab {
                    id,
                    area,
                    y: i16::try_from(y).unwrap_or(i16::MAX),
                }))
            }
            InputEvent::Gesture(GestureEvent::Drag { id, x, y, at_ms }) => {
                if self.drag.as_ref().is_some_and(|drag| drag.id == id) {
                    return Some(BusinessIntent::Widgets(WidgetsIntent::Drag {
                        id,
                        x,
                        y,
                        at_ms,
                    }));
                }
                let tag = self.values.pressed;
                if tag == TAG_SLIDER {
                    return Some(BusinessIntent::Widgets(WidgetsIntent::Slide { tag, x }));
                }
                let area = area(x, y)?;
                Some(BusinessIntent::Widgets(WidgetsIntent::Grab {
                    id,
                    area,
                    y: i16::try_from(y).unwrap_or(i16::MAX),
                }))
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
                Some(BusinessIntent::Widgets(WidgetsIntent::Release {
                    id,
                    area,
                    lines,
                    speed,
                    at_ms: Some(at_ms),
                }))
            }
            InputEvent::Gesture(GestureEvent::LongPress { .. }) => {
                Some(BusinessIntent::Widgets(WidgetsIntent::Tap { tag: 0 }))
            }
            _ => None,
        }
    }

    fn publish(&self, out: &mut PageSnapshot) {
        out.widgets = self.snapshot();
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        declare(snapshot, ui);
    }

    fn descriptor() -> Descriptor {
        Descriptor::new(Self::view)
            .with_custom(custom)
            .with_repaint_ms(REPAINT_MS)
            .with_repaints(Repaint::WhenOrOverlay(widgets_moved))
            .with_scroll(widgets_shift)
    }

    fn tick(&mut self, step_ms: u16) {
        self.scroll.tick(step_ms);
        if self.values.spinning {
            self.spin_ms = self.spin_ms.saturating_add(step_ms);
            while self.spin_ms >= SPIN_MS {
                self.spin_ms -= SPIN_MS;
                self.values.spinner = (self.values.spinner + 1) % 4;
            }
        } else {
            self.spin_ms = 0;
        }
    }

    fn on_hide(&mut self) {
        self.scroll.reset();
        self.drag = None;
        self.values.pressed = 0;
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
    let w = snapshot.pages.widgets;
    let theme = Theme::PANEL;
    chrome::page(ui, |ui| {
        chrome::title(ui, Msg::WidgetsTitle.bytes(lang));
        chrome::scroller(ui, AREA, w.area, |ui| {
            section(ui, Msg::SectionActions.bytes(lang), &theme);
            ui.row(Style::new().gap(GAP).middle(), |ui| {
                ui.add(
                    Button::new(Msg::Filled.bytes(lang))
                        .width(BUTTON)
                        .tag(TAG_FILLED)
                        .state(selected(w.selected & SEL_FILLED != 0)),
                );
                ui.add(
                    Button::new(Msg::Outlined.bytes(lang))
                        .kind(ButtonKind::Outlined)
                        .width(BUTTON)
                        .tag(TAG_OUTLINED)
                        .state(selected(w.selected & SEL_OUTLINED != 0)),
                );
                ui.add(
                    Button::new(Msg::Bare.bytes(lang))
                        .kind(ButtonKind::Text)
                        .width(BUTTON)
                        .tag(TAG_BARE)
                        .state(selected(w.selected & SEL_BARE != 0)),
                );
            });

            section(ui, Msg::SectionSelection.bytes(lang), &theme);
            ui.row(Style::new().gap(GAP).middle(), |ui| {
                ui.add(
                    Chip::new(Msg::Filled.bytes(lang))
                        .width(CHIP)
                        .tag(TAG_CHIP_A)
                        .selected(w.selected & SEL_CHIP_A != 0),
                );
                ui.add(
                    Chip::new(Msg::Outlined.bytes(lang))
                        .width(CHIP)
                        .tag(TAG_CHIP_B)
                        .selected(w.selected & SEL_CHIP_B != 0),
                );
                ui.add(Switch::new(w.switch).tag(TAG_SWITCH));
                ui.add(Checkbox::new(w.check).tag(TAG_CHECK));
            });
            ui.row(Style::new().gap(GAP).middle(), |ui| {
                ui.add(Radio::new(w.radio == 0).tag(TAG_RADIO0));
                ui.add(Radio::new(w.radio == 1).tag(TAG_RADIO1));
                ui.add(Radio::new(w.radio == 2).tag(TAG_RADIO2));
            });

            section(ui, Msg::SectionInput.bytes(lang), &theme);
            ui.add(Slider::new(w.slider).width(CONTENT_W).tag(TAG_SLIDER));
            ui.add(ProgressBar::new(w.slider).width(CONTENT_W));
            ui.add(
                TextField::new(Msg::Value.bytes(lang))
                    .width(CONTENT_W)
                    .focused(w.focus)
                    .tag(TAG_FIELD),
            );
            ui.add(
                Select::new(choice(lang, w.choice))
                    .width(CONTENT_W)
                    .tag(TAG_SELECT),
            );

            section(ui, Msg::SectionDisplay.bytes(lang), &theme);
            ui.row(Style::new().gap(GAP).middle(), |ui| {
                ui.add(Spinner::new(w.spinner));
                ui.add(Avatar::new(b"V"));
                ui.add(
                    IconButton::new(if w.spinning { '-' } else { '+' })
                        .tag(TAG_ICON)
                        .state(selected(w.spinning)),
                );
                let (text, tone) = if w.switch {
                    (Msg::On.bytes(lang), Tone::Accent)
                } else {
                    (Msg::Off.bytes(lang), Tone::Neutral)
                };
                ui.add(Badge::new(text).tone(tone));
            });

            section(ui, Msg::SectionRows.bytes(lang), &theme);
            ui.custom(
                Style::new().width(CONTENT_W).height(TILE_H).tag(TAG_TILE),
                TILE_SLOT,
            );
            ui.add(Tooltip::new(Msg::TapHint.bytes(lang)));
        });
    });
}

fn selected(on: bool) -> State {
    if on { State::SELECTED } else { State::NONE }
}

fn section(ui: &mut Ui<'_, '_>, run: &[u8], theme: &Theme) {
    let ink = ui.color(theme.muted);
    ui.text(run, Style::new().nowrap().color(ink));
}

fn choice(lang: Lang, index: u8) -> &'static [u8] {
    match index % 3 {
        0 => Msg::Choose.bytes(lang),
        1 => Msg::On.bytes(lang),
        _ => Msg::Off.bytes(lang),
    }
}

fn widgets_moved(prev: &FrameSnapshot, next: &FrameSnapshot) -> bool {
    prev.lang != next.lang || prev.pages.widgets != next.pages.widgets
}

fn widgets_shift(prev: &FrameSnapshot, next: &FrameSnapshot) -> Option<ScrollShift> {
    if prev.page != PageId::Widgets || next.page != PageId::Widgets {
        return None;
    }
    let areas = [next.pages.widgets.area];
    let mut before = prev.pages.widgets;
    let mut after = next.pages.widgets;
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

fn custom(
    snapshot: &FrameSnapshot,
    _screen: Screen,
    _slot: u8,
    canvas: &mut Canvas<'_>,
    rect: Rect,
) {
    let w = snapshot.pages.widgets;
    let lang = snapshot.lang;
    let trailing = if w.switch {
        Msg::On.bytes(lang)
    } else {
        Msg::Off.bytes(lang)
    };
    ListTile::new(Msg::Row.bytes(lang))
        .subtitle(Msg::Detail.bytes(lang))
        .leading(Slot::Glyph('#'))
        .trailing(Slot::Badge(trailing))
        .state(State::NONE.then(State::SELECTED, w.tile))
        .draw(canvas, rect, &Theme::PANEL);
}

/// The gallery never draws wider than the panel.
const _: () = assert!(CONTENT_W + 2 * BAR + chrome::MARGIN == PANEL_COLUMNS as i16);
const _: () = assert!(3 * BUTTON + 2 * GAP == CONTENT_W);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{LocalPage, Service};
    use alloc::vec::Vec;

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

    fn apply(page: &mut WidgetsPage, intent: BusinessIntent) {
        let mut state = DeviceState::default();
        page.apply(&intent, &mut state);
    }

    /// Brings a control into the viewport, then taps it where the gallery now draws it.
    fn tap_tag(page: &mut WidgetsPage, tag: u16) {
        let rect = page.tag_rect(tag).expect("the control was laid out");
        let area = page.scroll.areas()[0];
        let middle = rect.y + rect.h / 2;
        let target = middle - (area.rect.y + area.rect.bottom()) / 2;
        let delta = target - i32::from(area.offset);
        if delta != 0 {
            page.scroll.scroll(0, (delta / 12) as i8, 0);
        }
        let offset = page.scroll.areas()[0].offset;
        let x = (rect.x + rect.w / 2) as u16;
        let y = (middle - i32::from(offset)) as u16;
        let intent = page
            .bind(&tap(x, y), &DeviceState::default())
            .expect("the tap lands");
        apply(page, intent);
    }

    #[test]
    fn the_whole_gallery_fits_one_declaration() {
        for lang in Lang::ALL {
            let mut ui = headless();
            WidgetsPage::view(
                &FrameSnapshot {
                    lang: *lang,
                    ..FrameSnapshot::default()
                },
                &mut ui,
            );
            assert!(ui.node_count() <= 60, "{lang:?}: {} nodes", ui.node_count());
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
    fn every_control_is_tagged_once() {
        let page = WidgetsPage::default();
        for tag in 1..=TAG_TILE {
            assert!(page.tag_rect(tag).is_some(), "tag {tag} was not laid out");
        }
    }

    #[test]
    fn the_tags_stay_put_across_languages_and_values() {
        let page = WidgetsPage::default();
        let expected: Vec<_> = page.tags.to_vec();
        let states = [
            WidgetsSnapshot::default(),
            WidgetsSnapshot {
                selected: SEL_FILLED | SEL_CHIP_B,
                switch: true,
                check: true,
                radio: 2,
                slider: 5,
                focus: true,
                choice: 2,
                tile: true,
                spinner: 3,
                ..WidgetsSnapshot::default()
            },
        ];
        for lang in Lang::ALL {
            for values in states {
                let mut ui = headless();
                WidgetsPage::view(
                    &FrameSnapshot {
                        lang: *lang,
                        pages: PageSnapshot {
                            widgets: values,
                            ..PageSnapshot::default()
                        },
                        ..FrameSnapshot::default()
                    },
                    &mut ui,
                );
                finish(&mut ui);
                assert_eq!(ui.tags().collect::<Vec<_>>(), expected, "{lang:?}");
            }
        }
    }

    #[test]
    fn a_tap_toggles_the_control_it_lands_on() {
        let mut page = WidgetsPage::default();
        tap_tag(&mut page, TAG_SWITCH);
        assert!(page.values.switch);
        tap_tag(&mut page, TAG_CHECK);
        assert!(page.values.check);
        tap_tag(&mut page, TAG_CHIP_A);
        assert!(page.values.selected & SEL_CHIP_A != 0);
        tap_tag(&mut page, TAG_FIELD);
        assert!(page.values.focus);
        tap_tag(&mut page, TAG_TILE);
        assert!(page.values.tile);
        tap_tag(&mut page, TAG_ICON);
        assert!(page.values.spinning);
        tap_tag(&mut page, TAG_SELECT);
        assert_eq!(page.values.choice, 1);
    }

    #[test]
    fn a_radio_stays_chosen_until_another_is_tapped() {
        let mut page = WidgetsPage::default();
        tap_tag(&mut page, TAG_RADIO2);
        assert_eq!(page.values.radio, 2);
        tap_tag(&mut page, TAG_RADIO2);
        assert_eq!(page.values.radio, 2);
        tap_tag(&mut page, TAG_RADIO1);
        assert_eq!(page.values.radio, 1);
    }

    #[test]
    fn a_slider_follows_the_finger() {
        let mut page = WidgetsPage::default();
        let rect = page.tag_rect(TAG_SLIDER).unwrap();
        let press = page
            .bind(
                &op(GestureEvent::Press {
                    id: 0,
                    x: rect.x as u16,
                    y: (rect.y + rect.h / 2) as u16,
                }),
                &DeviceState::default(),
            )
            .expect("the slider takes the press");
        apply(&mut page, press);
        let drag = |x: i32| {
            op(GestureEvent::Drag {
                id: 0,
                x: x as u16,
                y: (rect.y + rect.h / 2) as u16,
                at_ms: 0,
            })
        };
        let left = page.bind(&drag(rect.x), &DeviceState::default()).unwrap();
        apply(&mut page, left);
        assert_eq!(page.values.slider, 0);
        let right = page
            .bind(&drag(rect.right() - 1), &DeviceState::default())
            .unwrap();
        apply(&mut page, right);
        assert_eq!(page.values.slider, 99);
    }

    #[test]
    fn a_tap_on_empty_space_is_not_the_lamp() {
        let page = WidgetsPage::default();
        assert_eq!(page.bind(&tap(0, 0), &DeviceState::default()), None);
        let area = page.scroll.areas()[0].rect;
        assert_eq!(
            page.bind(&tap(area.x as u16, area.y as u16), &DeviceState::default()),
            None
        );
    }

    #[test]
    fn a_scroll_frame_moves_only_the_gallery() {
        let mut page = WidgetsPage::default();
        let before = FrameSnapshot {
            page: PageId::Widgets,
            pages: PageSnapshot {
                widgets: page.snapshot(),
                ..PageSnapshot::default()
            },
            ..FrameSnapshot::default()
        };
        apply(
            &mut page,
            BusinessIntent::Widgets(WidgetsIntent::Scroll {
                area: 0,
                lines: 3,
                speed: 0,
            }),
        );
        let after = FrameSnapshot {
            page: PageId::Widgets,
            pages: PageSnapshot {
                widgets: page.snapshot(),
                ..PageSnapshot::default()
            },
            ..FrameSnapshot::default()
        };
        assert!(after.pages.widgets.area.offset > 0);
        let shift = widgets_shift(&before, &after).expect("a pure scroll");
        assert_eq!(shift.shifts()[0], after.pages.widgets.area.offset);
    }

    #[test]
    fn the_spinner_turns_only_while_its_button_is_on() {
        let mut page = WidgetsPage::default();
        let phase = page.values.spinner;
        page.tick(SPIN_MS);
        assert_eq!(
            page.values.spinner, phase,
            "a stopped spinner holds its frame"
        );
        tap_tag(&mut page, TAG_ICON);
        assert!(page.values.spinning);
        page.tick(SPIN_MS);
        assert_eq!(page.values.spinner, (phase + 1) % 4);
        tap_tag(&mut page, TAG_ICON);
        assert!(!page.values.spinning);
        let stopped = page.values.spinner;
        page.tick(SPIN_MS);
        assert_eq!(page.values.spinner, stopped);
    }

    #[test]
    fn a_hidden_page_drops_the_finger_and_the_glide() {
        let mut page = WidgetsPage::default();
        apply(
            &mut page,
            BusinessIntent::Widgets(WidgetsIntent::Scroll {
                area: 0,
                lines: 3,
                speed: 1_600,
            }),
        );
        page.on_hide();
        assert_eq!(page.snapshot().area.offset, 0);
        assert!(page.drag.is_none());
        assert_eq!(page.values.pressed, 0);
    }
}
