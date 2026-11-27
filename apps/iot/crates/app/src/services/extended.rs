//! The extended-widget page: the search field, the filter chips, the form and number fields,
//! and the keypad that shows what a pad cell reports.

use iot_core::drivers::input::{GestureEvent, InputEvent};
use iot_core::ui::panel::PANEL_COLUMNS;
use iot_core::ui::{Canvas, Layout, Rect, Screen, ScrollId, Style, Ui};

use super::gallery::{REPAINT_MS, Scroller};
use super::{Descriptor, Repaint};
use crate::shared::i18n::Msg;
use crate::shared::intent::{BusinessIntent, GalleryIntent, InputIntent, Key};
use crate::shared::model::{Command, DeviceState, Effects};
use crate::shared::page;
use crate::shared::route::PageId;
use crate::shared::snapshot::{ExtendedSnapshot, FrameSnapshot, PageSnapshot, ScrollShift};
use crate::shared::surface::{finish, headless};
use crate::shared::ui::keyboard;
use crate::shared::ui::scroll::{BAR, ScrollConfig};
use crate::shared::ui::state::State;
use crate::shared::ui::tag::Tag;
use crate::shared::ui::theme::Theme;
use crate::shared::ui::widgets::keypad;
use crate::shared::ui::widgets::{FilterChips, FormField, Keypad, NumberField, SearchField};

const AREA: ScrollId = ScrollId::new(1);
const CONTENT_W: i16 = PANEL_COLUMNS as i16 - page::MARGIN - 2 * BAR;
const KEY_SLOT: u8 = 0;
/// Four pad rows, each a full touch target.
const KEYPAD_H: i16 = page::TOUCH * 4;
/// The pad's own tag: the arena remembers a press on it instead of claiming the scroll.
const TAG_KEYPAD: u16 = 90;

const SCROLL: ScrollConfig = ScrollConfig {
    bounce: true,
    bar: true,
    pull: 0,
};

/// One control on the page. The discriminant is the tag the declaration draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
enum Action {
    Search = 1,
    Chip0,
    Chip1,
    Chip2,
    Form,
    Dec,
    Inc,
}

impl Tag for Action {
    const ALL: &'static [Self] = &[
        Self::Search,
        Self::Chip0,
        Self::Chip1,
        Self::Chip2,
        Self::Form,
        Self::Dec,
        Self::Inc,
    ];

    fn tag(self) -> u16 {
        self as u16
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtendedPage {
    scroll: Scroller,
    values: ExtendedSnapshot,
}

impl Default for ExtendedPage {
    fn default() -> Self {
        let mut ui = headless();
        declare(&FrameSnapshot::default(), &mut ui);
        finish(&mut ui);
        let mut scroll = Scroller::default();
        scroll.measure(&ui, AREA, SCROLL);
        Self {
            scroll,
            values: ExtendedSnapshot {
                chips: 1,
                number: 3,
                ..ExtendedSnapshot::default()
            },
        }
    }
}

impl ExtendedPage {
    fn tag_at(&self, layout: &Layout, x: i32, y: i32) -> u16 {
        self.scroll
            .tag_at(|x, y| layout.at(x, y).unwrap_or(0), x, y)
    }

    fn snapshot(&self) -> ExtendedSnapshot {
        let mut view = self.values;
        view.area = self.scroll.view();
        view.moving = self.scroll.moving();
        view
    }

    fn tap(&mut self, action: Action) {
        match action {
            Action::Search => self.values.focus = !self.values.focus,
            Action::Chip0 => self.values.chips ^= 1,
            Action::Chip1 => self.values.chips ^= 2,
            Action::Chip2 => self.values.chips ^= 4,
            Action::Form => self.values.form_error = !self.values.form_error,
            Action::Dec => self.values.number = self.values.number.saturating_sub(1),
            Action::Inc => self.values.number = self.values.number.saturating_add(1),
        }
    }
}

impl super::Service for ExtendedPage {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        _state: &mut DeviceState,
        _layout: &iot_core::ui::Layout,
    ) -> (Option<Command>, Effects) {
        let BusinessIntent::Gallery { wire, intent } = intent else {
            return (None, Effects::default());
        };
        if *wire != PageId::Extended.code() {
            return (None, Effects::default());
        }
        match *intent {
            GalleryIntent::Tap { tag } => {
                if let Some(action) = Action::from_tag(tag) {
                    self.tap(action);
                }
            }
            GalleryIntent::Key { key, at_ms } => {
                if key == Key::Cancel {
                    self.values.focus = false;
                } else {
                    self.values.text.edit(key, at_ms);
                }
            }
            GalleryIntent::Pad { cell } => self.values.key = Some(cell),
            _ => {}
        }
        self.scroll.apply(*intent);
        (None, Effects::default())
    }
}

impl super::LocalPage for ExtendedPage {
    fn bind(
        &self,
        op: &InputIntent,
        _state: &DeviceState,
        layout: &Layout,
    ) -> Option<BusinessIntent> {
        let tap = |intent| {
            Some(BusinessIntent::Gallery {
                wire: PageId::Extended.code(),
                intent,
            })
        };
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
            return tap(GalleryIntent::Key { key, at_ms });
        }
        if let InputEvent::Gesture(GestureEvent::Tap { x, y, .. }) = op.event
            && !self.scroll.dragging()
        {
            let area = self.scroll.area();
            if area.rect.contains(i32::from(x), i32::from(y))
                && let Some(keypad) = layout.slot(KEY_SLOT)
                && let Some(cell) = Keypad::key_at(keypad, i32::from(x), i32::from(y))
            {
                return tap(GalleryIntent::Pad { cell });
            }
        }
        self.scroll.bind(
            op,
            PageId::Extended.code(),
            |x, y| self.tag_at(layout, x, y),
            |_| false,
        )
    }

    fn publish(&self, out: &mut PageSnapshot) {
        out.extended = self.snapshot();
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        declare(snapshot, ui);
    }

    fn descriptor() -> Descriptor {
        Descriptor::new(Self::view)
            .with_custom(custom)
            .with_repaint_ms(REPAINT_MS)
            .with_repaints(Repaint::WhenOrOverlay(extended_moved))
            .with_scroll(extended_shift)
    }

    fn tick(&mut self, step_ms: u16) {
        self.scroll.tick(step_ms);
    }

    fn on_hide(&mut self) {
        self.scroll.reset();
        self.values.focus = false;
    }
}

fn declare(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
    let lang = snapshot.lang;
    let w = snapshot.pages.extended;
    let theme = *Theme::of(snapshot.theme);
    page::body(ui, |ui| {
        page::title(ui, Msg::ExtendedTitle.bytes(lang));
        if w.focus {
            ui.add(
                SearchField::new(w.text.as_bytes(), Msg::Search.bytes(lang))
                    .width(CONTENT_W)
                    .focused(true),
            );
            ui.custom(Style::new().width(CONTENT_W).height(keyboard::H), KEY_SLOT);
            return;
        }
        page::scroller(ui, AREA, w.area, |ui| {
            page::caption(ui, Msg::CapSearch.bytes(lang), &theme);
            ui.add(
                SearchField::new(w.text.as_bytes(), Msg::Search.bytes(lang))
                    .width(CONTENT_W)
                    .tag(Action::Search.tag()),
            );
            page::caption(ui, Msg::CapChips.bytes(lang), &theme);
            ui.add(
                FilterChips::new(
                    [
                        Msg::Filled.bytes(lang),
                        Msg::Outlined.bytes(lang),
                        Msg::Bare.bytes(lang),
                    ],
                    [
                        Action::Chip0.tag(),
                        Action::Chip1.tag(),
                        Action::Chip2.tag(),
                    ],
                    w.chips,
                )
                .theme(&theme),
            );
            page::caption(ui, Msg::CapForm.bytes(lang), &theme);
            ui.add(
                FormField::new(
                    Msg::Name.bytes(lang),
                    w.text.as_bytes(),
                    Msg::Mail.bytes(lang),
                )
                .state(State::NONE.then(State::ERROR, w.form_error))
                .width(CONTENT_W)
                .tag(Action::Form.tag()),
            );
            page::caption(ui, Msg::CapNumber.bytes(lang), &theme);
            ui.add(NumberField::new(
                w.number,
                [Action::Dec.tag(), Action::Inc.tag()],
            ));
            page::caption(ui, Msg::CapKeypad.bytes(lang), &theme);
            ui.custom(
                Style::new()
                    .width(CONTENT_W)
                    .height(KEYPAD_H)
                    .tag(TAG_KEYPAD),
                KEY_SLOT,
            );
            let key: &[u8] = match w.key {
                Some(cell) => {
                    let column = usize::from(cell % 3);
                    &keypad::KEYS[usize::from(cell / 3)][column..column + 1]
                }
                None => b"-",
            };
            page::reading(ui, Msg::Key.bytes(lang), key);
        });
    });
}

fn custom(snapshot: &FrameSnapshot, screen: Screen, slot: u8, canvas: &mut Canvas<'_>, rect: Rect) {
    let theme = *Theme::of(screen.theme);
    let w = snapshot.pages.extended;
    if slot == KEY_SLOT {
        if w.focus {
            keyboard::draw(canvas, rect, w.text, &theme);
        } else {
            Keypad { pressed: w.key }.draw(canvas, rect, &theme);
        }
    }
}

fn extended_moved(prev: &FrameSnapshot, next: &FrameSnapshot) -> bool {
    prev.lang != next.lang || prev.pages.extended != next.pages.extended
}

fn extended_shift(prev: &FrameSnapshot, next: &FrameSnapshot) -> Option<ScrollShift> {
    super::gallery::shift(
        PageId::Extended,
        prev,
        next,
        |pages| pages.extended.area,
        |pages| pages.extended.moving,
    )
}

const _: () = assert!(CONTENT_W + 2 * BAR + page::MARGIN == PANEL_COLUMNS as i16);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{LocalPage, Service};
    use crate::shared::i18n::Lang;
    use alloc::vec::Vec;
    use iot_core::ui::panel::CONTENT_BOTTOM;
    use iot_core::ui::{Layout, PX};

    fn layout(page: &ExtendedPage) -> Layout {
        crate::services::layout_of(page, PageId::Extended)
    }

    fn tap(page: &mut ExtendedPage, tag: u16) {
        let mut state = DeviceState::default();
        let intent = BusinessIntent::Gallery {
            wire: PageId::Extended.code(),
            intent: GalleryIntent::Tap { tag },
        };
        page.apply(&intent, &mut state, &layout(page));
    }

    #[test]
    fn the_whole_gallery_fits_one_declaration() {
        for lang in Lang::ALL {
            let mut ui = headless();
            ExtendedPage::view(
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
    fn the_focused_field_leaves_room_for_its_keyboard() {
        let mut focused = FrameSnapshot::default();
        focused.pages.extended.focus = true;
        let mut ui = headless();
        ExtendedPage::view(&focused, &mut ui);
        finish(&mut ui);
        assert_eq!(ui.tags().count(), 0, "the gallery is gone while typing");
        assert!(ui.paint_rect(KEY_SLOT).is_some());
    }

    #[test]
    fn every_control_is_drawn_once() {
        let page = ExtendedPage::default();
        let drawn: Vec<u16> = layout(&page)
            .tags()
            .map(|(tag, _)| tag)
            .filter(|tag| Action::from_tag(*tag).is_some())
            .collect();
        let expected: Vec<u16> = Action::ALL.iter().map(|action| action.tag()).collect();
        assert_eq!(drawn, expected);
    }

    #[test]
    fn a_tap_changes_the_control_it_lands_on() {
        let mut page = ExtendedPage::default();
        tap(&mut page, Action::Chip2.tag());
        assert_eq!(page.values.chips, 1 | 4);
        tap(&mut page, Action::Inc.tag());
        assert_eq!(page.values.number, 4);
        tap(&mut page, Action::Dec.tag());
        tap(&mut page, Action::Dec.tag());
        assert_eq!(page.values.number, 2);
        tap(&mut page, Action::Form.tag());
        assert!(page.values.form_error);
        tap(&mut page, Action::Search.tag());
        assert!(page.values.focus);
    }

    #[test]
    fn a_press_then_a_tap_reports_the_cell_under_the_finger() {
        let mut page = ExtendedPage::default();
        let area = page.scroll.area();
        let rect = layout(&page).slot(KEY_SLOT).expect("a pad");
        let target = rect.y + rect.h / 2 - (area.rect.y + area.rect.h / 2);
        page.scroll.park(i16::try_from(target.max(0)).unwrap_or(0));
        let rect = layout(&page).slot(KEY_SLOT).expect("a pad");
        let x = u16::try_from(rect.x + rect.w / 2).unwrap_or(0);
        let y = u16::try_from(rect.y + rect.h / 2).unwrap_or(0);
        assert!(
            page.scroll.area().rect.contains(i32::from(x), i32::from(y)),
            "the cell has to be on screen to be touched"
        );
        let cell = Keypad::key_at(rect, i32::from(x), i32::from(y)).expect("a cell");
        send(&mut page, GestureEvent::Press { id: 0, x, y });
        send(
            &mut page,
            GestureEvent::Tap {
                id: 0,
                x,
                y,
                held_ms: 40,
                at_ms: 40,
            },
        );
        assert_eq!(page.values.key, Some(cell));
    }

    #[test]
    fn the_keypad_carries_its_own_tag() {
        let page = ExtendedPage::default();
        let layout = layout(&page);
        let keypad = layout.slot(KEY_SLOT).expect("a pad");
        assert_eq!(
            layout.at(keypad.x + 2, keypad.y + 2),
            Some(TAG_KEYPAD),
            "the arena claims the scroll unless the pad is tagged"
        );
    }

    fn send(page: &mut ExtendedPage, event: GestureEvent) {
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
                    wire: PageId::Extended.code(),
                    intent,
                },
                &mut state,
                &layout,
            );
        }
    }

    /// The lift after a drag arrives as a tap; the pad must let the arena answer it.
    #[test]
    fn a_dragged_lift_presses_no_key_and_the_next_drag_still_scrolls() {
        let mut page = ExtendedPage::default();
        let area = page.scroll.area();
        let keypad = layout(&page).slot(KEY_SLOT).expect("a pad");
        let target = keypad.y + keypad.h / 2 - (area.rect.y + area.rect.h / 2);
        page.scroll.park(i16::try_from(target.max(0)).unwrap_or(0));
        let keypad = layout(&page).slot(KEY_SLOT).expect("a pad");
        let x = keypad.x + keypad.w / 2;
        let y = keypad.y + keypad.h / 8;
        let (x, y) = (u16::try_from(x).unwrap(), u16::try_from(y).unwrap());
        send(&mut page, GestureEvent::Press { id: 0, x, y });
        send(
            &mut page,
            GestureEvent::Drag {
                id: 0,
                x,
                y: y - 30,
                at_ms: 20,
            },
        );
        send(
            &mut page,
            GestureEvent::Tap {
                id: 0,
                x,
                y: y - 30,
                held_ms: 40,
                at_ms: 40,
            },
        );
        assert!(page.values.key.is_none(), "a lift presses no key");
        assert!(!page.scroll.dragging(), "the arena let the finger go");
        let moved = page.scroll.area().offset;
        send(
            &mut page,
            GestureEvent::Press {
                id: 1,
                x,
                y: y - 30,
            },
        );
        send(
            &mut page,
            GestureEvent::Drag {
                id: 1,
                x,
                y: y - 60,
                at_ms: 20,
            },
        );
        assert!(page.scroll.area().offset > moved, "the next drag scrolls");
    }

    #[test]
    fn the_pad_is_four_rows_of_full_touch_targets() {
        let page = ExtendedPage::default();
        let keypad = layout(&page).slot(KEY_SLOT).expect("a pad");
        assert_eq!(keypad.h % 4, 0);
        assert!(keypad.h / 4 >= i32::from(page::TOUCH));
        for cell in 0..12u8 {
            let column = i32::from(cell % 3);
            let row = i32::from(cell / 3);
            let x = keypad.x + (column * 2 + 1) * keypad.w / 6;
            let y = keypad.y + (row * 2 + 1) * keypad.h / 8;
            assert_eq!(Keypad::key_at(keypad, x, y), Some(cell));
        }
    }

    #[test]
    fn a_scroll_frame_moves_only_the_gallery() {
        let mut page = ExtendedPage::default();
        let before = FrameSnapshot {
            page: PageId::Extended,
            pages: PageSnapshot {
                extended: page.snapshot(),
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
            page: PageId::Extended,
            pages: PageSnapshot {
                extended: page.snapshot(),
                ..PageSnapshot::default()
            },
            ..FrameSnapshot::default()
        };
        assert!(after.pages.extended.area.offset > 0);
        let shift = extended_shift(&before, &after).expect("a pure scroll");
        assert_eq!(shift.shifts()[0], after.pages.extended.area.offset);
    }
}
