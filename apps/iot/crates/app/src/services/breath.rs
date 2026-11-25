//! The glass-breath demo: the one page that lets a breath reach the pixels, and the editor for
//! the parameters an alarm raises over every page.

use iot_core::drivers::input::{GestureEvent, InputEvent};
use iot_core::ui::panel::PANEL_COLUMNS;
use iot_core::ui::text::format_u16;
use iot_core::ui::{Style, Ui};

use crate::shared::i18n::Msg;
use crate::shared::intent::{BreathIntent, BusinessIntent, InputIntent};
use crate::shared::model::{Command, DeviceState, Effects, GlassBreath};
use crate::shared::page;
use crate::shared::snapshot::{FrameSnapshot, PageSnapshot};
use crate::shared::surface::{finish, headless};
use crate::shared::ui::tag::TagTable;
use crate::shared::ui::theme::Theme;
use crate::shared::ui::widgets::Button;

use super::{Descriptor, Repaint};

const GAP: i16 = 6;
const CONTENT_W: i16 = PANEL_COLUMNS as i16 - page::MARGIN;
const HALF: i16 = (CONTENT_W - GAP) / 2;

const _: () = assert!(2 * HALF + GAP == CONTENT_W);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreathPage {
    breath: GlassBreath,
    tags: TagTable<{ BreathIntent::ALL.len() }>,
}

impl Default for BreathPage {
    fn default() -> Self {
        let mut ui = headless();
        declare(&FrameSnapshot::default(), &mut ui);
        finish(&mut ui);
        Self {
            breath: GlassBreath::CALM,
            tags: TagTable::measure(&ui),
        }
    }
}

impl BreathPage {
    fn tag_at(&self, x: i32, y: i32) -> u16 {
        self.tags.at(x, y, 0)
    }
}

impl super::Service for BreathPage {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        state: &mut DeviceState,
    ) -> (Option<Command>, Effects) {
        let BusinessIntent::Breath(intent) = intent else {
            return (None, Effects::default());
        };
        match *intent {
            BreathIntent::Calm => self.breath = GlassBreath::CALM,
            BreathIntent::Alert => self.breath = GlassBreath::ALERT,
            BreathIntent::Slower => self.breath = self.breath.slower(),
            BreathIntent::Faster => self.breath = self.breath.faster(),
            BreathIntent::Everywhere => {
                state.glass_breath = if state.glass_breath == Some(self.breath) {
                    None
                } else {
                    Some(self.breath)
                };
            }
        }
        (None, Effects::default())
    }
}

impl super::LocalPage for BreathPage {
    fn bind(&self, op: &InputIntent, _state: &DeviceState) -> Option<BusinessIntent> {
        let InputEvent::Gesture(GestureEvent::Tap { x, y, .. }) = op.event else {
            return None;
        };
        let intent = BreathIntent::from_tag(self.tag_at(i32::from(x), i32::from(y)))?;
        Some(BusinessIntent::Breath(intent))
    }

    fn publish(&self, out: &mut PageSnapshot) {
        out.breath = Some(self.breath);
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        declare(snapshot, ui);
    }

    fn descriptor() -> Descriptor {
        Descriptor::new(Self::view).with_repaints(Repaint::When(breath_moved))
    }
}

fn declare(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
    let lang = snapshot.lang;
    let breath = snapshot.pages.breath.unwrap_or(GlassBreath::CALM);
    page::body(ui, |ui| {
        page::title(ui, Msg::PageBreath.bytes(lang));
        page::caption(ui, Msg::BreathHint.bytes(lang), Theme::of(snapshot.theme));
        ui.row(Style::new().gap(GAP), |ui| {
            ui.add(
                Button::new(Msg::Calm.bytes(lang))
                    .width(HALF)
                    .tag(BreathIntent::Calm.tag()),
            );
            ui.add(
                Button::new(Msg::Alert.bytes(lang))
                    .width(HALF)
                    .tag(BreathIntent::Alert.tag()),
            );
        });
        ui.row(Style::new().gap(GAP), |ui| {
            ui.add(
                Button::new(Msg::Slower.bytes(lang))
                    .width(HALF)
                    .tag(BreathIntent::Slower.tag()),
            );
            ui.add(
                Button::new(Msg::Faster.bytes(lang))
                    .width(HALF)
                    .tag(BreathIntent::Faster.tag()),
            );
        });
        ui.add(
            Button::new(Msg::EveryPage.bytes(lang))
                .width(CONTENT_W)
                .tag(BreathIntent::Everywhere.tag()),
        );
        let mut buf = [0u8; 6];
        page::reading(ui, b"MS", format_u16(breath.period_ms, &mut buf));
        page::reading(ui, b"HUE", format_u16(u16::from(breath.hue), &mut buf));
    });
}

fn breath_moved(prev: &FrameSnapshot, next: &FrameSnapshot) -> bool {
    prev.lang != next.lang || prev.pages.breath != next.pages.breath
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{LocalPage, Service};
    use crate::shared::i18n::Lang;
    use iot_core::ui::PX;
    use iot_core::ui::panel::CONTENT_BOTTOM;

    fn tap(page: &mut BreathPage, intent: BreathIntent, state: &mut DeviceState) {
        page.apply(&BusinessIntent::Breath(intent), state);
    }

    #[test]
    fn the_page_fits_one_declaration_in_every_language() {
        for lang in Lang::ALL {
            let mut ui = headless();
            BreathPage::view(
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
                ui.text_bytes() <= 1024,
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
        let page = BreathPage::default();
        let drawn: alloc::vec::Vec<u16> = page.tags.iter().map(|(tag, _)| tag).collect();
        let expected: alloc::vec::Vec<u16> = BreathIntent::ALL
            .iter()
            .map(|intent| intent.tag())
            .collect();
        assert_eq!(drawn, expected);
    }

    #[test]
    fn a_tap_reports_the_control_under_it() {
        let page = BreathPage::default();
        for intent in BreathIntent::ALL {
            let rect = page.tags.rect(intent.tag()).expect("a drawn control");
            let tap = InputIntent {
                source: 0,
                event: InputEvent::Gesture(GestureEvent::Tap {
                    id: 0,
                    x: u16::try_from(rect.x + rect.w / 2).unwrap(),
                    y: u16::try_from(rect.y + rect.h / 2).unwrap(),
                    held_ms: 0,
                    at_ms: 0,
                }),
            };
            assert_eq!(
                page.bind(&tap, &DeviceState::default()),
                Some(BusinessIntent::Breath(intent))
            );
        }
    }

    #[test]
    fn presets_and_period_steps_move_only_the_page_until_every_page_is_asked() {
        let mut page = BreathPage::default();
        let mut state = DeviceState::default();
        assert_eq!(page.breath, GlassBreath::CALM);

        tap(&mut page, BreathIntent::Alert, &mut state);
        assert_eq!(page.breath, GlassBreath::ALERT);
        assert_eq!(state.glass_breath, None, "the alarm is not global yet");

        tap(&mut page, BreathIntent::Slower, &mut state);
        assert!(page.breath.period_ms > GlassBreath::ALERT.period_ms);
        tap(&mut page, BreathIntent::Faster, &mut state);
        assert_eq!(page.breath.period_ms, GlassBreath::ALERT.period_ms);

        tap(&mut page, BreathIntent::Everywhere, &mut state);
        assert_eq!(state.glass_breath, Some(page.breath));
        tap(&mut page, BreathIntent::Everywhere, &mut state);
        assert_eq!(state.glass_breath, None, "the second tap takes it back");
    }

    #[test]
    fn editing_the_page_does_not_retune_a_raised_alarm() {
        let mut page = BreathPage::default();
        let mut state = DeviceState::default();
        tap(&mut page, BreathIntent::Everywhere, &mut state);
        assert_eq!(state.glass_breath, Some(GlassBreath::CALM));
        tap(&mut page, BreathIntent::Alert, &mut state);
        assert_eq!(
            state.glass_breath,
            Some(GlassBreath::CALM),
            "editing the page does not retune an alarm already raised"
        );
    }
}
