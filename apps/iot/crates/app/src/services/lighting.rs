//! The lighting page: where a glow may reach the glass, how it moves, and the panel backlight.

use iot_core::drivers::input::{GestureEvent, InputEvent};
use iot_core::ui::panel::PANEL_COLUMNS;
use iot_core::ui::text::format_u16;
use iot_core::ui::{Layout, Rect, Style, TextAlign, Ui};

use crate::shared::i18n::{Lang, Msg};
use crate::shared::intent::{BusinessIntent, InputIntent, LightingControl, LightingIntent};
use crate::shared::model::{
    Command, DeviceState, Effects, GlassBreath, GlowPattern, GlowScope, PEAK_FLOOR, trough,
};
use crate::shared::page;
use crate::shared::snapshot::{FrameSnapshot, LightingView, PageSnapshot};
use crate::shared::surface::{finish, headless};
use crate::shared::ui::state::State;
use crate::shared::ui::tag::Tag;
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

const _: () = assert!(SLIDER_W > 0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightingPage {
    breath: GlassBreath,
    grab: Option<(u8, LightingControl)>,
}

impl Default for LightingPage {
    fn default() -> Self {
        let mut ui = headless();
        declare(&FrameSnapshot::default(), &mut ui);
        finish(&mut ui);
        Self {
            breath: GlassBreath::CALM,
            grab: None,
        }
    }
}

impl LightingPage {
    fn set(&mut self, control: LightingControl, x: u16, state: &mut DeviceState, layout: &Layout) {
        match control {
            LightingControl::Off => state.glow = GlowScope::Off,
            LightingControl::Page => state.glow = GlowScope::Page,
            LightingControl::All => state.glow = GlowScope::Everywhere,
            LightingControl::PatternBreath => self.breath.pattern = GlowPattern::Breath,
            LightingControl::PatternSteady => self.breath.pattern = GlowPattern::Steady,
            LightingControl::PatternCycle => self.breath.pattern = GlowPattern::Cycle,
            LightingControl::PatternAurora => self.breath.pattern = GlowPattern::Aurora,
            LightingControl::PatternCandle => self.breath.pattern = GlowPattern::Candle,
            LightingControl::PatternPulse => self.breath.pattern = GlowPattern::Pulse,
            LightingControl::PatternHeartbeat => self.breath.pattern = GlowPattern::Heartbeat,
            LightingControl::PatternLightning => self.breath.pattern = GlowPattern::Lightning,
            LightingControl::PatternSparkle => self.breath.pattern = GlowPattern::Sparkle,
            LightingControl::PatternStrobe => self.breath.pattern = GlowPattern::Strobe,
            LightingControl::PatternPolice => self.breath.pattern = GlowPattern::Police,
            LightingControl::PatternSos => self.breath.pattern = GlowPattern::Sos,
            LightingControl::Period
            | LightingControl::Hue
            | LightingControl::Peak
            | LightingControl::Backlight => {
                // A steady glow has no envelope, so the period keeps its value for the next mode.
                if control == LightingControl::Period && self.breath.pattern == GlowPattern::Steady
                {
                    return;
                }
                let Some(rect) = layout.rect(control.tag()) else {
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
        layout: &iot_core::ui::Layout,
    ) -> (Option<Command>, Effects) {
        let BusinessIntent::Lighting(intent) = intent else {
            return (None, Effects::default());
        };
        match *intent {
            LightingIntent::Press { id, control } => self.grab = Some((id, control)),
            LightingIntent::Slide { control, x } => {
                if self.grab.is_some() {
                    self.set(control, x, state, layout);
                }
            }
            LightingIntent::Tap { control, x } => {
                self.set(control, x, state, layout);
                self.grab = None;
            }
            LightingIntent::LetGo => self.grab = None,
        }
        (None, Effects::default())
    }
}

impl super::LocalPage for LightingPage {
    fn bind(
        &self,
        op: &InputIntent,
        _state: &DeviceState,
        layout: &Layout,
    ) -> Option<BusinessIntent> {
        let intent = match op.event {
            InputEvent::Gesture(GestureEvent::Press { id, x, y }) => {
                let control = LightingControl::from_tag(layout.at(i32::from(x), i32::from(y))?)?;
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
                    _ => LightingControl::from_tag(layout.at(i32::from(x), i32::from(y))?)?,
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
        for row in PATTERN_ROWS {
            ui.add(
                Segmented::new(
                    row.map(|pattern| pattern_msg(pattern).bytes(lang)),
                    row.map(|pattern| pattern_control(pattern).tag()),
                    pattern_cell(breath.pattern, row),
                )
                .height(ROW),
            );
        }
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

/// The three four-cell strips, in the order the page shows them.
const PATTERN_ROWS: [[GlowPattern; 4]; 3] = [
    [
        GlowPattern::Steady,
        GlowPattern::Breath,
        GlowPattern::Cycle,
        GlowPattern::Aurora,
    ],
    [
        GlowPattern::Candle,
        GlowPattern::Pulse,
        GlowPattern::Heartbeat,
        GlowPattern::Lightning,
    ],
    [
        GlowPattern::Sparkle,
        GlowPattern::Strobe,
        GlowPattern::Police,
        GlowPattern::Sos,
    ],
];

/// The cell of `row` that `pattern` lights, or `row.len()` when it lives in another strip.
fn pattern_cell(pattern: GlowPattern, row: [GlowPattern; 4]) -> u8 {
    row.iter()
        .position(|&cell| cell == pattern)
        .map_or(row.len() as u8, |index| index as u8)
}

fn pattern_control(pattern: GlowPattern) -> LightingControl {
    match pattern {
        GlowPattern::Steady => LightingControl::PatternSteady,
        GlowPattern::Breath => LightingControl::PatternBreath,
        GlowPattern::Cycle => LightingControl::PatternCycle,
        GlowPattern::Aurora => LightingControl::PatternAurora,
        GlowPattern::Candle => LightingControl::PatternCandle,
        GlowPattern::Pulse => LightingControl::PatternPulse,
        GlowPattern::Heartbeat => LightingControl::PatternHeartbeat,
        GlowPattern::Lightning => LightingControl::PatternLightning,
        GlowPattern::Sparkle => LightingControl::PatternSparkle,
        GlowPattern::Strobe => LightingControl::PatternStrobe,
        GlowPattern::Police => LightingControl::PatternPolice,
        GlowPattern::Sos => LightingControl::PatternSos,
    }
}

fn pattern_msg(pattern: GlowPattern) -> Msg {
    match pattern {
        GlowPattern::Steady => Msg::PatternSteady,
        GlowPattern::Breath => Msg::PatternBreath,
        GlowPattern::Cycle => Msg::PatternCycle,
        GlowPattern::Aurora => Msg::PatternAurora,
        GlowPattern::Candle => Msg::PatternCandle,
        GlowPattern::Pulse => Msg::PatternPulse,
        GlowPattern::Heartbeat => Msg::PatternHeartbeat,
        GlowPattern::Lightning => Msg::PatternLightning,
        GlowPattern::Sparkle => Msg::PatternSparkle,
        GlowPattern::Strobe => Msg::PatternStrobe,
        GlowPattern::Police => Msg::PatternPolice,
        GlowPattern::Sos => Msg::PatternSos,
    }
}

fn note(pattern: GlowPattern, lang: Lang) -> &'static [u8] {
    match pattern {
        GlowPattern::Breath => Msg::PeakNote.bytes(lang),
        GlowPattern::Steady => Msg::SteadyNote.bytes(lang),
        GlowPattern::Cycle => Msg::CycleNote.bytes(lang),
        GlowPattern::Aurora => Msg::AuroraNote.bytes(lang),
        GlowPattern::Candle => Msg::CandleNote.bytes(lang),
        GlowPattern::Pulse => Msg::PulseNote.bytes(lang),
        GlowPattern::Heartbeat => Msg::HeartbeatNote.bytes(lang),
        GlowPattern::Lightning => Msg::LightningNote.bytes(lang),
        GlowPattern::Sparkle => Msg::SparkleNote.bytes(lang),
        GlowPattern::Strobe => Msg::StrobeNote.bytes(lang),
        GlowPattern::Police => Msg::PoliceNote.bytes(lang),
        GlowPattern::Sos => Msg::SosNote.bytes(lang),
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
    (u16::from(PEAK_FLOOR) + u16::from(pct) * (255 - u16::from(PEAK_FLOOR)) / 100) as u8
}

fn period_pct(ms: u16) -> u8 {
    ((ms.saturating_sub(GlassBreath::FASTEST_MS) / PERIOD_STEP_MS) as u8).min(100)
}

fn hue_pct(hue: u8) -> u8 {
    (u16::from(hue) * 100 / 255) as u8
}

fn peak_pct(peak: u8) -> u8 {
    (u16::from(peak).saturating_sub(u16::from(PEAK_FLOOR)) * 100 / (255 - u16::from(PEAK_FLOOR)))
        as u8
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
    use crate::shared::model::EFFECTS;
    use crate::shared::route::PageId;

    fn layout(page: &LightingPage) -> Layout {
        crate::services::layout_of(page, PageId::Lighting)
    }

    #[test]
    fn the_pattern_strip_switches_the_glass_style() {
        let mut page = LightingPage::default();
        let mut state = DeviceState::default();
        for pattern in EFFECTS {
            let layout = layout(&page);
            page.set(pattern_control(pattern), 0, &mut state, &layout);
            assert_eq!(page.breath.pattern, pattern);
        }
    }

    #[test]
    fn every_effect_has_one_cell_in_exactly_one_row() {
        for pattern in EFFECTS {
            let lit = PATTERN_ROWS
                .into_iter()
                .filter(|&row| pattern_cell(pattern, row) < row.len() as u8)
                .count();
            assert_eq!(lit, 1, "{pattern:?}");
        }
        assert_eq!(PATTERN_ROWS.len() * PATTERN_ROWS[0].len(), EFFECTS.len());
    }

    #[test]
    fn every_pattern_label_fits_its_cell() {
        let metrics = iot_core::ui::font::Metrics::at(iot_core::ui::font::Metrics::ROOT as u16);
        let cell = (CONTENT_W / PATTERN_ROWS[0].len() as i16) as usize;
        for lang in [Lang::En, Lang::ZhHans] {
            for pattern in EFFECTS {
                let label = pattern_msg(pattern).bytes(lang);
                assert!(
                    iot_core::ui::typeset::max_content(label, metrics) <= cell,
                    "{pattern:?} {lang:?} label overflows its cell"
                );
            }
        }
    }

    #[test]
    fn a_steady_glow_ignores_the_period_slider() {
        let mut page = LightingPage::default();
        let mut state = DeviceState::default();
        let layout = layout(&page);
        page.set(LightingControl::PatternSteady, 0, &mut state, &layout);
        let period = page.breath.period_ms;
        page.set(LightingControl::Period, 0, &mut state, &layout);
        assert_eq!(page.breath.period_ms, period);

        page.set(LightingControl::PatternBreath, 0, &mut state, &layout);
        page.set(LightingControl::Period, 0, &mut state, &layout);
        assert_eq!(page.breath.period_ms, GlassBreath::FASTEST_MS);
    }
}
