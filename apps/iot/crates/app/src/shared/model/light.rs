//! The light surfaces as the render loop reads them: breathing, page behavior, and the verb tables.

use iot_core::drivers::network::NetworkError;
use iot_core::light::{Effect, EffectParams, GROUP_CAPACITY, Rgb, Sample};

use crate::shared::intent::LightAction;

pub use iot_core::light::Effect as GlowPattern;

/// A breathing schedule: brightness envelope plus the hue path it travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Breath {
    pub effect: Effect,
    pub period_ms: u32,
    pub hue_period_ms: u32,
    /// Hue sweep width; `0` means a static standalone color.
    pub hue_span: u8,
    /// Trough value of the brightness envelope.
    pub min_brightness: u8,
    pub max_brightness: u8,
    pub saturation: u8,
    /// Base hue the sweep starts from (or holds when `hue_span == 0`).
    pub hue: u8,
    /// Color waypoints on the 256-step hue wheel.
    pub group: [u8; Self::MAX_GROUP],
    pub group_len: u8,
}

impl Breath {
    pub const MAX_GROUP: usize = GROUP_CAPACITY;

    /// The color this breath paints at `elapsed_ms`, and how far into its envelope that is.
    pub fn sample(self, elapsed_ms: u32) -> Sample {
        EffectParams {
            period_ms: self.period_ms,
            hue_period_ms: self.hue_period_ms,
            hue_span: self.hue_span,
            min_brightness: self.min_brightness,
            max_brightness: self.max_brightness,
            saturation: self.saturation,
            hue: self.hue,
            group: self.group,
            group_len: self.group_len,
        }
        .sample(self.effect, elapsed_ms)
    }
}

/// Where a breathing glass may appear: nowhere, only on the lighting page, or on every page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlowScope {
    Off,
    #[default]
    Page,
    Everywhere,
}

/// A breath drawn on the glass: the lamp's envelope with its own colour and pace. A page or an
/// alarm raises one; every other page holds the glass still.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlassBreath {
    pub period_ms: u16,
    pub hue: u8,
    pub saturation: u8,
    pub min_value: u8,
    pub max_value: u8,
    pub pattern: GlowPattern,
}

impl GlassBreath {
    /// A slow warm glow: the demo's default and a calm ambient cue.
    pub const CALM: Self = Self {
        period_ms: 3_000,
        hue: 28,
        saturation: 160,
        min_value: 32,
        max_value: 255,
        pattern: GlowPattern::Breath,
    };

    /// A fast red pulse for an alarm.
    pub const ALERT: Self = Self {
        period_ms: 800,
        hue: 0,
        saturation: 255,
        min_value: 10,
        max_value: 255,
        pattern: GlowPattern::Breath,
    };

    pub const FASTEST_MS: u16 = 500;
    pub const SLOWEST_MS: u16 = 5_000;

    /// The colour at `now_ms`. `Steady` and `Cycle` hold the peak; `Cycle` walks the hue once
    /// per period; `Aurora` drifts through four waypoints while it breathes. Flashing effects
    /// drop their floor to black so the gaps read as dark.
    pub fn at(self, now_ms: u32) -> Sample {
        let peak = self.max_value;
        let (min, hue_period_ms, group, group_len) = match self.pattern {
            GlowPattern::Steady | GlowPattern::Cycle => (peak, 0, [0; GROUP_CAPACITY], 0),
            GlowPattern::Aurora => (
                self.min_value,
                u32::from(self.period_ms) * 4,
                [0; GROUP_CAPACITY],
                0,
            ),
            GlowPattern::Lightning
            | GlowPattern::Sparkle
            | GlowPattern::Strobe
            | GlowPattern::Sos => (0, 0, [0; GROUP_CAPACITY], 0),
            GlowPattern::Police => (peak, 0, [0, 170, 0, 0, 0, 0, 0], 2),
            GlowPattern::Breath
            | GlowPattern::Candle
            | GlowPattern::Pulse
            | GlowPattern::Heartbeat => (self.min_value, 0, [0; GROUP_CAPACITY], 0),
        };
        EffectParams {
            period_ms: u32::from(self.period_ms),
            hue_period_ms,
            hue_span: 0,
            min_brightness: min,
            max_brightness: peak,
            saturation: self.saturation,
            hue: self.hue,
            group,
            group_len,
        }
        .sample(self.pattern, now_ms)
    }
}

/// The panel backlight as a setting: `pct` is both the slider position and the level the surface
/// is asked for. Zero is dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backlight {
    pct: u8,
}

impl Backlight {
    pub const FULL: Self = Self { pct: 100 };

    pub const fn pct(self) -> u8 {
        self.pct
    }

    pub const fn level(self) -> u8 {
        self.pct
    }

    pub fn set_pct(&mut self, pct: u8) {
        self.pct = pct.min(100);
    }
}

impl Default for Backlight {
    fn default() -> Self {
        Self::FULL
    }
}

/// The solid-mode palette, in the order a step walks it.
pub const PALETTE: [Rgb; 7] = [
    Rgb(255, 64, 48),
    Rgb(255, 160, 48),
    Rgb(255, 224, 64),
    Rgb(80, 200, 96),
    Rgb(32, 200, 220),
    Rgb(72, 128, 255),
    Rgb(190, 96, 255),
];

/// The time-based effects a single click walks, in the order the page lists them.
pub const EFFECTS: [Effect; 12] = [
    Effect::Steady,
    Effect::Breath,
    Effect::Cycle,
    Effect::Aurora,
    Effect::Candle,
    Effect::Pulse,
    Effect::Heartbeat,
    Effect::Lightning,
    Effect::Sparkle,
    Effect::Strobe,
    Effect::Police,
    Effect::Sos,
];

/// The breathing periods a step walks, in milliseconds.
const BREATH_PERIODS_MS: [u32; 10] = [125, 250, 375, 500, 750, 1_000, 2_000, 3_000, 5_000, 8_000];

/// The brightness ladder a step walks: 30% up to full, so the first press is never blinding.
const BRIGHTNESS_STEPS: [u8; 5] = [77, 128, 179, 230, 255];

/// The dimmest a breathing envelope goes, as a share of its peak.
pub const PEAK_FLOOR: u8 = 32;

pub const fn trough(peak: u8) -> u8 {
    (peak as u16 * PEAK_FLOOR as u16 / 255) as u8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorGroup {
    pub hue_span: u8,
    pub group: [u8; GROUP_CAPACITY],
    pub group_len: u8,
}

impl ColorGroup {
    pub fn into_breath(self, base: Breath) -> Breath {
        Breath {
            hue_span: self.hue_span,
            group: self.group,
            group_len: self.group_len,
            ..base
        }
    }
}

/// The hue groups a colour step walks, head first: sunset, rainbow, one hue sweep, and static.
pub const COLOR_GROUPS: [ColorGroup; 4] = [
    ColorGroup {
        hue_span: 255,
        group: [16, 32, 3, 0, 0, 0, 0],
        group_len: 3,
    },
    ColorGroup {
        hue_span: 255,
        group: [0, 21, 43, 85, 128, 170, 213],
        group_len: 7,
    },
    ColorGroup {
        hue_span: 255,
        group: [28, 0, 0, 0, 0, 0, 0],
        group_len: 1,
    },
    ColorGroup {
        hue_span: 255,
        group: [0, 0, 0, 0, 0, 0, 0],
        group_len: 0,
    },
];

/// Light behavior, read by the render loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightState {
    Off,
    Solid { color: Rgb, brightness: u8 },
    Breath(Breath),
}

pub struct LampLine(pub Option<LightState>);

impl core::fmt::Display for LampLine {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            None => f.write_str("[LAMP] the page's own light"),
            Some(LightState::Off) => f.write_str("[LAMP] dark"),
            Some(LightState::Solid { color, brightness }) => write!(
                f,
                "[LAMP] solid rgb({}, {}, {}), brightness {brightness} of 255",
                color.0, color.1, color.2
            ),
            Some(LightState::Breath(breath)) => write!(
                f,
                "[LAMP] breathing over {} ms, brightness {}..{}",
                breath.period_ms, breath.min_brightness, breath.max_brightness
            ),
        }
    }
}

impl LightState {
    pub const DEFAULT_PERIOD_MS: u32 = 3_000;
    pub const DEFAULT_HUE_PERIOD_MS: u32 = 60_000;
    pub const DEFAULT_HUE_SPAN: u8 = 255;
    pub const DEFAULT_MIN_BRIGHTNESS: u8 = trough(Self::DEFAULT_MAX_BRIGHTNESS);
    pub const DEFAULT_MAX_BRIGHTNESS: u8 = 77;
    pub const DEFAULT_SATURATION: u8 = 200;
    pub const DEFAULT_HUE: u8 = 0;
    /// The walking group: the lamp's default aurora and breath colours.
    pub const DEFAULT_GROUP: [u8; Breath::MAX_GROUP] = [0, 21, 43, 85, 128, 170, 213];
    pub const DEFAULT_GROUP_LEN: u8 = 7;

    /// Full-brightness entry point for solid mode; dimming is a long-press advance from here.
    pub const SOLID_DEFAULT_BRIGHTNESS: u8 = 255;

    /// The panel's dim cap while the display demand is on-change.
    pub const ON_DEMAND_BACKLIGHT_PCT: u8 = 12;

    /// Whether this state has anything for the action to move.
    pub(crate) fn can(self, action: LightAction) -> bool {
        self.apply(action).is_some()
    }

    /// The state the action lands on, or `None` when it has nothing to move.
    pub(crate) fn apply(self, action: LightAction) -> Option<Self> {
        match action {
            LightAction::Cycle => Some(cycle_mode(self)),
            LightAction::NextColor { forward } => advance_color(self, forward),
            LightAction::Step => advance_step(self),
            LightAction::Brightness { up } => swipe_brightness(self, up),
            LightAction::NextEffect => advance_effect(self),
            LightAction::TogglePower => Some(toggle_power(self)),
        }
    }

    pub const fn boot() -> Self {
        LightState::Breath(Breath {
            effect: Effect::Aurora,
            period_ms: LightState::DEFAULT_PERIOD_MS,
            hue_period_ms: LightState::DEFAULT_HUE_PERIOD_MS,
            hue_span: LightState::DEFAULT_HUE_SPAN,
            min_brightness: LightState::DEFAULT_MIN_BRIGHTNESS,
            max_brightness: LightState::DEFAULT_MAX_BRIGHTNESS,
            saturation: LightState::DEFAULT_SATURATION,
            hue: LightState::DEFAULT_HUE,
            group: LightState::DEFAULT_GROUP,
            group_len: LightState::DEFAULT_GROUP_LEN,
        })
    }

    /// The setup access point is up, waiting for a phone: a slow blue breath.
    pub const SETUP: Self = Self::Breath(Breath {
        effect: Effect::Breath,
        period_ms: 2_000,
        hue_period_ms: 0,
        hue_span: 0,
        min_brightness: 8,
        max_brightness: 48,
        saturation: 255,
        hue: 170,
        group: [0; Breath::MAX_GROUP],
        group_len: 0,
    });

    /// A credential is in hand and the radio is acting on it: the setup blue, a step quicker.
    pub const JOINING: Self = Self::Breath(Breath {
        effect: Effect::Breath,
        period_ms: 800,
        hue_period_ms: 0,
        hue_span: 0,
        min_brightness: 16,
        max_brightness: 64,
        saturation: 255,
        hue: 170,
        group: [0; Breath::MAX_GROUP],
        group_len: 0,
    });
}

const JOINED_FLASH_MS: u32 = 100;
const OFFLINE_FLASH_MS: u32 = 160;
const REFUSED_FLASH_MS: u32 = 120;

const FLASHES: u32 = 3;

/// The one-shot news a lamp says instead of a page: joined, offline, or refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Joined,
    Offline,
    Refused,
}

impl NoticeKind {
    /// The news a failed attempt is: amber when not found, red when the radio refused.
    pub const fn for_failure(kind: NetworkError) -> Self {
        match kind {
            NetworkError::NotFound => Self::Offline,
            NetworkError::Busy
            | NetworkError::InvalidArguments
            | NetworkError::NoMemory
            | NetworkError::Credentials
            | NetworkError::Driver => Self::Refused,
        }
    }

    /// The colour this news shows while its flash is lit.
    pub const fn color(self) -> LightState {
        match self {
            Self::Joined => LightState::Solid {
                color: Rgb(0, 255, 0),
                brightness: 64,
            },
            Self::Offline => LightState::Solid {
                color: Rgb(255, 155, 0),
                brightness: 96,
            },
            Self::Refused => LightState::Solid {
                color: Rgb(255, 0, 0),
                brightness: 96,
            },
        }
    }

    /// How long each half of one flash lasts.
    const fn flash_ms(self) -> u32 {
        match self {
            Self::Joined => JOINED_FLASH_MS,
            Self::Offline => OFFLINE_FLASH_MS,
            Self::Refused => REFUSED_FLASH_MS,
        }
    }

    /// What the lamp shows at `elapsed_ms`, or `None` once the news is over.
    pub const fn light_at(self, elapsed_ms: u32) -> Option<LightState> {
        let flash_ms = self.flash_ms();
        let step = flash_ms * 2;
        let flash = elapsed_ms / step;
        if flash >= FLASHES {
            return None;
        }
        if flash + 1 == FLASHES && elapsed_ms % step >= flash_ms {
            return None;
        }
        if elapsed_ms % step < flash_ms {
            Some(self.color())
        } else {
            Some(LightState::Off)
        }
    }
}

/// One-shot news on a light-only board's lamp, with the step the clock last put up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Notice {
    pub kind: NoticeKind,
    /// Left `None` until the first frame that draws it.
    pub started_ms: Option<u32>,
    /// What the lamp shows at the current step; the clock's owner rewrites it.
    pub light: LightState,
}

impl Notice {
    pub const fn new(kind: NoticeKind) -> Self {
        Self {
            kind,
            started_ms: None,
            light: kind.color(),
        }
    }

    /// The step this notice is on at `now_ms`, or `None` once its flashes have run out.
    pub fn tick(mut self, now_ms: u32) -> Option<Self> {
        let started = *self.started_ms.get_or_insert(now_ms);
        self.light = self.kind.light_at(now_ms.wrapping_sub(started))?;
        Some(self)
    }
}

fn toggle_power(current: LightState) -> LightState {
    match current {
        LightState::Off => LightState::boot(),
        _ => LightState::Off,
    }
}

fn advance_effect(current: LightState) -> Option<LightState> {
    match current {
        // The list's end is the lamp's off stop, so one press can also put it out.
        LightState::Off => Some(lit()),
        LightState::Solid { .. } => Some(LightState::boot()),
        LightState::Breath(breath) if breath.effect == EFFECTS[EFFECTS.len() - 1] => {
            Some(LightState::Off)
        }
        LightState::Breath(breath) => {
            let effect = next_effect(breath.effect);
            Some(LightState::Breath(tone(effect, breath)))
        }
    }
}

/// The lamp's signature tone per effect: each effect reads as itself instead of one ambient
/// hue. Steady, Breath, Aurora and Cycle keep the walking group; the rest name their colour.
fn tone(effect: Effect, base: Breath) -> Breath {
    let (hue, saturation, group, group_len) = match effect {
        Effect::Steady | Effect::Breath | Effect::Aurora | Effect::Cycle => (
            LightState::DEFAULT_HUE,
            LightState::DEFAULT_SATURATION,
            LightState::DEFAULT_GROUP,
            LightState::DEFAULT_GROUP_LEN,
        ),
        Effect::Candle => (28, 200, [0; Breath::MAX_GROUP], 0),
        Effect::Pulse => (128, 255, [0; Breath::MAX_GROUP], 0),
        Effect::Heartbeat => (0, 255, [0; Breath::MAX_GROUP], 0),
        Effect::Lightning | Effect::Sparkle | Effect::Strobe => {
            (190, 40, [0; Breath::MAX_GROUP], 0)
        }
        Effect::Sos => (190, 0, [0; Breath::MAX_GROUP], 0),
        Effect::Police => (0, 255, [0, 170, 0, 0, 0, 0, 0], 2),
    };
    Breath {
        effect,
        hue,
        saturation,
        group,
        group_len,
        ..base
    }
}

/// The effect after `effect`, wrapping at the list's end.
pub fn next_effect(effect: Effect) -> Effect {
    let index = EFFECTS.iter().position(|&e| e == effect).unwrap_or(0);
    EFFECTS[(index + 1) % EFFECTS.len()]
}

/// The first effect of the list, over the boot envelope.
fn lit() -> LightState {
    let LightState::Breath(breath) = LightState::boot() else {
        unreachable!()
    };
    LightState::Breath(tone(EFFECTS[0], breath))
}

fn cycle_mode(current: LightState) -> LightState {
    match current {
        LightState::Off => LightState::default(),
        LightState::Breath { .. } => LightState::Solid {
            color: PALETTE[0],
            brightness: LightState::SOLID_DEFAULT_BRIGHTNESS,
        },
        LightState::Solid { .. } => LightState::Off,
    }
}

fn advance_color(current: LightState, forward: bool) -> Option<LightState> {
    match current {
        LightState::Off => None,
        LightState::Breath(breath) => {
            let next_group = color_group_step(&breath, forward);
            Some(LightState::Breath(next_group.into_breath(breath)))
        }
        LightState::Solid { color, brightness } => {
            let next_color = table_step(&PALETTE, color, if forward { 1 } else { -1 });
            Some(LightState::Solid {
                color: next_color,
                brightness,
            })
        }
    }
}

fn advance_step(current: LightState) -> Option<LightState> {
    match current {
        LightState::Off => None,
        LightState::Breath(breath) => {
            let next_period = table_step(&BREATH_PERIODS_MS, breath.period_ms, 1);
            Some(LightState::Breath(Breath {
                period_ms: next_period,
                ..breath
            }))
        }
        LightState::Solid { color, brightness } => {
            let next = brightness_step(brightness, true);
            Some(LightState::Solid {
                color,
                brightness: next,
            })
        }
    }
}

/// The brightness dial: one step up or down, with the dark stop past full. A dark lamp answers
/// an upward step by lighting at the ladder's foot.
fn swipe_brightness(current: LightState, up: bool) -> Option<LightState> {
    match current {
        LightState::Off => up.then(lit),
        LightState::Breath(breath) => {
            if up && breath.max_brightness >= BRIGHTNESS_STEPS[BRIGHTNESS_STEPS.len() - 1] {
                return Some(LightState::Off);
            }
            let peak = brightness_step(breath.max_brightness, up);
            Some(LightState::Breath(Breath {
                min_brightness: trough(peak),
                max_brightness: peak,
                ..breath
            }))
        }
        LightState::Solid { color, brightness } => {
            if up && brightness >= BRIGHTNESS_STEPS[BRIGHTNESS_STEPS.len() - 1] {
                return Some(LightState::Off);
            }
            Some(LightState::Solid {
                color,
                brightness: brightness_step(brightness, up),
            })
        }
    }
}

/// The next ladder stop above or below `current`, wrapping at both ends.
fn brightness_step(current: u8, up: bool) -> u8 {
    if up {
        BRIGHTNESS_STEPS
            .iter()
            .copied()
            .find(|&step| step > current)
            .unwrap_or(BRIGHTNESS_STEPS[0])
    } else {
        BRIGHTNESS_STEPS
            .iter()
            .rev()
            .copied()
            .find(|&step| step < current)
            .unwrap_or(BRIGHTNESS_STEPS[BRIGHTNESS_STEPS.len() - 1])
    }
}

fn table_step<T: Copy + PartialEq>(table: &[T], current: T, dir: isize) -> T {
    let idx = table.iter().position(|&v| v == current).unwrap_or(0);
    let len = table.len() as isize;
    table[((idx as isize + dir).rem_euclid(len)) as usize]
}

fn color_group_idx(breath: &Breath) -> usize {
    COLOR_GROUPS
        .iter()
        .position(|c| c.group == breath.group && c.group_len == breath.group_len)
        .unwrap_or(0)
}

fn color_group_step(breath: &Breath, forward: bool) -> ColorGroup {
    let idx = color_group_idx(breath);
    let len = COLOR_GROUPS.len();
    COLOR_GROUPS[if forward {
        (idx + 1) % len
    } else {
        (idx + len - 1) % len
    }]
}

impl Default for LightState {
    fn default() -> Self {
        LightState::boot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glass_breath_rides_its_own_envelope_and_holds_one_hue() {
        for breath in [GlassBreath::CALM, GlassBreath::ALERT] {
            let period = u32::from(breath.period_ms);
            assert_eq!(breath.at(0).phase, 0, "the period starts at the trough");
            assert_eq!(breath.at(period / 2).phase, 255, "and peaks halfway");
            assert_eq!(breath.at(period / 2).color, breath.at(period * 3 / 2).color);
            assert_eq!(
                iot_core::light::brightness_of(breath.at(period / 2).color),
                breath.max_value
            );
        }
    }

    #[test]
    fn a_steady_glow_holds_one_colour_at_the_peak() {
        let mut breath = GlassBreath::CALM;
        breath.pattern = GlowPattern::Steady;
        let first = breath.at(0);
        let later = breath.at(u32::from(breath.period_ms) * 3 / 2);
        assert_eq!(first.color, later.color);
        assert_eq!(
            iot_core::light::brightness_of(first.color),
            breath.max_value
        );
    }

    #[test]
    fn a_cycle_holds_brightness_and_turns_the_hue() {
        let mut breath = GlassBreath::CALM;
        breath.pattern = GlowPattern::Cycle;
        breath.hue = 0;
        let first = breath.at(0);
        let quarter = breath.at(u32::from(breath.period_ms) / 4);
        assert_eq!(
            iot_core::light::brightness_of(first.color),
            breath.max_value
        );
        assert_eq!(
            iot_core::light::brightness_of(quarter.color),
            breath.max_value
        );
        assert_ne!(first.color, quarter.color);
    }

    #[test]
    fn aurora_breathes_while_it_drifts_through_its_waypoints() {
        let mut breath = GlassBreath::CALM;
        breath.pattern = GlowPattern::Aurora;
        let period = u32::from(breath.period_ms);
        assert_eq!(
            breath.at(0).phase,
            0,
            "the breath still starts at the trough"
        );
        assert_eq!(breath.at(period / 2).phase, 255, "and peaks halfway");
        assert_ne!(
            breath.at(0).color,
            breath.at(period).color,
            "one period advances the hue to the next waypoint"
        );
    }

    #[test]
    fn every_pattern_is_deterministic_and_stays_in_its_envelope() {
        for pattern in EFFECTS {
            let mut breath = GlassBreath::CALM;
            breath.pattern = pattern;
            let period = u32::from(breath.period_ms);
            let floor = match pattern {
                GlowPattern::Lightning
                | GlowPattern::Sparkle
                | GlowPattern::Strobe
                | GlowPattern::Sos => 0,
                GlowPattern::Steady | GlowPattern::Cycle | GlowPattern::Police => breath.max_value,
                _ => breath.min_value,
            };
            for t in [0, 13, period / 3, period / 2, period] {
                let first = breath.at(t);
                assert_eq!(first, breath.at(t), "{pattern:?} at {t} is deterministic");
                let value = iot_core::light::brightness_of(first.color);
                assert!(
                    value >= floor && value <= breath.max_value,
                    "{pattern:?} at {t}: {value} outside {floor}..={}",
                    breath.max_value
                );
            }
        }
    }

    #[test]
    fn the_lamp_boots_on_aurora() {
        let LightState::Breath(breath) = LightState::boot() else {
            panic!("boot is breathing");
        };
        assert_eq!(breath.effect, GlowPattern::Aurora);
    }

    #[test]
    fn a_backlight_setting_reaches_dark() {
        let mut backlight = Backlight::default();
        assert_eq!(backlight.level(), 100);
        backlight.set_pct(0);
        assert_eq!(backlight.pct(), 0);
        assert_eq!(backlight.level(), 0);
    }

    #[test]
    fn the_status_modes_differ_in_pace_but_share_the_setup_blue() {
        let states = [LightState::SETUP, LightState::JOINING, LightState::boot()];
        for (index, first) in states.iter().enumerate() {
            for second in &states[index + 1..] {
                assert_ne!(first, second);
            }
        }
        for state in [LightState::SETUP, LightState::JOINING] {
            let LightState::Breath(breath) = state else {
                panic!("{state:?} is not a breath");
            };
            assert_eq!(breath.hue, 170, "{state:?}");
            assert_eq!(breath.hue_span, 0, "{state:?} must hold one hue");
            assert_eq!(breath.group_len, 0, "{state:?} must hold one hue");
            assert!(breath.min_brightness < breath.max_brightness, "{state:?}");
        }
        let LightState::Breath(setup) = LightState::SETUP else {
            unreachable!();
        };
        let LightState::Breath(joining) = LightState::JOINING else {
            unreachable!();
        };
        assert!(joining.period_ms < setup.period_ms);
    }

    fn flashes(kind: NoticeKind, flash_ms: u32, lit: LightState) {
        let step = flash_ms * 2;
        let dark = Some(LightState::Off);
        assert_eq!(kind.light_at(0), Some(lit));
        assert_eq!(kind.light_at(flash_ms - 1), Some(lit));
        assert_eq!(kind.light_at(flash_ms), dark);
        assert_eq!(kind.light_at(step - 1), dark);
        assert_eq!(kind.light_at(step), Some(lit));
        assert_eq!(kind.light_at((FLASHES - 1) * step), Some(lit));
        assert_eq!(
            kind.light_at((FLASHES - 1) * step + flash_ms - 1),
            Some(lit)
        );
        assert_eq!(kind.light_at((FLASHES - 1) * step + flash_ms), None);
        assert_eq!(kind.light_at(FLASHES * step), None);
    }

    #[test]
    fn a_join_that_worked_is_three_green_flashes_and_then_over() {
        flashes(
            NoticeKind::Joined,
            JOINED_FLASH_MS,
            LightState::Solid {
                color: Rgb(0, 255, 0),
                brightness: 64,
            },
        );
    }

    #[test]
    fn an_offline_link_is_three_amber_flashes_and_then_over() {
        flashes(
            NoticeKind::Offline,
            OFFLINE_FLASH_MS,
            LightState::Solid {
                color: Rgb(255, 155, 0),
                brightness: 96,
            },
        );
    }

    #[test]
    fn a_refusal_is_three_red_flashes_and_then_over() {
        flashes(
            NoticeKind::Refused,
            REFUSED_FLASH_MS,
            LightState::Solid {
                color: Rgb(255, 0, 0),
                brightness: 96,
            },
        );
    }

    #[test]
    fn a_network_that_is_not_there_is_amber_and_a_refusal_is_red() {
        assert_eq!(
            NoticeKind::for_failure(NetworkError::NotFound),
            NoticeKind::Offline
        );
        for kind in [
            NetworkError::Busy,
            NetworkError::InvalidArguments,
            NetworkError::NoMemory,
            NetworkError::Credentials,
            NetworkError::Driver,
        ] {
            assert_eq!(
                NoticeKind::for_failure(kind),
                NoticeKind::Refused,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn the_lamp_line_says_what_the_board_was_handed() {
        let mut line = alloc::string::String::new();
        use core::fmt::Write;
        for (lamp, expected) in [
            (None, "[LAMP] the page's own light"),
            (Some(LightState::Off), "[LAMP] dark"),
            (
                Some(LightState::Solid {
                    color: Rgb(1, 2, 3),
                    brightness: 42,
                }),
                "[LAMP] solid rgb(1, 2, 3), brightness 42 of 255",
            ),
            (
                Some(LightState::Breath(Breath {
                    effect: Effect::Breath,
                    period_ms: 1_000,
                    hue_period_ms: 0,
                    hue_span: 0,
                    min_brightness: 10,
                    max_brightness: 200,
                    saturation: 255,
                    hue: 0,
                    group: [0; Breath::MAX_GROUP],
                    group_len: 1,
                })),
                "[LAMP] breathing over 1000 ms, brightness 10..200",
            ),
        ] {
            line.clear();
            write!(&mut line, "{}", LampLine(lamp)).expect("a string takes every line");
            assert_eq!(line, expected);
        }
    }

    #[test]
    fn a_notice_starts_lit_with_no_start_time() {
        for kind in [NoticeKind::Joined, NoticeKind::Offline, NoticeKind::Refused] {
            let notice = Notice::new(kind);
            assert_eq!(notice.started_ms, None);
            assert_eq!(Some(notice.light), kind.light_at(0));
        }
    }
}

#[cfg(test)]
mod actions {
    use super::*;
    use iot_core::light::hsv_to_rgb;

    const BREATH: LightState = LightState::boot();

    const EVERY_ACTION: [LightAction; 8] = [
        LightAction::Cycle,
        LightAction::NextColor { forward: true },
        LightAction::NextColor { forward: false },
        LightAction::Step,
        LightAction::Brightness { up: true },
        LightAction::Brightness { up: false },
        LightAction::NextEffect,
        LightAction::TogglePower,
    ];

    fn breathe() -> Breath {
        let LightState::Breath(breath) = BREATH else {
            unreachable!();
        };
        breath
    }

    fn solid(brightness: u8) -> LightState {
        LightState::Solid {
            color: PALETTE[0],
            brightness,
        }
    }

    #[test]
    fn cycle_walks_off_breath_solid_and_back() {
        let breathing = LightState::Off
            .apply(LightAction::Cycle)
            .expect("off cycles on");
        assert!(matches!(breathing, LightState::Breath(_)));
        let lit = breathing
            .apply(LightAction::Cycle)
            .expect("breath cycles to solid");
        assert_eq!(lit, solid(LightState::SOLID_DEFAULT_BRIGHTNESS));
        assert_eq!(lit.apply(LightAction::Cycle), Some(LightState::Off));
    }

    #[test]
    fn next_color_walks_the_group_in_breath_and_the_palette_in_solid() {
        let breath = breathe();
        assert_eq!(
            color_group_idx(&breath),
            1,
            "the boot group is the seven-colour walk"
        );
        assert_eq!(
            BREATH.apply(LightAction::NextColor { forward: true }),
            Some(LightState::Breath(COLOR_GROUPS[2].into_breath(breath)))
        );
        assert_eq!(
            BREATH.apply(LightAction::NextColor { forward: false }),
            Some(LightState::Breath(COLOR_GROUPS[0].into_breath(breath)))
        );
        assert_eq!(
            solid(140).apply(LightAction::NextColor { forward: true }),
            Some(LightState::Solid {
                color: PALETTE[1],
                brightness: 140,
            })
        );
        assert_eq!(
            solid(60).apply(LightAction::NextColor { forward: false }),
            Some(LightState::Solid {
                color: *PALETTE.last().unwrap(),
                brightness: 60,
            })
        );
    }

    #[test]
    fn step_walks_the_period_in_breath_and_the_ladder_in_solid() {
        let mut state = LightState::Breath(Breath {
            period_ms: BREATH_PERIODS_MS[0],
            ..breathe()
        });
        for i in 0..BREATH_PERIODS_MS.len() {
            let next = state
                .apply(LightAction::Step)
                .expect("a breath has a period");
            let LightState::Breath(next) = next else {
                panic!("step must stay in breathing");
            };
            assert_eq!(
                next.period_ms,
                BREATH_PERIODS_MS[(i + 1) % BREATH_PERIODS_MS.len()],
                "step {i}: wrong period reached"
            );
            state = LightState::Breath(next);
        }
        assert_eq!(solid(140).apply(LightAction::Step), Some(solid(179)));
        assert_eq!(solid(255).apply(LightAction::Step), Some(solid(77)));
    }

    #[test]
    fn brightness_walks_the_ladder_and_keeps_the_envelope_ratio() {
        let up = BREATH
            .apply(LightAction::Brightness { up: true })
            .expect("a breath has a peak");
        assert_eq!(
            up,
            LightState::Breath(Breath {
                min_brightness: trough(128),
                max_brightness: 128,
                ..breathe()
            })
        );
        let down = BREATH
            .apply(LightAction::Brightness { up: false })
            .expect("a breath has a peak");
        assert_eq!(
            down,
            LightState::Breath(Breath {
                min_brightness: trough(255),
                max_brightness: 255,
                ..breathe()
            }),
            "the lowest stop wraps to the brightest"
        );
        assert_eq!(
            solid(140).apply(LightAction::Brightness { up: true }),
            Some(solid(179))
        );
        assert_eq!(
            solid(140).apply(LightAction::Brightness { up: false }),
            Some(solid(128))
        );
        assert_eq!(
            solid(77).apply(LightAction::Brightness { up: false }),
            Some(solid(255))
        );
        assert_eq!(
            solid(255).apply(LightAction::Brightness { up: true }),
            Some(LightState::Off),
            "the dial's dark stop is past full"
        );
        assert_eq!(
            LightState::Off.apply(LightAction::Brightness { up: true }),
            Some(lit()),
            "a dark lamp answers the dial by lighting at its foot"
        );
        assert_eq!(
            LightState::Off.apply(LightAction::Brightness { up: false }),
            None
        );
    }

    #[test]
    fn the_ladder_wraps_at_both_ends_and_clicks_off_past_full() {
        assert_eq!(brightness_step(BRIGHTNESS_STEPS[0], false), 255);
        let mut state = BREATH;
        for _ in 0..BRIGHTNESS_STEPS.len() - 1 {
            state = state
                .apply(LightAction::Brightness { up: true })
                .expect("the dial climbs");
        }
        assert_eq!(
            state,
            LightState::Breath(Breath {
                min_brightness: trough(255),
                max_brightness: 255,
                ..breathe()
            })
        );
        assert_eq!(
            state.apply(LightAction::Brightness { up: true }),
            Some(LightState::Off)
        );
        for step in BRIGHTNESS_STEPS {
            assert!(trough(step) < step, "the trough stays below the peak");
        }
    }

    #[test]
    fn off_answers_only_the_actions_that_can_light_it() {
        for action in EVERY_ACTION {
            let lights_it = matches!(
                action,
                LightAction::Cycle
                    | LightAction::NextEffect
                    | LightAction::TogglePower
                    | LightAction::Brightness { up: true }
            );
            assert_eq!(LightState::Off.can(action), lights_it, "{action:?}");
        }
    }

    #[test]
    fn every_effect_carries_its_own_tone() {
        let base = breathe();
        let peak = base.max_brightness;
        let tone_of = |effect| {
            let breath = tone(effect, base);
            (breath.hue, breath.saturation, breath.group_len)
        };
        assert_eq!(tone_of(Effect::Candle), (28, 200, 0));
        assert_eq!(tone_of(Effect::Pulse), (128, 255, 0));
        assert_eq!(tone_of(Effect::Heartbeat), (0, 255, 0));
        for effect in [Effect::Lightning, Effect::Sparkle, Effect::Strobe] {
            assert_eq!(tone_of(effect), (190, 40, 0), "{effect:?}");
        }
        assert_eq!(tone_of(Effect::Sos), (190, 0, 0), "SOS is white");
        let police = tone(Effect::Police, base);
        assert_eq!(police.group_len, 2);
        assert_eq!(
            police.sample(0).color,
            hsv_to_rgb(0, 255, peak),
            "the police starts red"
        );
        assert_eq!(
            police.sample(base.period_ms / 2).color,
            hsv_to_rgb(170, 255, peak),
            "and flips to blue"
        );
        for effect in [Effect::Steady, Effect::Breath, Effect::Aurora] {
            let breath = tone(effect, base);
            assert_eq!(breath.group, LightState::DEFAULT_GROUP, "{effect:?}");
            assert_eq!(breath.group_len, LightState::DEFAULT_GROUP_LEN);
        }
    }

    #[test]
    fn next_effect_walks_the_list_and_stops_at_off() {
        let mut state = lit();
        for (i, effect) in EFFECTS.iter().enumerate().skip(1) {
            state = state
                .apply(LightAction::NextEffect)
                .expect("a breath turns");
            let LightState::Breath(breath) = state else {
                panic!("next effect stays in breathing");
            };
            assert_eq!(breath.effect, *effect, "step {i}");
        }
        assert_eq!(
            state.apply(LightAction::NextEffect),
            Some(LightState::Off),
            "the list's end is the off stop"
        );
        assert_eq!(
            LightState::Off.apply(LightAction::NextEffect),
            Some(lit()),
            "a press turns it back on at the head"
        );
        assert_eq!(
            solid(140).apply(LightAction::NextEffect),
            Some(LightState::boot()),
            "a solid lamp enters the default breath"
        );
        assert_eq!(lit().apply(LightAction::TogglePower), Some(LightState::Off));
        assert_eq!(
            LightState::Off.apply(LightAction::TogglePower),
            Some(LightState::boot())
        );
    }

    #[test]
    fn a_step_falls_back_to_head_when_current_is_not_in_the_table() {
        assert_eq!(
            table_step(&BREATH_PERIODS_MS, 9999, 1),
            BREATH_PERIODS_MS[1]
        );
        assert_eq!(table_step(&PALETTE, Rgb(0, 0, 0), 1), PALETTE[1]);
        assert_eq!(
            brightness_step(7, true),
            BRIGHTNESS_STEPS[0],
            "a peak below the ladder starts at its head"
        );
        let unknown = Breath {
            group: [200, 250, 30, 0, 0, 0, 0],
            group_len: 3,
            ..breathe()
        };
        assert_eq!(color_group_step(&unknown, true), COLOR_GROUPS[1]);
    }

    #[test]
    fn retreat_and_step_wrap_backwards() {
        let last = *PALETTE.last().unwrap();
        assert_eq!(table_step(&PALETTE, PALETTE[0], -1), last);
        assert_eq!(table_step(&PALETTE, last, 1), PALETTE[0]);
    }

    #[test]
    fn the_seven_color_preset_uses_every_hue_slot() {
        let expected: [u8; GROUP_CAPACITY] = [0, 21, 43, 85, 128, 170, 213];
        let rainbow = COLOR_GROUPS[1];
        assert_eq!(rainbow.group_len, expected.len() as u8);
        assert_eq!(rainbow.group, expected);
    }
}

#[cfg(test)]
mod lamp {
    use super::*;
    use crate::shared::intent::*;
    use crate::state::test_support::*;
    use iot_core::drivers::network::{LinkPhase, LinkStatus, SetupHost};

    fn lamp_only() -> DeviceManager {
        DeviceManager::boot(Boot {
            backed: Backed {
                network: true,
                ..Backed::NONE
            },
            surface_roles: [SurfaceRole::Lamp, SurfaceRole::Absent],
            ..Default::default()
        })
    }

    fn report(manager: &mut DeviceManager, status: LinkStatus) {
        manager.apply_business(BusinessIntent::Link(LinkIntent::Status(status)));
    }

    fn setup(manager: &mut DeviceManager) {
        report(
            manager,
            LinkStatus {
                mode: iot_core::drivers::network::NetworkMode::Setup {
                    host: SetupHost::AccessPoint,
                    cancellable: false,
                },
                ..LinkStatus::default()
            },
        );
    }

    fn connecting(manager: &mut DeviceManager, saved: bool) {
        report(
            manager,
            LinkStatus {
                phase: LinkPhase::Connecting,
                saved,
                ..LinkStatus::default()
            },
        );
    }

    fn connected(manager: &mut DeviceManager) {
        report(
            manager,
            LinkStatus {
                phase: LinkPhase::Connected,
                address: Some(Ipv4::from_octets([192, 168, 4, 2])),
                saved: true,
                ..LinkStatus::default()
            },
        );
    }

    fn failed(manager: &mut DeviceManager, kind: NetworkError, saved: bool) {
        report(
            manager,
            LinkStatus {
                phase: LinkPhase::Failed(kind),
                saved,
                ..LinkStatus::default()
            },
        );
    }

    fn drain(manager: &mut DeviceManager, budget_ms: u32) {
        manager.tick_notice(0);
        let mut elapsed = 0;
        while manager.notice_active() {
            manager.tick_notice(elapsed);
            elapsed += 20;
            assert!(elapsed < budget_ms, "the announcement never ended");
        }
    }

    const GREEN: LightState = LightState::Solid {
        color: Rgb(0, 255, 0),
        brightness: 64,
    };
    const AMBER: LightState = LightState::Solid {
        color: Rgb(255, 155, 0),
        brightness: 96,
    };
    const RED: LightState = LightState::Solid {
        color: Rgb(255, 0, 0),
        brightness: 96,
    };

    #[test]
    fn the_link_notice_belongs_to_a_board_with_a_radio_and_a_lamp() {
        let mut lamp = lamp_only();
        setup(&mut lamp);
        assert_eq!(lamp.state().link_notice(), Some(LightState::SETUP));

        let mut screened = DeviceManager::boot(Boot {
            backed: Backed {
                network: true,
                ..Backed::NONE
            },
            surface_roles: [SurfaceRole::Screen, SurfaceRole::Absent],
            ..Default::default()
        });
        setup(&mut screened);
        assert_eq!(
            screened.state().link_notice(),
            None,
            "a display has a page to say it, so nothing is routed to a lamp"
        );

        let mut both = DeviceManager::boot(Boot {
            backed: Backed {
                network: true,
                ..Backed::NONE
            },
            surface_roles: [SurfaceRole::Screen, SurfaceRole::Lamp],
            ..Default::default()
        });
        setup(&mut both);
        assert_eq!(
            both.state().link_notice(),
            Some(LightState::SETUP),
            "a lamp still speaks beside a display"
        );

        let mut no_radio = DeviceManager::boot(Boot {
            backed: Backed {
                network: false,
                ..Backed::NONE
            },
            surface_roles: [SurfaceRole::Lamp, SurfaceRole::Absent],
            ..Default::default()
        });
        setup(&mut no_radio);
        assert_eq!(no_radio.state().link_notice(), None);
    }

    #[test]
    fn the_lamp_tells_the_mode_from_the_attempt_and_from_the_resting_state() {
        let mut manager = lamp_only();
        setup(&mut manager);
        assert_eq!(manager.state().link_notice(), Some(LightState::SETUP));

        connecting(&mut manager, false);
        assert_eq!(manager.state().link_notice(), Some(LightState::JOINING));

        connecting(&mut manager, true);
        assert_eq!(manager.state().link_notice(), None);

        report(
            &mut manager,
            LinkStatus {
                phase: LinkPhase::Off,
                saved: true,
                ..LinkStatus::default()
            },
        );
        assert_eq!(manager.state().link_notice(), None);
    }

    #[test]
    fn a_join_that_worked_is_green_and_then_hands_the_lamp_back() {
        let mut manager = lamp_only();
        connecting(&mut manager, true);
        connected(&mut manager);
        assert_eq!(manager.state().link_notice(), Some(GREEN));

        manager.tick_notice(0);
        assert_eq!(
            manager.state().link_notice(),
            Some(GREEN),
            "the announcement is drawn before it is stepped away"
        );
        drain(&mut manager, 2_000);
        assert!(!manager.notice_active(), "the news is over");
        assert_eq!(
            manager.state().link_notice(),
            None,
            "a saved device that is connected has nothing left to announce"
        );
    }

    #[test]
    fn the_news_outranks_the_mode_it_arrived_in() {
        let mut manager = lamp_only();
        setup(&mut manager);
        connected(&mut manager);
        setup(&mut manager);
        assert_eq!(manager.state().link_notice(), Some(GREEN));
    }

    #[test]
    fn a_refusal_flashes_and_ends_and_the_same_phase_does_not_restart_it() {
        let mut manager = lamp_only();
        failed(&mut manager, NetworkError::Credentials, false);
        assert_eq!(
            manager.state().link_notice(),
            Some(RED),
            "a refusal starts lit"
        );
        manager.tick_notice(0);
        let announced = manager.state().notice;
        assert_eq!(
            announced.and_then(|notice| notice.started_ms),
            Some(0),
            "the first frame records the announcement"
        );

        failed(&mut manager, NetworkError::Credentials, false);
        assert_eq!(manager.state().notice, announced);

        drain(&mut manager, 2_000);
        assert_eq!(
            manager.state().link_notice(),
            None,
            "a refusal is an event, not the lamp's resting colour"
        );
    }

    #[test]
    fn a_boot_with_nothing_saved_flashes_amber_and_then_shows_the_page() {
        let mut manager = lamp_only();
        assert_eq!(manager.state().page(), PageId::Splash);
        manager.apply_business(BusinessIntent::Navigation(NavigationIntent::SplashOver));
        assert_eq!(
            manager.state().notice.map(|notice| notice.kind),
            Some(NoticeKind::Offline)
        );
        assert_eq!(manager.state().link_notice(), Some(AMBER));

        drain(&mut manager, 2_000);
        assert_eq!(
            manager.state().link_notice(),
            None,
            "the page owns the lamp again once the news is over"
        );
        assert_eq!(manager.state().page(), PageId::Headless);
    }

    #[test]
    fn a_board_with_a_display_and_a_lamp_still_announces_on_the_lamp() {
        let mut manager = DeviceManager::boot(Boot {
            backed: Backed {
                network: true,
                ..Backed::NONE
            },
            surface_roles: [SurfaceRole::Screen, SurfaceRole::Lamp],
            ..Default::default()
        });
        manager.apply_business(BusinessIntent::Navigation(NavigationIntent::SplashOver));
        assert_eq!(
            manager.state().notice.map(|notice| notice.kind),
            Some(NoticeKind::Offline)
        );
        assert_eq!(manager.state().link_notice(), Some(AMBER));
    }

    #[test]
    fn a_splash_hold_opens_setup_instead_of_flashing_offline() {
        let mut manager = lamp_only();
        manager.apply_business(BusinessIntent::Navigation(NavigationIntent::RequestSetup));
        manager.apply_business(BusinessIntent::Navigation(NavigationIntent::SplashOver));
        assert_eq!(manager.state().notice, None);
        assert_eq!(manager.state().page(), PageId::Network);
    }

    #[test]
    fn a_hold_through_the_splash_of_a_lamp_only_board_opens_provisioning() {
        let mut manager = lamp_only();
        assert_eq!(manager.state().page(), PageId::Splash);
        let hold = InputIntent {
            source: 0,
            event: InputEvent::Button(ButtonEvent::LongPress),
        };
        assert_eq!(
            translate(&hold, &manager.state(), &Services::default()),
            Some(BusinessIntent::Navigation(NavigationIntent::RequestSetup))
        );
        manager.apply_intent(Intent::Input(hold), &iot_core::ui::Layout::EMPTY);
        assert_eq!(
            manager.apply_business(BusinessIntent::Navigation(NavigationIntent::SplashOver)),
            Some(crate::shared::model::Command::Network(
                crate::shared::model::NetworkCommand::SplashOver { setup: true }
            ))
        );
        assert_eq!(manager.state().page(), PageId::Network);
    }

    #[test]
    fn a_missing_network_is_amber_and_a_refused_credential_is_red() {
        let mut missing = lamp_only();
        failed(&mut missing, NetworkError::NotFound, true);
        assert_eq!(
            missing.state().notice.map(|notice| notice.kind),
            Some(NoticeKind::Offline)
        );
        assert_eq!(missing.state().link_notice(), Some(AMBER));

        let mut refused = lamp_only();
        failed(&mut refused, NetworkError::Credentials, false);
        assert_eq!(
            refused.state().notice.map(|notice| notice.kind),
            Some(NoticeKind::Refused)
        );
        assert_eq!(refused.state().link_notice(), Some(RED));
    }

    #[test]
    fn a_retried_failure_is_announced_once_until_the_link_connects() {
        let mut manager = lamp_only();
        failed(&mut manager, NetworkError::NotFound, true);
        drain(&mut manager, 2_000);

        connecting(&mut manager, true);
        failed(&mut manager, NetworkError::NotFound, true);
        assert_eq!(manager.state().notice, None, "the retry is not new news");

        connected(&mut manager);
        assert_eq!(
            manager.state().notice.map(|notice| notice.kind),
            Some(NoticeKind::Joined)
        );
        drain(&mut manager, 2_000);
        failed(&mut manager, NetworkError::NotFound, true);
        assert_eq!(
            manager.state().notice.map(|notice| notice.kind),
            Some(NoticeKind::Offline)
        );
    }
}
