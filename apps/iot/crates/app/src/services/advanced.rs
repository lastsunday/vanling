//! The advanced-widgets page: the navigation furniture, the composed controls, and the card
//! pager whose index its dots share.

use iot_core::drivers::input::{GestureEvent, InputEvent};
use iot_core::ui::panel::PANEL_COLUMNS;
use iot_core::ui::{Canvas, Rect, Screen, ScrollId, Style, Ui};

use crate::shared::i18n::{Lang, Msg};
use crate::shared::intent::{BusinessIntent, GalleryIntent, InputIntent};
use crate::shared::model::{Command, DeviceState, Effects};
use crate::shared::page;
use crate::shared::route::PageId;
use crate::shared::snapshot::{AdvancedSnapshot, FrameSnapshot, PageSnapshot, ScrollShift};
use crate::shared::surface::{finish, headless};
use crate::shared::ui::scroll::{BAR, ScrollConfig};
use crate::shared::ui::tag::TagTable;
use crate::shared::ui::theme::Theme;
use crate::shared::ui::widgets::{
    Accordion, AppBar, NavigationBar, Pager, PagerCard, Segmented, Stepper, Tabs, dots, pager,
};

use super::gallery::{REPAINT_MS, Scroller};
use super::{Descriptor, Repaint};

const AREA: ScrollId = ScrollId::new(1);
const CONTENT_W: i16 = PANEL_COLUMNS as i16 - page::MARGIN - 2 * BAR;
const PAGER_SLOT: u8 = 0;
const DOT_SLOT: u8 = 1;
const PAGER_COUNT: u8 = 3;
/// A card tall enough for a title, a mark and a detail line, with air around it.
const PAGER_H: i16 = 2 * page::TOUCH;

/// The dots and the cards share one tag range: either picks a pager index.
const TAG_INDEX: u16 = 60;

const SCROLL: ScrollConfig = ScrollConfig {
    bounce: true,
    bar: true,
    pull: 0,
};

/// One control on the page. The discriminant is the tag the declaration draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
enum Action {
    App = 1,
    Nav0,
    Nav1,
    Nav2,
    Tab0,
    Tab1,
    Tab2,
    Pager,
    Accordion,
    Seg0,
    Seg1,
    Seg2,
    Step,
}

impl Action {
    const ALL: [Self; 13] = [
        Self::App,
        Self::Nav0,
        Self::Nav1,
        Self::Nav2,
        Self::Tab0,
        Self::Tab1,
        Self::Tab2,
        Self::Pager,
        Self::Accordion,
        Self::Seg0,
        Self::Seg1,
        Self::Seg2,
        Self::Step,
    ];

    const fn tag(self) -> u16 {
        self as u16
    }

    fn from_tag(tag: u16) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.tag() == tag)
    }
}

fn index_tag(index: u8) -> u16 {
    TAG_INDEX + u16::from(index)
}

fn tag_index(tag: u16) -> Option<u8> {
    tag.checked_sub(TAG_INDEX)
        .filter(|index| *index < u16::from(PAGER_COUNT))
        .and_then(|index| u8::try_from(index).ok())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvancedPage {
    scroll: Scroller,
    tags: TagTable<{ Action::ALL.len() }>,
    dots: Rect,
    pager: Rect,
    values: AdvancedSnapshot,
}

impl Default for AdvancedPage {
    fn default() -> Self {
        let mut ui = headless();
        declare(&FrameSnapshot::default(), &mut ui);
        finish(&mut ui);
        let mut scroll = Scroller::default();
        scroll.measure(&ui, AREA, SCROLL);
        Self {
            scroll,
            tags: TagTable::measure(&ui),
            dots: ui.paint_rect(DOT_SLOT).unwrap_or_default(),
            pager: ui.paint_rect(PAGER_SLOT).unwrap_or_default(),
            values: AdvancedSnapshot {
                tab: 1,
                open: true,
                pager: 1,
                ..AdvancedSnapshot::default()
            },
        }
    }
}

impl AdvancedPage {
    fn tag_at(&self, x: i32, y: i32) -> u16 {
        self.scroll
            .tag_at(|x, y, offset| self.tags.at(x, y, offset), x, y)
    }

    fn snapshot(&self) -> AdvancedSnapshot {
        let mut view = self.values;
        view.area = self.scroll.view();
        view.moving = self.scroll.moving();
        view
    }

    fn tap(&mut self, action: Action) {
        match action {
            Action::App => self.values.action = !self.values.action,
            Action::Nav0 => self.values.nav = 0,
            Action::Nav1 => self.values.nav = 1,
            Action::Nav2 => self.values.nav = 2,
            Action::Tab0 => self.values.tab = 0,
            Action::Tab1 => self.values.tab = 1,
            Action::Tab2 => self.values.tab = 2,
            Action::Accordion => self.values.open = !self.values.open,
            Action::Seg0 => self.values.segment = 0,
            Action::Seg1 => self.values.segment = 1,
            Action::Seg2 => self.values.segment = 2,
            Action::Step => self.values.step = (self.values.step + 1) % 3,
            Action::Pager => self.values.pager = (self.values.pager + 1) % PAGER_COUNT,
        }
    }

    fn tap_tag(&mut self, tag: u16) {
        if let Some(index) = tag_index(tag) {
            self.values.pager = index;
            return;
        }
        if let Some(action) = Action::from_tag(tag) {
            self.tap(action);
        }
    }
}

impl super::Service for AdvancedPage {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        _state: &mut DeviceState,
    ) -> (Option<Command>, Effects) {
        let BusinessIntent::Gallery { wire, intent } = intent else {
            return (None, Effects::default());
        };
        if *wire != PageId::Advanced.code() {
            return (None, Effects::default());
        }
        match *intent {
            GalleryIntent::Tap { tag } => self.tap_tag(tag),
            GalleryIntent::Slide { tag, dx, .. } if tag == Action::Pager.tag() => {
                self.values.drag = dx;
            }
            GalleryIntent::SlideEnd { tag, dx, .. } if tag == Action::Pager.tag() => {
                self.values.pager = pager::turn(self.values.pager, PAGER_COUNT, self.pager, dx);
                self.values.drag = 0;
            }
            _ => {}
        }
        self.scroll.apply(*intent);
        (None, Effects::default())
    }
}

impl super::LocalPage for AdvancedPage {
    fn bind(&self, op: &InputIntent, _state: &DeviceState) -> Option<BusinessIntent> {
        let gallery = |intent| {
            Some(BusinessIntent::Gallery {
                wire: PageId::Advanced.code(),
                intent,
            })
        };
        if let InputEvent::Gesture(GestureEvent::Tap { x, y, .. }) = op.event
            && !self.scroll.dragging()
        {
            let area = self.scroll.area();
            let (x, y) = (i32::from(x), i32::from(y));
            let mut dots = self.dots;
            dots.y -= i32::from(area.offset);
            let mut pager = self.pager;
            pager.y -= i32::from(area.offset);
            if area.rect.contains(x, y)
                && let Some(index) = dots::dot_at(dots, PAGER_COUNT, x, y)
                    .or_else(|| pager::page_at(pager, self.values.pager, PAGER_COUNT, x, y))
            {
                return gallery(GalleryIntent::Tap {
                    tag: index_tag(index),
                });
            }
        }
        self.scroll.bind(
            op,
            PageId::Advanced.code(),
            |x, y| self.tag_at(x, y),
            |tag| tag == Action::Pager.tag(),
        )
    }

    fn publish(&self, out: &mut PageSnapshot) {
        out.advanced = self.snapshot();
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        declare(snapshot, ui);
    }

    fn descriptor() -> Descriptor {
        Descriptor::new(Self::view)
            .with_custom(custom)
            .with_repaint_ms(REPAINT_MS)
            .with_repaints(Repaint::WhenOrOverlay(advanced_moved))
            .with_scroll(advanced_shift)
    }

    fn tick(&mut self, step_ms: u16) {
        self.scroll.tick(step_ms);
    }

    fn on_hide(&mut self) {
        self.scroll.reset();
        self.values.drag = 0;
    }
}

fn declare(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
    let lang = snapshot.lang;
    let w = snapshot.pages.advanced;
    let theme = Theme::PANEL;
    page::body(ui, |ui| {
        page::title(ui, Msg::AdvancedTitle.bytes(lang));
        page::scroller(ui, AREA, w.area, |ui| {
            page::caption(ui, Msg::CapAppBar.bytes(lang), &theme);
            ui.add(
                AppBar::new(
                    Msg::Brand.bytes(lang),
                    word(lang, if w.action { Msg::Off } else { Msg::On }),
                )
                .tag(Action::App.tag()),
            );
            page::caption(ui, Msg::CapNav.bytes(lang), &theme);
            ui.add(NavigationBar::new(
                [
                    Msg::EnvTitle.bytes(lang),
                    Msg::AudioTitle.bytes(lang),
                    Msg::CameraTitle.bytes(lang),
                ],
                [Action::Nav0.tag(), Action::Nav1.tag(), Action::Nav2.tag()],
                w.nav,
            ));
            page::caption(ui, Msg::CapTabs.bytes(lang), &theme);
            ui.add(Tabs::new(
                [
                    Msg::PageMenu.bytes(lang),
                    Msg::PageInput.bytes(lang),
                    Msg::PageText.bytes(lang),
                ],
                [Action::Tab0.tag(), Action::Tab1.tag(), Action::Tab2.tag()],
                w.tab,
            ));
            page::caption(ui, Msg::CapPager.bytes(lang), &theme);
            ui.custom(
                Style::new()
                    .width(CONTENT_W)
                    .height(PAGER_H)
                    .tag(Action::Pager.tag()),
                PAGER_SLOT,
            );
            ui.custom(Style::new().width(CONTENT_W).height(page::TOUCH), DOT_SLOT);

            page::caption(ui, Msg::CapAccordion.bytes(lang), &theme);
            ui.add(
                Accordion::new(Msg::Details.bytes(lang), Msg::BodyCopy.bytes(lang))
                    .open(w.open)
                    .tag(Action::Accordion.tag()),
            );
            page::caption(ui, Msg::CapSegmented.bytes(lang), &theme);
            ui.add(
                Segmented::new(
                    [
                        Msg::Filled.bytes(lang),
                        Msg::Outlined.bytes(lang),
                        Msg::Bare.bytes(lang),
                    ],
                    [Action::Seg0.tag(), Action::Seg1.tag(), Action::Seg2.tag()],
                    w.segment,
                )
                .theme(&theme),
            );
            page::caption(ui, Msg::CapStepper.bytes(lang), &theme);
            ui.add(
                Stepper::new(
                    [
                        Msg::Name.bytes(lang),
                        Msg::Mail.bytes(lang),
                        Msg::Save.bytes(lang),
                    ],
                    w.step,
                )
                .tag(Action::Step.tag()),
            );
        });
    });
}

fn word(lang: Lang, msg: Msg) -> &'static [u8] {
    msg.bytes(lang)
}

fn cards(lang: Lang) -> [PagerCard<'static>; PAGER_COUNT as usize] {
    [
        PagerCard {
            title: Msg::Filled.bytes(lang),
            detail: Msg::CardDetail.bytes(lang),
            mark: '1',
        },
        PagerCard {
            title: Msg::Outlined.bytes(lang),
            detail: Msg::CardDetail.bytes(lang),
            mark: '2',
        },
        PagerCard {
            title: Msg::Bare.bytes(lang),
            detail: Msg::CardDetail.bytes(lang),
            mark: '3',
        },
    ]
}

fn custom(
    snapshot: &FrameSnapshot,
    _screen: Screen,
    slot: u8,
    canvas: &mut Canvas<'_>,
    rect: Rect,
) {
    let theme = Theme::PANEL;
    let w = snapshot.pages.advanced;
    match slot {
        PAGER_SLOT => Pager {
            cards: &cards(snapshot.lang),
            index: w.pager,
            drag: w.drag,
        }
        .draw(canvas, rect, &theme),
        DOT_SLOT => dots::draw(canvas, rect, PAGER_COUNT, w.pager, &theme),
        _ => {}
    }
}

fn advanced_moved(prev: &FrameSnapshot, next: &FrameSnapshot) -> bool {
    prev.lang != next.lang || prev.pages.advanced != next.pages.advanced
}

fn advanced_shift(prev: &FrameSnapshot, next: &FrameSnapshot) -> Option<ScrollShift> {
    super::gallery::shift(
        PageId::Advanced,
        prev,
        next,
        |pages| pages.advanced.area,
        |pages| pages.advanced.moving,
    )
}

const _: () = assert!(CONTENT_W + 2 * BAR + page::MARGIN == PANEL_COLUMNS as i16);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{LocalPage, Service};
    use alloc::vec::Vec;
    use iot_core::ui::PX;
    use iot_core::ui::panel::CONTENT_BOTTOM;

    fn tap(page: &mut AdvancedPage, action: Action) {
        tap_tag(page, action.tag());
    }

    fn tap_tag(page: &mut AdvancedPage, tag: u16) {
        let mut state = DeviceState::default();
        page.apply(
            &BusinessIntent::Gallery {
                wire: PageId::Advanced.code(),
                intent: GalleryIntent::Tap { tag },
            },
            &mut state,
        );
    }

    #[test]
    fn the_whole_gallery_fits_one_declaration() {
        for lang in Lang::ALL {
            let mut ui = headless();
            AdvancedPage::view(
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
    fn every_control_is_drawn_once() {
        let page = AdvancedPage::default();
        let drawn: Vec<u16> = page.tags.iter().map(|(tag, _)| tag).collect();
        let expected: Vec<u16> = Action::ALL.iter().map(|action| action.tag()).collect();
        assert_eq!(drawn, expected);
    }

    #[test]
    fn a_tap_changes_the_control_it_lands_on() {
        let mut page = AdvancedPage::default();
        tap(&mut page, Action::Nav2);
        assert_eq!(page.values.nav, 2);
        tap(&mut page, Action::Tab0);
        assert_eq!(page.values.tab, 0);
        tap(&mut page, Action::Seg2);
        assert_eq!(page.values.segment, 2);
        tap(&mut page, Action::Step);
        assert_eq!(page.values.step, 1);
        tap(&mut page, Action::Accordion);
        assert!(!page.values.open);
        tap(&mut page, Action::App);
        assert!(page.values.action);
    }

    #[test]
    fn the_dots_and_the_pager_share_one_index() {
        let mut page = AdvancedPage::default();
        assert_eq!(page.values.pager, 1);
        tap_tag(&mut page, index_tag(2));
        assert_eq!(page.values.pager, 2);
        tap(&mut page, Action::Pager);
        assert_eq!(page.values.pager, 0, "a card tap advances and wraps");
    }

    fn send(page: &mut AdvancedPage, event: GestureEvent) {
        let op = InputIntent {
            source: 0,
            event: InputEvent::Gesture(event),
        };
        let mut state = DeviceState::default();
        if let Some(bound) = page.bind(&op, &state) {
            let BusinessIntent::Gallery { intent, .. } = bound else {
                panic!("not a gallery intent")
            };
            page.apply(
                &BusinessIntent::Gallery {
                    wire: PageId::Advanced.code(),
                    intent,
                },
                &mut state,
            );
        }
    }

    /// The lift after a drag arrives as a tap; the pager must let the arena answer it.
    #[test]
    fn a_dragged_lift_turns_no_card_and_the_next_drag_still_scrolls() {
        let mut page = AdvancedPage::default();
        let area = page.scroll.area();
        let target = page.pager.y + page.pager.h / 2 - (area.rect.y + area.rect.h / 2);
        page.scroll.park(i16::try_from(target.max(0)).unwrap_or(0));
        let offset = page.scroll.area().offset;
        let card = pager::card_width(page.pager);
        let x = page.pager.x + (page.pager.w - card) / 2 + card / 2;
        let y = page.pager.y + page.pager.h / 2 - i32::from(offset);
        let (x, y) = (u16::try_from(x).unwrap(), u16::try_from(y).unwrap());
        let index = page.values.pager;
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
        assert_eq!(page.values.pager, index, "a lift turns no page");
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
    fn a_tap_on_a_peeking_card_picks_that_card() {
        let mut page = AdvancedPage::default();
        let area = page.scroll.area();
        let target = page.pager.y + page.pager.h / 2 - (area.rect.y + area.rect.h / 2);
        page.scroll.park(i16::try_from(target.max(0)).unwrap_or(0));
        let offset = i32::from(page.scroll.area().offset);
        let card = pager::card_width(page.pager);
        let step = pager::pitch(page.pager);
        let origin = page.pager.x + (page.pager.w - card) / 2;
        let x = u16::try_from(origin + step + 4).unwrap();
        let y = u16::try_from(page.pager.y + page.pager.h / 2 - offset).unwrap();
        let intent = page
            .bind(
                &InputIntent {
                    source: 0,
                    event: InputEvent::Gesture(GestureEvent::Tap {
                        id: 0,
                        x,
                        y,
                        held_ms: 0,
                        at_ms: 0,
                    }),
                },
                &DeviceState::default(),
            )
            .expect("the card takes the tap");
        let mut state = DeviceState::default();
        page.apply(&intent, &mut state);
        assert_eq!(page.values.pager, 2, "the right peek is the next card");
    }

    #[test]
    fn a_slide_turns_the_pager_by_its_release_travel() {
        let mut page = AdvancedPage::default();
        let mut state = DeviceState::default();
        for intent in [
            GalleryIntent::Press {
                id: 0,
                tag: Action::Pager.tag(),
                x: 100,
                y: 100,
            },
            GalleryIntent::Slide {
                id: 0,
                tag: Action::Pager.tag(),
                dx: -80,
            },
            GalleryIntent::SlideEnd {
                id: 0,
                tag: Action::Pager.tag(),
                dx: -80,
            },
        ] {
            page.apply(
                &BusinessIntent::Gallery {
                    wire: PageId::Advanced.code(),
                    intent,
                },
                &mut state,
            );
        }
        assert_eq!(page.values.pager, 2, "a left drag advances one card");
        assert_eq!(page.values.drag, 0, "the release snaps");
    }

    #[test]
    fn every_dot_is_touch_wide_and_picks_its_own_card() {
        let mut page = AdvancedPage::default();
        assert!(
            dots::fit(page.dots) >= PAGER_COUNT,
            "the strip holds every dot"
        );
        let area = page.scroll.area();
        let target = page.dots.y + page.dots.h / 2 - (area.rect.y + area.rect.h / 2);
        page.scroll.park(i16::try_from(target.max(0)).unwrap_or(0));
        let offset = i32::from(page.scroll.area().offset);
        let span = i32::from(PAGER_COUNT - 1) * dots::PITCH;
        let left = page.dots.x + (page.dots.w - span) / 2;
        let x = u16::try_from(left).unwrap();
        let y = u16::try_from(page.dots.y + page.dots.h / 2 - offset).unwrap();
        let intent = page
            .bind(
                &InputIntent {
                    source: 0,
                    event: InputEvent::Gesture(GestureEvent::Tap {
                        id: 0,
                        x,
                        y,
                        held_ms: 0,
                        at_ms: 0,
                    }),
                },
                &DeviceState::default(),
            )
            .expect("the first dot takes the tap");
        let mut state = DeviceState::default();
        page.apply(&intent, &mut state);
        assert_eq!(page.values.pager, 0);
    }

    #[test]
    fn a_scroll_frame_moves_only_the_gallery() {
        let mut page = AdvancedPage::default();
        let before = FrameSnapshot {
            page: PageId::Advanced,
            pages: PageSnapshot {
                advanced: page.snapshot(),
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
            page: PageId::Advanced,
            pages: PageSnapshot {
                advanced: page.snapshot(),
                ..PageSnapshot::default()
            },
            ..FrameSnapshot::default()
        };
        assert!(after.pages.advanced.area.offset > 0);
        let shift = advanced_shift(&before, &after).expect("a pure scroll");
        assert_eq!(shift.shifts()[0], after.pages.advanced.area.offset);
    }
}
