//! The feedback-widget page: a standing notice, a toast it raises, placeholder bars, and a busy
//! overlay the finger can take down.

use iot_core::drivers::input::{GestureEvent, InputEvent};
use iot_core::ui::panel::PANEL_COLUMNS;
use iot_core::ui::{ScrollId, Style, Ui};

use crate::shared::i18n::{Lang, Msg};
use crate::shared::intent::{BusinessIntent, GalleryIntent, InputIntent};
use crate::shared::model::{Command, DeviceState, Effects};
use crate::shared::page;
use crate::shared::route::PageId;
use crate::shared::snapshot::{FeedbackSnapshot, FrameSnapshot, PageSnapshot, ScrollShift};
use crate::shared::surface::{finish, headless};
use crate::shared::ui::scroll::{BAR, ScrollConfig};
use crate::shared::ui::tag::TagTable;
use crate::shared::ui::theme::Theme;
use crate::shared::ui::widgets::{Banner, Button, LoadingOverlay, Skeleton, Toast};

use super::gallery::{REPAINT_MS, Scroller};
use super::{Descriptor, Repaint};

const AREA: ScrollId = ScrollId::new(1);
const CONTENT_W: i16 = PANEL_COLUMNS as i16 - page::MARGIN - 2 * BAR;

const SCROLL: ScrollConfig = ScrollConfig {
    bounce: true,
    bar: true,
    pull: 0,
};

/// One control on the page. The discriminant is the tag the declaration draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
enum Action {
    Banner = 1,
    Toast,
    Skeleton,
    Overlay,
}

impl Action {
    const ALL: [Self; 4] = [Self::Banner, Self::Toast, Self::Skeleton, Self::Overlay];

    const fn tag(self) -> u16 {
        self as u16
    }

    fn from_tag(tag: u16) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.tag() == tag)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackPage {
    scroll: Scroller,
    tags: TagTable<{ Action::ALL.len() }>,
    values: FeedbackSnapshot,
}

impl Default for FeedbackPage {
    fn default() -> Self {
        let mut ui = headless();
        declare(&FrameSnapshot::default(), &mut ui);
        finish(&mut ui);
        let mut scroll = Scroller::default();
        scroll.measure(&ui, AREA, SCROLL);
        Self {
            scroll,
            tags: TagTable::measure(&ui),
            values: FeedbackSnapshot {
                skeleton: true,
                ..FeedbackSnapshot::default()
            },
        }
    }
}

impl FeedbackPage {
    fn tag_at(&self, x: i32, y: i32) -> u16 {
        self.scroll
            .tag_at(|x, y, offset| self.tags.at(x, y, offset), x, y)
    }

    fn snapshot(&self) -> FeedbackSnapshot {
        let mut view = self.values;
        view.area = self.scroll.view();
        view.moving = self.scroll.moving();
        view
    }

    fn tap(&mut self, action: Action) {
        match action {
            Action::Banner => {
                self.values.banner = !self.values.banner;
                self.values.toast = true;
            }
            Action::Toast => self.values.toast = !self.values.toast,
            Action::Skeleton => self.values.skeleton = !self.values.skeleton,
            Action::Overlay => self.values.overlay = !self.values.overlay,
        }
    }
}

impl super::Service for FeedbackPage {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        _state: &mut DeviceState,
    ) -> (Option<Command>, Effects) {
        let BusinessIntent::Gallery { wire, intent } = intent else {
            return (None, Effects::default());
        };
        if *wire != PageId::Feedback.code() {
            return (None, Effects::default());
        }
        if let GalleryIntent::Tap { tag } = *intent
            && let Some(action) = Action::from_tag(tag)
        {
            self.tap(action);
        }
        self.scroll.apply(*intent);
        (None, Effects::default())
    }
}

impl super::LocalPage for FeedbackPage {
    fn bind(&self, op: &InputIntent, _state: &DeviceState) -> Option<BusinessIntent> {
        if let InputEvent::Gesture(GestureEvent::Tap { x, y, .. }) = op.event
            && let Some(action) = Action::from_tag(self.tag_at(i32::from(x), i32::from(y)))
        {
            return Some(BusinessIntent::Gallery {
                wire: PageId::Feedback.code(),
                intent: GalleryIntent::Tap { tag: action.tag() },
            });
        }
        self.scroll.bind(
            op,
            PageId::Feedback.code(),
            |x, y| self.tag_at(x, y),
            |_| false,
        )
    }

    fn publish(&self, out: &mut PageSnapshot) {
        out.feedback = self.snapshot();
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        declare(snapshot, ui);
    }

    fn descriptor() -> Descriptor {
        Descriptor::new(Self::view)
            .with_repaint_ms(REPAINT_MS)
            .with_repaints(Repaint::WhenOrOverlay(feedback_moved))
            .with_scroll(feedback_shift)
    }

    fn tick(&mut self, step_ms: u16) {
        self.scroll.tick(step_ms);
    }

    fn on_hide(&mut self) {
        self.scroll.reset();
    }
}

fn declare(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
    let lang = snapshot.lang;
    let w = snapshot.pages.feedback;
    let theme = *Theme::of(snapshot.theme);
    page::body(ui, |ui| {
        let ink = ui.color(theme.ink);
        let muted = ui.color(theme.muted);
        page::title(ui, Msg::FeedbackTitle.bytes(lang));
        page::scroller(ui, AREA, w.area, |ui| {
            page::caption(ui, Msg::CapBanner.bytes(lang), &theme);
            ui.add(
                Banner::new(
                    word(lang, if w.banner { Msg::Saved } else { Msg::Failed }),
                    Msg::Ok.bytes(lang),
                )
                .tag(Action::Banner.tag()),
            );
            page::caption(ui, Msg::CapToast.bytes(lang), &theme);
            if w.toast {
                ui.add(
                    Toast::new(Msg::Saved.bytes(lang))
                        .width(CONTENT_W)
                        .height(page::TOUCH)
                        .tag(Action::Toast.tag()),
                );
            } else {
                ui.add(
                    Button::new(Msg::Toast.bytes(lang))
                        .width(CONTENT_W)
                        .tag(Action::Toast.tag()),
                );
            }
            page::caption(ui, Msg::CapSkeleton.bytes(lang), &theme);
            ui.column(Style::new().height(page::TOUCH * 2).gap(2), |ui| {
                if w.skeleton {
                    ui.add(Skeleton::new(CONTENT_W));
                } else {
                    ui.text(Msg::Saved.bytes(lang), Style::new().nowrap().color(ink));
                    ui.text(Msg::Detail.bytes(lang), Style::new().nowrap().color(muted));
                    ui.text(Msg::Ended.bytes(lang), Style::new().nowrap().color(muted));
                }
            });
            ui.add(
                Button::new(Msg::Loading.bytes(lang))
                    .width(CONTENT_W)
                    .tag(Action::Skeleton.tag()),
            );

            page::caption(ui, Msg::CapOverlay.bytes(lang), &theme);
            if w.overlay {
                ui.add(
                    LoadingOverlay::new(Msg::Busy.bytes(lang))
                        .on(true)
                        .size(CONTENT_W, page::TOUCH)
                        .tag(Action::Overlay.tag()),
                );
            } else {
                ui.add(
                    Button::new(Msg::Busy.bytes(lang))
                        .width(CONTENT_W)
                        .tag(Action::Overlay.tag()),
                );
            }
        });
    });
}

fn word(lang: Lang, msg: Msg) -> &'static [u8] {
    msg.bytes(lang)
}

fn feedback_moved(prev: &FrameSnapshot, next: &FrameSnapshot) -> bool {
    prev.lang != next.lang || prev.pages.feedback != next.pages.feedback
}

fn feedback_shift(prev: &FrameSnapshot, next: &FrameSnapshot) -> Option<ScrollShift> {
    super::gallery::shift(
        PageId::Feedback,
        prev,
        next,
        |pages| pages.feedback.area,
        |pages| pages.feedback.moving,
    )
}

const _: () = assert!(CONTENT_W + 2 * BAR + page::MARGIN == PANEL_COLUMNS as i16);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{LocalPage, Service};
    use crate::shared::i18n::Lang;
    use alloc::vec::Vec;
    use iot_core::ui::PX;
    use iot_core::ui::panel::CONTENT_BOTTOM;

    fn tap(page: &mut FeedbackPage, action: Action) {
        let mut state = DeviceState::default();
        page.apply(
            &BusinessIntent::Gallery {
                wire: PageId::Feedback.code(),
                intent: GalleryIntent::Tap { tag: action.tag() },
            },
            &mut state,
        );
    }

    #[test]
    fn the_whole_gallery_fits_one_declaration() {
        for lang in Lang::ALL {
            let mut ui = headless();
            FeedbackPage::view(
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
        let page = FeedbackPage::default();
        let drawn: Vec<u16> = page.tags.iter().map(|(tag, _)| tag).collect();
        let expected: Vec<u16> = Action::ALL.iter().map(|action| action.tag()).collect();
        assert_eq!(drawn, expected);
    }

    #[test]
    fn the_banner_raises_a_toast_and_the_toast_takes_itself_down() {
        let mut page = FeedbackPage::default();
        assert!(!page.values.toast);
        tap(&mut page, Action::Banner);
        assert!(page.values.toast);
        assert!(page.values.banner);
        tap(&mut page, Action::Toast);
        assert!(!page.values.toast);
        tap(&mut page, Action::Banner);
        assert!(page.values.toast);
        assert!(!page.values.banner, "the second tap flips the notice");
    }

    #[test]
    fn the_placeholder_swaps_for_content_and_the_overlay_covers_the_box() {
        let mut page = FeedbackPage::default();
        assert!(page.values.skeleton);
        tap(&mut page, Action::Skeleton);
        assert!(!page.values.skeleton);
        assert!(!page.values.overlay);
        tap(&mut page, Action::Overlay);
        assert!(page.values.overlay);
        tap(&mut page, Action::Overlay);
        assert!(!page.values.overlay);
    }

    #[test]
    fn the_overlay_is_a_touch_target_while_it_is_up() {
        let mut page = FeedbackPage::default();
        tap(&mut page, Action::Overlay);
        let rect = page.tags.rect(Action::Overlay.tag()).expect("the box");
        let area = page.scroll.area();
        let max = i32::from(area.max());
        let target = (rect.y + rect.h / 2 - (area.rect.y + area.rect.h / 2)).clamp(0, max);
        page.scroll.park(i16::try_from(target).unwrap_or(0));
        let x = u16::try_from(rect.x + rect.w / 2).unwrap();
        let y = u16::try_from(rect.y + rect.h / 2 - target).unwrap();
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
            .expect("the overlay takes the tap");
        let mut state = DeviceState::default();
        page.apply(&intent, &mut state);
        assert!(!page.values.overlay);
    }

    #[test]
    fn a_scroll_frame_moves_only_the_gallery() {
        let mut page = FeedbackPage::default();
        let before = FrameSnapshot {
            page: PageId::Feedback,
            pages: PageSnapshot {
                feedback: page.snapshot(),
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
            page: PageId::Feedback,
            pages: PageSnapshot {
                feedback: page.snapshot(),
                ..PageSnapshot::default()
            },
            ..FrameSnapshot::default()
        };
        assert!(after.pages.feedback.area.offset > 0);
        let shift = feedback_shift(&before, &after).expect("a pure scroll");
        assert_eq!(shift.shifts()[0], after.pages.feedback.area.offset);
    }
}
