//! The advanced-widgets page: the navigation furniture, the composed controls, and the card
//! pager whose index its dots share.

use iot_core::drivers::input::{GestureEvent, InputEvent};
use iot_core::ui::{Canvas, Layout, Rect, Screen, ScrollId, Style, Ui};

use crate::shared::i18n::{Lang, Msg};
use crate::shared::intent::{BusinessIntent, GalleryIntent, InputIntent};
use crate::shared::model::{Command, DeviceState, Effects};
use crate::shared::page::{self, CONTENT_W};
use crate::shared::route::PageId;
use crate::shared::snapshot::{AdvancedSnapshot, FrameSnapshot, PageSnapshot};
use crate::shared::surface::{finish, headless};
use crate::shared::ui::scroll::ScrollConfig;
use crate::shared::ui::tag::Tag;
use crate::shared::ui::theme::Theme;
use crate::shared::ui::widgets::{
    Accordion, AppBar, Carousel, NavigationBar, Pager, PagerCard, Segmented, Stepper, Switch, Tabs,
    carousel, dots, pager,
};

use super::gallery::{REPAINT_MS, Scroller};
use super::{Descriptor, Repaint};

const AREA: ScrollId = ScrollId::new(1);
const PAGER_SLOT: u8 = 0;
const DOT_SLOT: u8 = 1;
const CAROUSEL_SLOT: u8 = 2;
const PAGER_COUNT: u8 = 3;
/// A card tall enough for a title, a mark and a detail line, with air around it.
const PAGER_H: i16 = 2 * page::TOUCH;
/// The carousel's strip plus the dwell bar under it.
const CAROUSEL_H: i16 = PAGER_H + carousel::PROGRESS_H;
/// How long one card dwells before the carousel turns itself.
const DWELL_MS: u16 = 2_000;

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
    Carousel,
    Auto,
    /// The pager's cards and dots: the index travels in the intent's `value`.
    PagerCard = 60,
    /// The carousel's cards: the index travels in the intent's `value`.
    CarouselCard = 70,
}

impl Tag for Action {
    const ALL: &'static [Self] = &[
        Self::App,
        Self::Nav0,
        Self::Nav1,
        Self::Nav2,
        Self::Tab0,
        Self::Tab1,
        Self::Tab2,
        Self::Pager,
        Self::Auto,
        Self::Carousel,
        Self::Accordion,
        Self::Seg0,
        Self::Seg1,
        Self::Seg2,
        Self::Step,
        // Bases the slot arithmetic reports; they are never boxes of their own.
        Self::PagerCard,
        Self::CarouselCard,
    ];

    fn tag(self) -> u16 {
        self as u16
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvancedPage {
    scroll: Scroller,
    /// Milliseconds the carousel's current card has dwelt, carried across ticks.
    dwell: u16,
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
            dwell: 0,
            values: AdvancedSnapshot {
                tab: 1,
                open: true,
                pager: 1,
                carousel: 1,
                ..AdvancedSnapshot::default()
            },
        }
    }
}

impl AdvancedPage {
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
            Action::Carousel => {}
            // Family bases are matched in `tap_tag`; they are never a drawn control.
            Action::PagerCard | Action::CarouselCard => {}
            Action::Auto => {
                self.values.autoplay = !self.values.autoplay;
                self.dwell = 0;
                self.values.progress = 0;
            }
        }
    }

    fn tap_tag(&mut self, tag: u16, value: u16) {
        let card = u8::try_from(value).unwrap_or(u8::MAX);
        if tag == Action::PagerCard.tag() {
            if card < PAGER_COUNT {
                self.values.pager = card;
            }
            return;
        }
        if tag == Action::CarouselCard.tag() {
            if card < PAGER_COUNT {
                self.values.carousel = card;
                self.dwell = 0;
                self.values.progress = 0;
            }
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
        layout: &iot_core::ui::Layout,
    ) -> (Option<Command>, Effects) {
        let BusinessIntent::Gallery { wire, intent } = intent else {
            return (None, Effects::default());
        };
        if *wire != PageId::Advanced.code() {
            return (None, Effects::default());
        }
        match *intent {
            GalleryIntent::Tap { tag, value } => self.tap_tag(tag, value),
            GalleryIntent::Slide { tag, dx, .. } if tag == Action::Pager.tag() => {
                self.values.drag = dx;
            }
            GalleryIntent::SlideEnd { tag, dx, .. } if tag == Action::Pager.tag() => {
                if let Some(pager) = layout.slot(PAGER_SLOT) {
                    self.values.pager = pager::turn(self.values.pager, PAGER_COUNT, pager, dx);
                }
                self.values.drag = 0;
            }
            GalleryIntent::Slide { tag, dx, .. } if tag == Action::Carousel.tag() => {
                self.values.carousel_drag = dx;
            }
            GalleryIntent::SlideEnd { tag, dx, .. } if tag == Action::Carousel.tag() => {
                if let Some(carousel) = layout.slot(CAROUSEL_SLOT) {
                    self.values.carousel =
                        pager::turn(self.values.carousel, PAGER_COUNT, carousel, dx);
                }
                self.values.carousel_drag = 0;
                self.dwell = 0;
            }
            _ => {}
        }
        self.scroll.apply(*intent);
        (None, Effects::default())
    }
}

impl super::LocalPage for AdvancedPage {
    fn bind(
        &self,
        op: &InputIntent,
        _state: &DeviceState,
        layout: &Layout,
    ) -> Option<BusinessIntent> {
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
            if area.rect.contains(x, y) {
                if let Some(index) = layout
                    .slot(DOT_SLOT)
                    .and_then(|dots| dots::dot_at(dots, PAGER_COUNT, x, y))
                    .or_else(|| {
                        layout.slot(PAGER_SLOT).and_then(|pager| {
                            pager::page_at(pager, self.values.pager, PAGER_COUNT, x, y)
                        })
                    })
                {
                    return gallery(GalleryIntent::Tap {
                        tag: Action::PagerCard.tag(),
                        value: u16::from(index),
                    });
                }
                if let Some(carousel) = layout.slot(CAROUSEL_SLOT)
                    && let Some(index) = pager::page_at(
                        carousel::body(carousel),
                        self.values.carousel,
                        PAGER_COUNT,
                        x,
                        y,
                    )
                {
                    return gallery(GalleryIntent::Tap {
                        tag: Action::CarouselCard.tag(),
                        value: u16::from(index),
                    });
                }
            }
        }
        self.scroll.bind(op, PageId::Advanced.code(), layout)
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
            .with_repaints(Repaint::PageOrOverlay)
            .with_scroll()
    }

    fn tick(&mut self, step_ms: u16) {
        self.scroll.tick(step_ms);
        if !self.values.autoplay {
            return;
        }
        self.dwell = self.dwell.saturating_add(step_ms);
        while self.dwell >= DWELL_MS {
            self.dwell -= DWELL_MS;
            self.values.carousel = (self.values.carousel + 1) % PAGER_COUNT;
        }
        self.values.progress = (u32::from(self.dwell) * 100 / u32::from(DWELL_MS)) as u8;
    }

    fn on_hide(&mut self) {
        self.scroll.reset();
        self.values.drag = 0;
        self.values.carousel_drag = 0;
        self.dwell = 0;
        self.values.progress = 0;
    }
}

fn declare(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
    let lang = snapshot.lang;
    let w = snapshot.pages.advanced;
    let theme = *Theme::of(snapshot.theme);
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
                    .tag(Action::Pager.tag())
                    .slide(),
                PAGER_SLOT,
            );
            ui.custom(Style::new().width(CONTENT_W).height(page::TOUCH), DOT_SLOT);

            page::caption(ui, Msg::CapCarousel.bytes(lang), &theme);
            ui.row(Style::new().gap(theme.gap).middle(), |ui| {
                page::caption(ui, Msg::Auto.bytes(lang), &theme);
                ui.add(Switch::new(w.autoplay).tag(Action::Auto.tag()));
            });
            ui.custom(
                Style::new()
                    .width(CONTENT_W)
                    .height(CAROUSEL_H)
                    .tag(Action::Carousel.tag())
                    .slide(),
                CAROUSEL_SLOT,
            );

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

fn custom(snapshot: &FrameSnapshot, screen: Screen, slot: u8, canvas: &mut Canvas<'_>, rect: Rect) {
    let theme = *Theme::of(screen.theme);
    let w = snapshot.pages.advanced;
    match slot {
        PAGER_SLOT => Pager {
            cards: &cards(snapshot.lang),
            index: w.pager,
            drag: w.drag,
        }
        .draw(canvas, rect, &theme),
        DOT_SLOT => dots::draw(canvas, rect, PAGER_COUNT, w.pager, &theme),
        CAROUSEL_SLOT => Carousel {
            cards: &cards(snapshot.lang),
            index: w.carousel,
            drag: w.carousel_drag,
            progress: w.progress,
            running: w.autoplay,
        }
        .draw(canvas, rect, &theme),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{LocalPage, Service};
    use alloc::vec::Vec;
    use iot_core::ui::panel::{CONTENT_BOTTOM, PANEL_COLUMNS};
    use iot_core::ui::{Layout, PX};

    fn layout(page: &AdvancedPage) -> Layout {
        crate::services::layout_of(page, PageId::Advanced)
    }

    fn tap(page: &mut AdvancedPage, action: Action) {
        tap_tag(page, action.tag());
    }

    fn tap_tag(page: &mut AdvancedPage, tag: u16) {
        tap_value(page, tag, 0);
    }

    fn tap_value(page: &mut AdvancedPage, tag: u16, value: u16) {
        let mut state = DeviceState::default();
        let intent = BusinessIntent::Gallery {
            wire: PageId::Advanced.code(),
            intent: GalleryIntent::Tap { tag, value },
        };
        page.apply(&intent, &mut state, &layout(page));
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
        let drawn: Vec<u16> = layout(&page).tags().map(|(tag, _)| tag).collect();
        let expected: Vec<u16> = [
            Action::App,
            Action::Nav0,
            Action::Nav1,
            Action::Nav2,
            Action::Tab0,
            Action::Tab1,
            Action::Tab2,
            Action::Pager,
            Action::Auto,
            Action::Carousel,
            Action::Accordion,
            Action::Seg0,
            Action::Seg1,
            Action::Seg2,
            Action::Step,
        ]
        .iter()
        .map(|action| action.tag())
        .collect();
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
        tap_value(&mut page, Action::PagerCard.tag(), 2);
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
        let layout = layout(page);
        if let Some(bound) = page.bind(&op, &state, &layout) {
            let BusinessIntent::Gallery { intent, .. } = bound else {
                panic!("not a gallery intent")
            };
            page.apply(
                &BusinessIntent::Gallery {
                    wire: PageId::Advanced.code(),
                    intent,
                },
                &mut state,
                &layout,
            );
        }
    }

    /// The lift after a drag arrives as a tap; the pager must let the arena answer it.
    #[test]
    fn a_dragged_lift_turns_no_card_and_the_next_drag_still_scrolls() {
        let mut page = AdvancedPage::default();
        let area = page.scroll.area();
        let pager = layout(&page).slot(PAGER_SLOT).expect("a pager");
        let target = pager.y + pager.h / 2 - (area.rect.y + area.rect.h / 2);
        page.scroll.park(i16::try_from(target.max(0)).unwrap_or(0));
        let pager = layout(&page).slot(PAGER_SLOT).expect("a pager");
        let card = pager::card_width(pager);
        let x = pager.x + (pager.w - card) / 2 + card / 2;
        let y = pager.y + pager.h / 2;
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
        let pager = layout(&page).slot(PAGER_SLOT).expect("a pager");
        let target = pager.y + pager.h / 2 - (area.rect.y + area.rect.h / 2);
        page.scroll.park(i16::try_from(target.max(0)).unwrap_or(0));
        let pager = layout(&page).slot(PAGER_SLOT).expect("a pager");
        let card = pager::card_width(pager);
        let step = pager::pitch(pager);
        let origin = pager.x + (pager.w - card) / 2;
        let x = u16::try_from(origin + step + 4).unwrap();
        let y = u16::try_from(pager.y + pager.h / 2).unwrap();
        let layout = layout(&page);
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
                &layout,
            )
            .expect("the card takes the tap");
        let mut state = DeviceState::default();
        page.apply(&intent, &mut state, &layout);
        assert_eq!(page.values.pager, 2, "the right peek is the next card");
    }

    #[test]
    fn a_slide_turns_the_pager_by_its_release_travel() {
        let mut page = AdvancedPage::default();
        let mut state = DeviceState::default();
        let layout = layout(&page);
        for intent in [
            GalleryIntent::Press {
                id: 0,
                tag: Action::Pager.tag(),
                value: 0,
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
                &layout,
            );
        }
        assert_eq!(page.values.pager, 2, "a left drag advances one card");
        assert_eq!(page.values.drag, 0, "the release snaps");
    }

    #[test]
    fn every_dot_is_touch_wide_and_picks_its_own_card() {
        let mut page = AdvancedPage::default();
        let strip = layout(&page).slot(DOT_SLOT).expect("a dot strip");
        assert!(dots::fit(strip) >= PAGER_COUNT, "the strip holds every dot");
        let area = page.scroll.area();
        let target = strip.y + strip.h / 2 - (area.rect.y + area.rect.h / 2);
        page.scroll.park(i16::try_from(target.max(0)).unwrap_or(0));
        let strip = layout(&page).slot(DOT_SLOT).expect("a dot strip");
        let span = i32::from(PAGER_COUNT - 1) * dots::PITCH;
        let left = strip.x + (strip.w - span) / 2;
        let x = u16::try_from(left).unwrap();
        let y = u16::try_from(strip.y + strip.h / 2).unwrap();
        let layout = layout(&page);
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
                &layout,
            )
            .expect("the first dot takes the tap");
        let mut state = DeviceState::default();
        page.apply(&intent, &mut state, &layout);
        assert_eq!(page.values.pager, 0);
    }

    #[test]
    fn the_auto_switch_turns_the_carousel_on_its_own_clock() {
        let mut page = AdvancedPage::default();
        assert!(!page.values.autoplay, "a carousel waits for a finger");
        page.tick(1_000);
        assert_eq!(page.values.carousel, 1, "a still carousel does not turn");
        tap(&mut page, Action::Auto);
        assert!(page.values.autoplay);
        page.tick(1_000);
        assert_eq!(page.values.carousel, 1, "half the dwell is not a turn");
        assert_eq!(page.values.progress, 50);
        page.tick(1_000);
        assert_eq!(page.values.carousel, 2, "the dwell turns one card");
        assert_eq!(page.values.progress, 0);
        page.tick(2_000);
        assert_eq!(page.values.carousel, 0, "and wraps");
        tap(&mut page, Action::Auto);
        page.tick(1_000);
        assert_eq!(page.values.progress, 0, "the switch stops the clock");
    }

    #[test]
    fn a_swipe_turns_the_carousel_and_restarts_its_dwell() {
        let mut page = AdvancedPage::default();
        let mut state = DeviceState::default();
        let layout = layout(&page);
        for intent in [
            GalleryIntent::Press {
                id: 0,
                tag: Action::Carousel.tag(),
                value: 0,
                x: 100,
                y: 100,
            },
            GalleryIntent::Slide {
                id: 0,
                tag: Action::Carousel.tag(),
                dx: -80,
            },
            GalleryIntent::SlideEnd {
                id: 0,
                tag: Action::Carousel.tag(),
                dx: -80,
            },
        ] {
            page.apply(
                &BusinessIntent::Gallery {
                    wire: PageId::Advanced.code(),
                    intent,
                },
                &mut state,
                &layout,
            );
        }
        assert_eq!(page.values.carousel, 2, "a left drag advances one card");
        assert_eq!(page.values.carousel_drag, 0, "the release snaps");
        assert_eq!(page.values.progress, 0);
    }

    #[test]
    fn a_tap_on_a_carousel_card_picks_it_without_moving_the_pager() {
        let mut page = AdvancedPage::default();
        let area = page.scroll.area();
        let carousel = layout(&page).slot(CAROUSEL_SLOT).expect("a carousel");
        let target = carousel.y + carousel.h / 2 - (area.rect.y + area.rect.h / 2);
        page.scroll.park(i16::try_from(target.max(0)).unwrap_or(0));
        let carousel = layout(&page).slot(CAROUSEL_SLOT).expect("a carousel");
        let card = pager::card_width(carousel);
        let step = pager::pitch(carousel);
        let origin = carousel.x + (carousel.w - card) / 2;
        let x = u16::try_from(origin + step + 4).unwrap();
        let y = u16::try_from(carousel.y + carousel.h / 2).unwrap();
        let layout = layout(&page);
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
                &layout,
            )
            .expect("the card takes the tap");
        let mut state = DeviceState::default();
        page.apply(&intent, &mut state, &layout);
        assert_eq!(page.values.carousel, 2, "the right peek is the next card");
        assert_eq!(page.values.pager, 1, "the pager above keeps its own card");
    }

    #[test]
    fn the_dwell_bar_is_not_a_card() {
        let mut page = AdvancedPage::default();
        let area = page.scroll.area();
        let carousel = layout(&page).slot(CAROUSEL_SLOT).expect("a carousel");
        let target = carousel.y + carousel.h / 2 - (area.rect.y + area.rect.h / 2);
        page.scroll.park(i16::try_from(target.max(0)).unwrap_or(0));
        let carousel = layout(&page).slot(CAROUSEL_SLOT).expect("a carousel");
        let card = pager::card_width(carousel);
        let step = pager::pitch(carousel);
        let origin = carousel.x + (carousel.w - card) / 2;
        let x = u16::try_from(origin + step + 4).unwrap();
        let y = u16::try_from(carousel.bottom() - 2).unwrap();
        let layout = layout(&page);
        if let Some(intent) = page.bind(
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
            &layout,
        ) {
            page.apply(&intent, &mut DeviceState::default(), &layout);
        }
        assert_eq!(page.values.carousel, 1, "the bar is decoration");
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
        let shift = crate::services::gallery::shift(&before, &after).expect("a pure scroll");
        assert_eq!(shift.shifts()[0], after.pages.advanced.area.offset);
    }
}
