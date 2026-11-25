//! The lighting page: where a glow may reach the glass, how it moves, and the panel backlight.

use iot_core::drivers::input::{GestureEvent, InputEvent};
use iot_core::ui::panel::PANEL_COLUMNS;
use iot_core::ui::text::format_u16;
use iot_core::ui::{Rect, Style, TextAlign, Ui};

use crate::shared::i18n::{Lang, Msg};
use crate::shared::intent::{BusinessIntent, InputIntent, LightingControl, LightingIntent};
use crate::shared::model::{Command, DeviceState, Effects, GlassBreath, GlowPattern, GlowScope};
use crate::shared::page;
use crate::shared::snapshot::{FrameSnapshot, LightingView, PageSnapshot};
use crate::shared::surface::{finish, headless};
use crate::shared::ui::state::State;
use crate::shared::ui::tag::TagTable;
use crate::shared::ui::theme::Theme;
use crate::shared::ui::widgets::Slider;
use crate::shared::ui::widgets::segmented::Segmented;
use crate::shared::ui::widgets::slider::KNOB;

use super::{Descriptor, Repaint};

const CONTENT_W: i16 = PANEL_COLUMNS as i16 - page::MARGIN;
const GAP: i16 = 6;
const LABEL_W: i16 = 36;
const VALUE_W: i16 = 36;
const SLIDER_W: i16 = CONTENT_W - LABEL_W - VALUE_W - 2 * GAP;
const ROW: i16 = 20;
const PERIOD_STEP_MS: u16 = 45;
const PEAK_FLOOR: u16 = 32;

const _: () = assert!(SLIDER_W > 0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightingPage {
    breath: GlassBreath,
    tags: TagTable<{ LightingControl::ALL.len() }>,
    grab: Option<(u8, LightingControl)>,
}

impl Default for LightingPage {
    fn default() -> Self {
        let mut ui = headless();
        declare(&FrameSnapshot::default(), &mut ui);
        finish(&mut ui);
        Self {
            breath: GlassBreath::CALM,
            tags: TagTable::measure(&ui),
            grab: None,
        }
    }
}

impl LightingPage {
    fn tag_at(&self, x: i32, y: i32) -> u16 {
        self.tags.at(x, y, 0)
    }

    fn set(&mut self, control: LightingControl, x: u16, state: &mut DeviceState) {
        match control {
            LightingControl::Off => state.glow = GlowScope::Off,
            LightingControl::Page => state.glow = GlowScope::Page,
            LightingControl::All => state.glow = GlowScope::Everywhere,
            LightingControl::PatternBreath => self.breath.pattern = GlowPattern::Breath,
            LightingControl::PatternSteady => self.breath.pattern = GlowPattern::Steady,
            LightingControl::PatternCycle => self.breath.pattern = GlowPattern::Cycle,
            LightingControl::Period
            | LightingControl::Hue
            | LightingControl::Peak
            | LightingControl::Backlight => {
                // A steady glow has no envelope, so the period keeps its value for the next mode.
                if control == LightingControl::Period && self.breath.pattern == GlowPattern::Steady
                {
                    return;
                }
                let Some(rect) = self.tags.rect(control.tag()) else {
                    return;
                };
                let pct = percent_at(rect, i32::from(x));
                match control {
                    LightingControl::Period => self.breath.period_ms = period_at(pct),
                    LightingControl::Hue => self.breath.hue = hue_at(pct),
                    LightingControl::Peak => {
                        self.breath.max_value = peak_at(pct);
                        self.breath.min_value = trough(self.breath.max_value);
                    }
                    LightingControl::Backlight => state.backlight.set_pct(pct),
                    _ => {}
                }
            }
        }
    }
}

impl super::Service for LightingPage {
    fn apply(
        &mut self,
        intent: &BusinessIntent,
        state: &mut DeviceState,
    ) -> (Option<Command>, Effects) {
        let BusinessIntent::Lighting(intent) = intent else {
            return (None, Effects::default());
        };
        match *intent {
            LightingIntent::Press { id, control } => self.grab = Some((id, control)),
            LightingIntent::Slide { control, x } => {
                if self.grab.is_some() {
                    self.set(control, x, state);
                }
            }
            LightingIntent::Tap { control, x } => {
                self.set(control, x, state);
                self.grab = None;
            }
            LightingIntent::LetGo => self.grab = None,
        }
        (None, Effects::default())
    }
}

impl super::LocalPage for LightingPage {
    fn bind(&self, op: &InputIntent, _state: &DeviceState) -> Option<BusinessIntent> {
        let intent = match op.event {
            InputEvent::Gesture(GestureEvent::Press { id, x, y }) => {
                let control = LightingControl::from_tag(self.tag_at(i32::from(x), i32::from(y)))?;
                LightingIntent::Press { id, control }
            }
            InputEvent::Gesture(GestureEvent::Drag { id, x, .. }) => {
                let (held, control) = self.grab?;
                if held != id || !control.slider() {
                    return None;
                }
                LightingIntent::Slide { control, x }
            }
            InputEvent::Gesture(GestureEvent::Tap { id, x, y, .. }) => {
                let control = match self.grab {
                    Some((held, control)) if held == id => control,
                    _ => LightingControl::from_tag(self.tag_at(i32::from(x), i32::from(y)))?,
                };
                LightingIntent::Tap { control, x }
            }
            InputEvent::Gesture(
                GestureEvent::Swipe { .. } | GestureEvent::Ghost | GestureEvent::LongPress { .. },
            ) => {
                self.grab?;
                LightingIntent::LetGo
            }
            _ => return None,
        };
        Some(BusinessIntent::Lighting(intent))
    }

    fn publish(&self, out: &mut PageSnapshot) {
        out.lighting = LightingView {
            breath: self.breath,
        };
    }

    fn view(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
        declare(snapshot, ui);
    }

    fn descriptor() -> Descriptor {
        Descriptor::new(Self::view).with_repaints(Repaint::When(lighting_moved))
    }
}

fn declare(snapshot: &FrameSnapshot, ui: &mut Ui<'_, '_>) {
    let lang = snapshot.lang;
    let theme = Theme::of(snapshot.theme);
    let breath = snapshot.pages.lighting.breath;
    let backlight = snapshot.backlight;
    let steady = breath.pattern == GlowPattern::Steady;
    page::body(ui, |ui| {
        page::title(ui, Msg::PageLighting.bytes(lang));
        ui.add(Segmented::new(
            [
                Msg::ScopeOff.bytes(lang),
                Msg::ScopePage.bytes(lang),
                Msg::ScopeAll.bytes(lang),
            ],
            [
                LightingControl::Off.tag(),
                LightingControl::Page.tag(),
                LightingControl::All.tag(),
            ],
            scope_index(snapshot.glow),
        ));
        page::caption(ui, hint(snapshot.glow, lang), theme);
        ui.add(
            Segmented::new(
                [
                    Msg::PatternBreath.bytes(lang),
                    Msg::PatternSteady.bytes(lang),
                    Msg::PatternCycle.bytes(lang),
                ],
                [
                    LightingControl::PatternBreath.tag(),
                    LightingControl::PatternSteady.tag(),
                    LightingControl::PatternCycle.tag(),
                ],
                pattern_index(breath.pattern),
            )
            .height(ROW),
        );
        let mut buf = [0u8; 10];
        slider_row(
            ui,
            theme,
            Msg::Period.bytes(lang),
            number(&mut buf, breath.period_ms, b"MS"),
            LightingControl::Period,
            period_pct(breath.period_ms),
            steady,
        );
        slider_row(
            ui,
            theme,
            Msg::Hue.bytes(lang),
            number(&mut buf, u16::from(breath.hue), b""),
            LightingControl::Hue,
            hue_pct(breath.hue),
            false,
        );
        slider_row(
            ui,
            theme,
            Msg::Peak.bytes(lang),
            number(&mut buf, u16::from(breath.max_value), b""),
            LightingControl::Peak,
            peak_pct(breath.max_value),
            false,
        );
        page::section(ui, Msg::SectionBacklight.bytes(lang), theme);
        slider_row(
            ui,
            theme,
            Msg::Backlight.bytes(lang),
            number(&mut buf, u16::from(backlight.pct()), b"%"),
            LightingControl::Backlight,
            backlight.pct(),
            false,
        );
        page::caption(ui, note(breath.pattern, lang), theme);
    });
}

fn scope_index(scope: GlowScope) -> u8 {
    match scope {
        GlowScope::Off => 0,
        GlowScope::Page => 1,
        GlowScope::Everywhere => 2,
    }
}

fn pattern_index(pattern: GlowPattern) -> u8 {
    match pattern {
        GlowPattern::Breath => 0,
        GlowPattern::Steady => 1,
        GlowPattern::Cycle => 2,
    }
}

fn hint(scope: GlowScope, lang: Lang) -> &'static [u8] {
    match scope {
        GlowScope::Off => Msg::HintOff.bytes(lang),
        GlowScope::Page => Msg::HintPage.bytes(lang),
        GlowScope::Everywhere => Msg::HintAll.bytes(lang),
    }
}

fn note(pattern: GlowPattern, lang: Lang) -> &'static [u8] {
    match pattern {
        GlowPattern::Breath => Msg::PeakNote.bytes(lang),
        GlowPattern::Steady => Msg::SteadyNote.bytes(lang),
        GlowPattern::Cycle => Msg::CycleNote.bytes(lang),
    }
}

fn slider_row(
    ui: &mut Ui<'_, '_>,
    theme: &Theme,
    label: &[u8],
    value: &[u8],
    control: LightingControl,
    pct: u8,
    disabled: bool,
) {
    let muted = ui.color(theme.muted);
    let style = |width: i16| {
        let style = Style::new().width(width).nowrap();
        if disabled { style.color(muted) } else { style }
    };
    ui.row(Style::new().height(ROW).gap(GAP).middle(), |ui| {
        ui.text(label, style(LABEL_W));
        ui.add(
            Slider::new(pct)
                .width(SLIDER_W)
                .tag(control.tag())
                .state(if disabled {
                    State::DISABLED
                } else {
                    State::NONE
                }),
        );
        ui.text(value, style(VALUE_W).text_align(TextAlign::End));
    });
}

/// `number` with `unit` after it, in `buf`.
fn number<'a>(buf: &'a mut [u8; 10], number: u16, unit: &[u8]) -> &'a [u8] {
    let mut digits = [0u8; 6];
    let digits = format_u16(number, &mut digits);
    let len = digits.len();
    buf[..len].copy_from_slice(digits);
    buf[len..len + unit.len()].copy_from_slice(unit);
    &buf[..len + unit.len()]
}

/// Where `x` sits on a slider's track, as the percent the widget draws its knob at.
fn percent_at(rect: Rect, x: i32) -> u8 {
    let travel = (rect.w - i32::from(KNOB)).max(1);
    let local = (x - rect.x - i32::from(KNOB) / 2).clamp(0, travel);
    u8::try_from(local * 100 / travel).unwrap_or(100)
}

fn period_at(pct: u8) -> u16 {
    GlassBreath::FASTEST_MS + u16::from(pct) * PERIOD_STEP_MS
}

fn hue_at(pct: u8) -> u8 {
    (u16::from(pct) * 255 / 100) as u8
}

fn peak_at(pct: u8) -> u8 {
    (PEAK_FLOOR + u16::from(pct) * (255 - PEAK_FLOOR) / 100) as u8
}

fn trough(peak: u8) -> u8 {
    (u16::from(peak) * PEAK_FLOOR / 255) as u8
}

fn period_pct(ms: u16) -> u8 {
    ((ms.saturating_sub(GlassBreath::FASTEST_MS) / PERIOD_STEP_MS) as u8).min(100)
}

fn hue_pct(hue: u8) -> u8 {
    (u16::from(hue) * 100 / 255) as u8
}

fn peak_pct(peak: u8) -> u8 {
    (u16::from(peak).saturating_sub(PEAK_FLOOR) * 100 / (255 - PEAK_FLOOR)) as u8
}

fn lighting_moved(prev: &FrameSnapshot, next: &FrameSnapshot) -> bool {
    prev.lang != next.lang
        || prev.glow != next.glow
        || prev.backlight != next.backlight
        || prev.pages.lighting != next.pages.lighting
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pattern_strip_switches_the_glass_style() {
        let mut page = LightingPage::default();
        let mut state = DeviceState::default();
        for (control, pattern) in [
            (LightingControl::PatternSteady, GlowPattern::Steady),
            (LightingControl::PatternCycle, GlowPattern::Cycle),
            (LightingControl::PatternBreath, GlowPattern::Breath),
        ] {
            page.set(control, 0, &mut state);
            assert_eq!(page.breath.pattern, pattern);
        }
    }

    #[test]
    fn a_steady_glow_ignores_the_period_slider() {
        let mut page = LightingPage::default();
        let mut state = DeviceState::default();
        page.set(LightingControl::PatternSteady, 0, &mut state);
        let period = page.breath.period_ms;
        page.set(LightingControl::Period, 0, &mut state);
        assert_eq!(page.breath.period_ms, period);

        page.set(LightingControl::PatternBreath, 0, &mut state);
        page.set(LightingControl::Period, 0, &mut state);
        assert_eq!(page.breath.period_ms, GlassBreath::FASTEST_MS);
    }
}
