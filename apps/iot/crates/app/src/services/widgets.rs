//! The basic-widgets page: every UI component grouped under headings, the whole gallery in one
//! scroll box. Controls that can hold a value keep it in this page, not in the device state.

use iot_core::drivers::input::{GestureEvent, InputEvent, SwipeDirection};
use iot_core::ui::panel::PANEL_COLUMNS;
use iot_core::ui::{Canvas, Layout, NodeId, Rect, Screen, ScrollId, Style, Ui};

use crate::shared::i18n::{Lang, Msg};
use crate::shared::intent::{BusinessIntent, InputIntent, Key, WidgetsAction, WidgetsIntent};
use crate::shared::model::{Command, DeviceState, Effects};
use crate::shared::page;
use crate::shared::route::PageId;
use crate::shared::snapshot::{
    FrameSnapshot, PageSnapshot, ScrollAreaView, ScrollShift, WidgetsSnapshot,
};
use crate::shared::surface::{finish, headless};
use crate::shared::ui::keyboard;
use crate::shared::ui::keyboard::FIELD_BYTES;
use crate::shared::ui::scroll::{
    BAR, DRAG_SLOP, Drag, Press, STEP_LINES, ScrollConfig, ScrollState, fling_speed, lift,
};
use crate::shared::ui::slot::Slot;
use crate::shared::ui::state::State;
use crate::shared::ui::tag::Tag;
use crate::shared::ui::theme::{ButtonKind, Theme};
use crate::shared::ui::widgets::{
    Avatar, Badge, Button, Checkbox, Chip, Divider, IconButton, Indicator, ListTile, ProgressBar,
    Radio, Select, Slider, Spinner, Switch, TextField, Tone, Tooltip, Variant,
};

use super::{Descriptor, Repaint};

const AREA: ScrollId = ScrollId::new(1);

/// The gallery's column: the panel less the page margin and the thumb's gutter.
const CONTENT_W: i16 = PANEL_COLUMNS as i16 - page::MARGIN - 2 * BAR;
const GAP: i16 = 6;
const BUTTON: i16 = (CONTENT_W - 2 * GAP) / 3;
/// Wide enough for `OUTLINED` at 14 px and fixed, so the row holds its shape in either language.
const CHIP: i16 = 76;
const TILE_H: i16 = 2 * iot_core::ui::panel::ROW_PITCH as i16;

/// The two custom cells: the list tile, and the keyboard the field raises.
const TILE_SLOT: u8 = 0;
const KEY_SLOT: u8 = 1;

const SEL_FILLED: u8 = 1 << 0;
const SEL_OUTLINED: u8 = 1 << 1;
const SEL_BARE: u8 = 1 << 2;
const SEL_CHIP_A: u8 = 1 << 3;
const SEL_CHIP_B: u8 = 1 << 4;

const SCROLL: ScrollConfig = ScrollConfig {
    bounce: true,
    bar: true,
    pull: 0,
};

/// A scroll frame blits the pixels already in the buffer, so it fits the loop's 20 ms cadence.
const REPAINT_MS: u64 = 20;
const SPIN_MS: u16 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetsPage {
    scroll: ScrollState<1>,
    drag: Option<Drag>,
    press: Option<Press>,
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
        Self {
            scroll,
            drag: None,
            press: None,
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
    fn tag_at(&self, layout: &Layout, x: i32, y: i32) -> u16 {
        let area = self.scroll.areas()[0];
        if !area.rect.contains(x, y) {
            return 0;
        }
        layout.at(x, y).unwrap_or(0)
    }

    fn toggle(&mut self, action: WidgetsAction) {
        match action {
            WidgetsAction::Filled => self.values.selected ^= SEL_FILLED,
            WidgetsAction::Outlined => self.values.selected ^= SEL_OUTLINED,
            WidgetsAction::Bare => self.values.selected ^= SEL_BARE,
            WidgetsAction::ChipA => self.values.selected ^= SEL_CHIP_A,
            WidgetsAction::ChipB => self.values.selected ^= SEL_CHIP_B,
            WidgetsAction::Switch => self.values.switch = !self.values.switch,
            WidgetsAction::Check => self.values.check = !self.values.check,
            WidgetsAction::Radio0 => self.values.radio = 0,
            WidgetsAction::Radio1 => self.values.radio = 1,
            WidgetsAction::Radio2 => self.values.radio = 2,
            WidgetsAction::Field => self.values.focus = !self.values.focus,
            WidgetsAction::Select => self.values.choice = (self.values.choice + 1) % 3,
            WidgetsAction::Icon => self.values.spinning = !self.values.spinning,
            WidgetsAction::Tile => self.values.tile = !self.values.tile,
            // A slider's value comes from `Slide`, not from a tap on its box.
            WidgetsAction::Slider => {}
        }
    }

    fn slide(&mut self, layout: &Layout, x: u16) {
        let Some(rect) = layout.rect(WidgetsAction::Slider.tag()) else {
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
        layout: &iot_core::ui::Layout,
    ) -> (Option<Command>, Effects) {
        let BusinessIntent::Widgets(intent) = intent else {
            return (None, Effects::default());
        };
        match *intent {
            WidgetsIntent::Press { action, id, x, y } => {
                self.values.pressed = action.tag();
                let x = i16::try_from(x).unwrap_or(i16::MAX);
                let y = i16::try_from(y).unwrap_or(i16::MAX);
                self.press = Some(Press {
                    id,
                    tag: action.tag(),
                    x,
                    y,
                });
            }
            WidgetsIntent::Tap { action } => {
                self.values.pressed = 0;
                self.toggle(action);
            }
            WidgetsIntent::Slide { action, x } => {
                if action == WidgetsAction::Slider {
                    self.press = None;
                    self.slide(layout, x);
                }
            }
            WidgetsIntent::Key { key, at_ms } => {
                if key == Key::Cancel {
                    self.values.focus = false;
                } else {
                    self.values.text.edit(key, at_ms);
                }
            }
            WidgetsIntent::Grab { id, area, y } => {
                self.values.pressed = 0;
                self.press = None;
                self.claim(id, area, y, 0);
            }
            WidgetsIntent::Drag { id, x, y, at_ms } => {
                let y = i16::try_from(y).unwrap_or(i16::MAX);
                if let Some(drag) = self.drag.as_mut().filter(|drag| drag.id == id) {
                    let delta = drag.step(y, at_ms);
                    self.scroll.drag(drag.area, delta);
                } else if let Some(press) = self.press.take().filter(|press| press.id == id) {
                    self.values.pressed = 0;
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
            WidgetsIntent::Release {
                id,
                area,
                lines,
                speed,
                at_ms,
            } => {
                // The finger is off the glass: whatever it last pressed is no longer held,
                // and a slider left latched would turn the next drag anywhere into a slide.
                self.values.pressed = 0;
                self.press = None;
                if let Some((area, speed)) = lift(&mut self.drag, id, at_ms) {
                    self.scroll.fling(area, speed);
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
    fn bind(
        &self,
        op: &InputIntent,
        _state: &DeviceState,
        layout: &Layout,
    ) -> Option<BusinessIntent> {
        // Focused, the keyboard owns the panel: the gallery behind it is not drawn, so a swipe
        // must not scroll it and a drag must not start.
        if self.values.focus {
            let InputEvent::Gesture(GestureEvent::Tap { x, y, at_ms, .. }) = op.event else {
                return None;
            };
            let key = keyboard::key_at(
                layout.slot(KEY_SLOT)?,
                i32::from(x),
                i32::from(y),
                self.values.text.layer(),
            )?;
            return Some(BusinessIntent::Widgets(WidgetsIntent::Key { key, at_ms }));
        }
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
                let action =
                    WidgetsAction::from_tag(self.tag_at(layout, i32::from(x), i32::from(y)))?;
                Some(BusinessIntent::Widgets(WidgetsIntent::Tap { action }))
            }
            InputEvent::Gesture(GestureEvent::Press { id, x, y }) => {
                let action =
                    WidgetsAction::from_tag(self.tag_at(layout, i32::from(x), i32::from(y)));
                if let Some(action) = action {
                    return Some(BusinessIntent::Widgets(WidgetsIntent::Press {
                        action,
                        id,
                        x,
                        y,
                    }));
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
                if self.values.pressed == WidgetsAction::Slider.tag() {
                    return Some(BusinessIntent::Widgets(WidgetsIntent::Slide {
                        action: WidgetsAction::Slider,
                        x,
                    }));
                }
                if let Some(press) = self.press.filter(|press| press.id == id) {
                    let dx = i32::from(x) - i32::from(press.x);
                    let dy = i32::from(y) - i32::from(press.y);
                    if dx.abs() < i32::from(DRAG_SLOP) && dy.abs() < i32::from(DRAG_SLOP) {
                        return None;
                    }
                }
                area(x, y)?;
                Some(BusinessIntent::Widgets(WidgetsIntent::Drag {
                    id,
                    x,
                    y,
                    at_ms,
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
            InputEvent::Gesture(GestureEvent::LongPress { id, x, y, .. }) => {
                // A hold that never became a swipe still ends the press it started.
                let area = tracked.or_else(|| area(x, y))?;
                Some(BusinessIntent::Widgets(WidgetsIntent::Release {
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
        out.widgets = self.snapshot();
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        let _ = declare(snapshot, ui);
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
        self.press = None;
        self.values.pressed = 0;
        self.values.focus = false;
    }
}

fn declare(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) -> Option<NodeId> {
    let lang = snapshot.lang;
    let w = snapshot.pages.widgets;
    let theme = *Theme::of(snapshot.theme);
    let mut keyboard = None;
    page::body(ui, |ui| {
        page::title(ui, Msg::WidgetsTitle.bytes(lang));
        if w.focus {
            ui.add(
                TextField::new(w.text.as_bytes())
                    .width(CONTENT_W)
                    .focused(true),
            );
            keyboard = Some(ui.custom(Style::new().width(CONTENT_W).height(keyboard::H), KEY_SLOT));
            return;
        }
        page::scroller(ui, AREA, w.area, |ui| {
            section(ui, Msg::SectionActions.bytes(lang), &theme);
            ui.row(Style::new().gap(GAP).middle(), |ui| {
                ui.add(
                    Button::new(Msg::Filled.bytes(lang))
                        .width(BUTTON)
                        .tag(WidgetsAction::Filled.tag())
                        .state(selected(w.selected & SEL_FILLED != 0)),
                );
                ui.add(
                    Button::new(Msg::Outlined.bytes(lang))
                        .kind(ButtonKind::Outlined)
                        .width(BUTTON)
                        .tag(WidgetsAction::Outlined.tag())
                        .state(selected(w.selected & SEL_OUTLINED != 0)),
                );
                ui.add(
                    Button::new(Msg::Bare.bytes(lang))
                        .kind(ButtonKind::Text)
                        .width(BUTTON)
                        .tag(WidgetsAction::Bare.tag())
                        .state(selected(w.selected & SEL_BARE != 0)),
                );
            });

            section(ui, Msg::SectionSelection.bytes(lang), &theme);
            ui.row(Style::new().gap(GAP).middle(), |ui| {
                ui.add(
                    Chip::new(Msg::Filled.bytes(lang))
                        .width(CHIP)
                        .tag(WidgetsAction::ChipA.tag())
                        .selected(w.selected & SEL_CHIP_A != 0),
                );
                ui.add(
                    Chip::new(Msg::Outlined.bytes(lang))
                        .width(CHIP)
                        .tag(WidgetsAction::ChipB.tag())
                        .selected(w.selected & SEL_CHIP_B != 0),
                );
                ui.add(Switch::new(w.switch).tag(WidgetsAction::Switch.tag()));
                ui.add(Checkbox::new(w.check).tag(WidgetsAction::Check.tag()));
            });
            ui.row(Style::new().gap(GAP).middle(), |ui| {
                ui.add(Radio::new(w.radio == 0).tag(WidgetsAction::Radio0.tag()));
                ui.add(Radio::new(w.radio == 1).tag(WidgetsAction::Radio1.tag()));
                ui.add(Radio::new(w.radio == 2).tag(WidgetsAction::Radio2.tag()));
            });

            section(ui, Msg::SectionInput.bytes(lang), &theme);
            ui.add(
                Slider::new(w.slider)
                    .width(CONTENT_W)
                    .tag(WidgetsAction::Slider.tag()),
            );
            ui.add(ProgressBar::new(w.slider).width(CONTENT_W));
            ui.add(
                TextField::new(field_text(&w.text, lang))
                    .width(CONTENT_W)
                    .focused(w.focus)
                    .tag(WidgetsAction::Field.tag()),
            );
            ui.add(
                Select::new(choice(lang, w.choice))
                    .width(CONTENT_W)
                    .tag(WidgetsAction::Select.tag()),
            );

            section(ui, Msg::SectionDisplay.bytes(lang), &theme);
            ui.row(Style::new().gap(GAP).middle(), |ui| {
                ui.add(Spinner::new(w.spinner));
                ui.add(Indicator::new(Avatar::new(b"V")).label(b"3"));
                ui.add(
                    IconButton::new(if w.spinning { '-' } else { '+' })
                        .tag(WidgetsAction::Icon.tag())
                        .state(selected(w.spinning)),
                );
                let word = if w.switch {
                    Msg::On.bytes(lang)
                } else {
                    Msg::Off.bytes(lang)
                };
                let tone = if w.switch {
                    Tone::Accent
                } else {
                    Tone::Neutral
                };
                for variant in [Variant::Filled, Variant::Light, Variant::Outline] {
                    ui.add(Badge::new(word).tone(tone).variant(variant));
                }
            });
            caption(ui, Msg::DisplayNames.bytes(lang), &theme);

            section(ui, Msg::SectionRows.bytes(lang), &theme);
            ui.add(Tooltip::new(Msg::Tooltip.bytes(lang)));
            ui.custom(
                Style::new()
                    .width(CONTENT_W)
                    .height(TILE_H)
                    .tag(WidgetsAction::Tile.tag()),
                TILE_SLOT,
            );
        });
    });
    keyboard
}

fn selected(on: bool) -> State {
    if on { State::SELECTED } else { State::NONE }
}

fn section(ui: &mut Ui<'_, '_>, run: &[u8], theme: &Theme) {
    ui.add(Divider::new().width(CONTENT_W));
    caption(ui, run, theme);
}

fn caption(ui: &mut Ui<'_, '_>, run: &[u8], theme: &Theme) {
    let fg = ui.color(theme.muted);
    ui.text(run, Style::new().nowrap().color(fg));
}

/// The field prints the draft, and falls back to its own name only while empty.
fn field_text(text: &keyboard::Text<FIELD_BYTES>, lang: Lang) -> &[u8] {
    if text.is_empty() {
        Msg::Value.bytes(lang)
    } else {
        text.as_bytes()
    }
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

fn custom(snapshot: &FrameSnapshot, screen: Screen, slot: u8, canvas: &mut Canvas<'_>, rect: Rect) {
    let w = snapshot.pages.widgets;
    let lang = snapshot.lang;
    match slot {
        TILE_SLOT => {
            let trailing = if w.tile {
                Msg::On.bytes(lang)
            } else {
                Msg::Off.bytes(lang)
            };
            ListTile::new(Msg::Row.bytes(lang))
                .subtitle(Msg::Detail.bytes(lang))
                .leading(Slot::Glyph('#'))
                .trailing(Slot::Badge(trailing))
                .state(State::NONE.then(State::SELECTED, w.tile))
                .draw(canvas, rect, Theme::of(screen.theme));
        }
        KEY_SLOT => keyboard::draw(canvas, rect, w.text, Theme::of(screen.theme)),
        _ => {}
    }
}

/// The gallery never draws wider than the panel.
const _: () = assert!(CONTENT_W + 2 * BAR + page::MARGIN == PANEL_COLUMNS as i16);
const _: () = assert!(3 * BUTTON + 2 * GAP == CONTENT_W);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{LocalPage, Service};
    use alloc::vec::Vec;
    use iot_core::drivers::input::{RawPoint, TouchContinuity, TouchGestures};
    use iot_core::ui::panel::CONTENT_BOTTOM;
    use iot_core::ui::{Layout, PX};

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

    fn layout(page: &WidgetsPage) -> Layout {
        crate::services::layout_of(page, PageId::Widgets)
    }

    fn hit(page: &WidgetsPage, intent: &InputIntent) -> Option<BusinessIntent> {
        page.bind(intent, &DeviceState::default(), &layout(page))
    }

    fn apply(page: &mut WidgetsPage, intent: BusinessIntent) {
        let mut state = DeviceState::default();
        let layout = layout(page);
        page.apply(&intent, &mut state, &layout);
    }

    fn key_intent(page: &WidgetsPage, cell: usize) -> BusinessIntent {
        let keys = layout(page).slot(KEY_SLOT).expect("a keyboard");
        let (x, y) = keyboard::key_centre(keys, cell);
        hit(&page, &tap(x, y)).expect("the key is inside the keyboard")
    }

    /// Brings a control into the viewport, then taps it where the gallery now draws it.
    fn tap_tag(page: &mut WidgetsPage, tag: u16) {
        let area = page.scroll.areas()[0];
        let rect = layout(&page).rect(tag).expect("the control was laid out");
        let middle = rect.y + rect.h / 2 + i32::from(area.offset);
        let target = middle - (area.rect.y + area.rect.bottom()) / 2;
        let delta = target - i32::from(area.offset);
        if delta != 0 {
            page.scroll.scroll(0, (delta / 12) as i8, 0);
        }
        let rect = layout(&page).rect(tag).expect("the control was laid out");
        let x = (rect.x + rect.w / 2) as u16;
        let y = (rect.y + rect.h / 2) as u16;
        let intent = hit(&page, &tap(x, y)).expect("the tap lands");
        apply(page, intent);
    }

    /// One raw panel sample through the driver's continuity tracker and the gesture
    /// classifier, into the page: the chain a finger actually runs.
    fn drive_raw(
        continuity: &mut TouchContinuity,
        gestures: &mut TouchGestures,
        page: &mut WidgetsPage,
        raw: &[RawPoint],
        now_ms: u64,
    ) {
        use iot_core::drivers::input::EventAggregator;
        let Some(sample) = continuity.update(raw) else {
            return;
        };
        let mut next = gestures.feed(InputEvent::Touch(sample), now_ms);
        while let Some(event) = next {
            if let Some(intent) = hit(&page, &InputIntent { source: 0, event }) {
                apply(page, intent);
            }
            next = gestures.tick(now_ms);
        }
    }

    /// A finger that lands again right after a slide must be a new contact. Merged into the
    /// slide's tracker it arrives as a contact, no press is reported for it, and the tap it
    /// becomes carries the slider's own coordinates — the control the finger chose never hears.
    #[test]
    fn a_tap_right_after_a_slide_reaches_the_control_it_lands_on() {
        let mut page = WidgetsPage::default();
        let mut continuity = TouchContinuity::new();
        let mut gestures = TouchGestures::new();
        let slider = layout(&page).rect(WidgetsAction::Slider.tag()).unwrap();
        let chip = layout(&page).rect(WidgetsAction::ChipB.tag()).unwrap();
        let point = |x: u16, y: u16| RawPoint::new(x, y, RawPoint::NO_ID);
        let y = (slider.y + slider.h / 2) as u16;
        let start = (slider.x + 20) as u16;

        drive_raw(
            &mut continuity,
            &mut gestures,
            &mut page,
            &[point(start, y)],
            0,
        );
        for step in 1..=5u16 {
            let x = start + step * 20;
            drive_raw(
                &mut continuity,
                &mut gestures,
                &mut page,
                &[point(x, y)],
                u64::from(step) * 10,
            );
        }
        assert!(page.values.slider > 0, "the finger slid the slider");

        // Lift, then land on the chip inside the window a release is still confirmed in.
        drive_raw(&mut continuity, &mut gestures, &mut page, &[], 60);
        let (x, y) = ((chip.x + chip.w / 2) as u16, (chip.y + chip.h / 2) as u16);
        drive_raw(
            &mut continuity,
            &mut gestures,
            &mut page,
            &[point(x, y)],
            70,
        );
        for step in 1..=3u64 {
            drive_raw(
                &mut continuity,
                &mut gestures,
                &mut page,
                &[],
                70 + step * 10,
            );
        }
        assert!(
            page.values.selected & SEL_CHIP_B != 0,
            "the tap after a slide reached the chip"
        );
        assert_eq!(page.values.pressed, 0, "and the slide let go");
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
    fn every_action_is_drawn_once() {
        let page = WidgetsPage::default();
        let drawn: Vec<u16> = layout(&page).tags().map(|(tag, _)| tag).collect();
        let expected: Vec<u16> = WidgetsAction::ALL
            .iter()
            .map(|action| action.tag())
            .collect();
        assert_eq!(drawn, expected);
    }

    #[test]
    fn a_focused_field_takes_keys_and_ok_leaves() {
        let mut page = WidgetsPage::default();
        tap_tag(&mut page, WidgetsAction::Field.tag());
        assert!(page.values.focus);
        let intent = key_intent(&page, 1);
        assert_eq!(
            intent,
            BusinessIntent::Widgets(WidgetsIntent::Key {
                key: Key::Char(b'2'),
                at_ms: 0
            })
        );
        apply(&mut page, intent);
        assert_eq!(page.values.text.as_bytes(), b"a");
        let ok = key_intent(&page, 14);
        apply(&mut page, ok);
        assert!(!page.values.focus, "OK puts the gallery back");
    }

    #[test]
    fn the_gallery_field_prints_what_was_typed() {
        let mut page = WidgetsPage::default();
        assert_eq!(field_text(&page.values.text, Lang::En), b"VALUE");
        page.values
            .text
            .edit(Key::Char(crate::shared::intent::Layer::Abc.face(1)), 0);
        assert_eq!(field_text(&page.values.text, Lang::En), b"a");
    }

    #[test]
    fn the_keyboard_is_drawn_only_while_the_field_is_focused() {
        let mut ui = headless();
        WidgetsPage::view(&FrameSnapshot::default(), &mut ui);
        assert!(ui.tags().next().is_some(), "the gallery is up");
        let mut focused = FrameSnapshot::default();
        focused.pages.widgets.focus = true;
        let mut ui = headless();
        let id = declare(&focused, &mut ui).expect("a focused field raises the keyboard");
        finish(&mut ui);
        assert_eq!(ui.rect(id).h, i32::from(keyboard::H));
        assert_eq!(ui.tags().count(), 0, "the gallery is gone while typing");
    }

    #[test]
    fn the_tags_stay_put_across_languages_and_values() {
        let page = WidgetsPage::default();
        let expected: Vec<_> = layout(&page).tags().collect();
        let states = [
            WidgetsSnapshot::default(),
            WidgetsSnapshot {
                selected: SEL_FILLED | SEL_CHIP_B,
                switch: true,
                check: true,
                radio: 2,
                slider: 5,
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
        tap_tag(&mut page, WidgetsAction::Switch.tag());
        assert!(page.values.switch);
        tap_tag(&mut page, WidgetsAction::Check.tag());
        assert!(page.values.check);
        tap_tag(&mut page, WidgetsAction::ChipA.tag());
        assert!(page.values.selected & SEL_CHIP_A != 0);
        tap_tag(&mut page, WidgetsAction::Tile.tag());
        assert!(page.values.tile);
        tap_tag(&mut page, WidgetsAction::Icon.tag());
        assert!(page.values.spinning);
        tap_tag(&mut page, WidgetsAction::Select.tag());
        assert_eq!(page.values.choice, 1);
        tap_tag(&mut page, WidgetsAction::Field.tag());
        assert!(page.values.focus);
    }

    #[test]
    fn a_radio_stays_chosen_until_another_is_tapped() {
        let mut page = WidgetsPage::default();
        tap_tag(&mut page, WidgetsAction::Radio2.tag());
        assert_eq!(page.values.radio, 2);
        tap_tag(&mut page, WidgetsAction::Radio2.tag());
        assert_eq!(page.values.radio, 2);
        tap_tag(&mut page, WidgetsAction::Radio1.tag());
        assert_eq!(page.values.radio, 1);
    }

    #[test]
    fn a_slider_follows_the_finger() {
        let mut page = WidgetsPage::default();
        let rect = layout(&page).rect(WidgetsAction::Slider.tag()).unwrap();
        let press = hit(
            &page,
            &op(GestureEvent::Press {
                id: 0,
                x: rect.x as u16,
                y: (rect.y + rect.h / 2) as u16,
            }),
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
        let left = hit(&page, &drag(rect.x)).unwrap();
        apply(&mut page, left);
        assert_eq!(page.values.slider, 0);
        let right = hit(&page, &drag(rect.right() - 1)).unwrap();
        apply(&mut page, right);
        assert_eq!(page.values.slider, 99);
    }

    #[test]
    fn a_slider_lets_go_of_the_finger_when_it_lifts() {
        let mut page = WidgetsPage::default();
        let rect = layout(&page).rect(WidgetsAction::Slider.tag()).unwrap();
        let y = (rect.y + rect.h / 2) as u16;
        let press = hit(
            &page,
            &op(GestureEvent::Press {
                id: 0,
                x: (rect.x + 20) as u16,
                y,
            }),
        )
        .unwrap();
        apply(&mut page, press);
        let drag = hit(
            &page,
            &op(GestureEvent::Drag {
                id: 0,
                x: (rect.x + 120) as u16,
                y,
                at_ms: 40,
            }),
        )
        .unwrap();
        apply(&mut page, drag);
        assert_eq!(page.values.pressed, WidgetsAction::Slider.tag());

        let lift = hit(
            &page,
            &op(GestureEvent::Swipe {
                id: 0,
                direction: SwipeDirection::Right,
                x: (rect.x + 20) as u16,
                y,
                end_x: (rect.x + 150) as u16,
                end_y: y,
                held_ms: 100,
                distance_px: 130,
                at_ms: 100,
            }),
        )
        .unwrap();
        apply(&mut page, lift);
        assert_eq!(
            page.values.pressed, 0,
            "a slider the finger has left must not stay held"
        );

        // The next drag across the gallery is a scroll, not another slide.
        let slid = page.values.slider;
        let area = page.scroll.areas()[0].rect;
        let grab = hit(
            &page,
            &op(GestureEvent::Press {
                id: 1,
                x: area.x as u16 + 40,
                y: (area.y + 4) as u16,
            }),
        )
        .unwrap();
        apply(&mut page, grab);
        let scroll = hit(
            &page,
            &op(GestureEvent::Drag {
                id: 1,
                x: area.x as u16 + 40,
                y: (area.y - 16) as u16,
                at_ms: 300,
            }),
        )
        .unwrap();
        apply(&mut page, scroll);
        assert_eq!(page.values.slider, slid, "the slider stayed where it was");
        assert!(
            page.scroll.areas()[0].offset > 0,
            "the gallery took the drag instead"
        );
    }

    #[test]
    fn a_hold_that_never_became_a_swipe_also_lets_go() {
        let mut page = WidgetsPage::default();
        let rect = layout(&page).rect(WidgetsAction::Switch.tag()).unwrap();
        let (x, y) = ((rect.x + rect.w / 2) as u16, (rect.y + rect.h / 2) as u16);
        let press = hit(&page, &op(GestureEvent::Press { id: 0, x, y })).unwrap();
        apply(&mut page, press);
        assert_eq!(page.values.pressed, WidgetsAction::Switch.tag());
        let hold = hit(
            &page,
            &op(GestureEvent::LongPress {
                id: 0,
                x,
                y,
                held_ms: 600,
            }),
        )
        .expect("a hold ends the press it started");
        apply(&mut page, hold);
        assert_eq!(page.values.pressed, 0);
    }

    #[test]
    fn a_tap_on_empty_space_binds_nothing() {
        let page = WidgetsPage::default();
        assert_eq!(hit(&page, &tap(0, 0)), None);
        let area = page.scroll.areas()[0].rect;
        assert_eq!(hit(&page, &tap(area.x as u16, area.y as u16)), None);
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
        tap_tag(&mut page, WidgetsAction::Icon.tag());
        assert!(page.values.spinning);
        page.tick(SPIN_MS);
        assert_eq!(page.values.spinner, (phase + 1) % 4);
        tap_tag(&mut page, WidgetsAction::Icon.tag());
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
