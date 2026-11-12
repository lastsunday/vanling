//! The light surfaces as the render loop reads them: the breathing schedule, the behaviour a
//! page's snapshot carries, and the tables the lamp's verbs walk.

use iot_core::drivers::network::NetworkError;
use iot_core::paint::{GROUP_CAPACITY, Rgb};

use crate::shared::intent::LightAction;

/// A breathing schedule: brightness envelope plus the hue path it travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Breath {
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

    /// The color this breath paints at `elapsed_ms`.
    pub fn rgb(self, elapsed_ms: u32) -> Rgb {
        iot_core::paint::breathe(
            elapsed_ms,
            self.period_ms,
            self.hue_period_ms,
            self.hue_span,
            self.min_brightness,
            self.max_brightness,
            self.saturation,
            self.hue,
            self.group,
            self.group_len,
        )
    }

    /// Backlight duty following the same envelope, never below `floor_pct`.
    pub fn backlight(self, elapsed_ms: u32, floor_pct: u8) -> u8 {
        iot_core::paint::backlight_level(
            elapsed_ms,
            self.period_ms,
            self.min_brightness,
            self.max_brightness,
            floor_pct,
        )
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

/// The breathing periods a step walks, in milliseconds.
const BREATH_PERIODS_MS: [u32; 10] = [125, 250, 375, 500, 750, 1_000, 2_000, 3_000, 5_000, 8_000];

/// The solid-mode brightness ladder a step walks.
const SOLID_BRIGHTNESS_STEPS: [u8; 6] = [32, 64, 100, 140, 200, 255];

/// How far one brightness swipe shifts the envelope.
const BRIGHTNESS_ENVELOPE_STEP: u8 = 32;

/// One hue group a breathing light can walk.
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

/// The hue groups a colour step walks, head first: the default sunset, the seven-way
/// rainbow, one single-hue sweep, and the static group.
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
    pub const DEFAULT_MIN_BRIGHTNESS: u8 = 24;
    pub const DEFAULT_MAX_BRIGHTNESS: u8 = 80;
    pub const DEFAULT_SATURATION: u8 = 200;
    pub const DEFAULT_HUE: u8 = 0;
    /// Warm sunset: amber, golden, deep rose — low-blue palette favored by circadian
    /// research for a calm ambient breathing glow.
    pub const DEFAULT_GROUP: [u8; Breath::MAX_GROUP] = [16, 32, 3, 0, 0, 0, 0];
    pub const DEFAULT_GROUP_LEN: u8 = 3;

    /// Full-brightness entry point for solid mode; dimming is a long-press advance from
    /// here.
    pub const SOLID_DEFAULT_BRIGHTNESS: u8 = 255;

    /// Backlight floor for the breathing envelope: the panel never sinks below this percent
    /// while pixels are lit, so the trough stays a dim glow instead of a hard on/off blip.
    pub const BACKLIGHT_FLOOR_PCT: u8 = 12;

    /// Whether this state has anything for the action to move. `Off` answers only `Cycle`.
    pub(crate) fn can(self, action: LightAction) -> bool {
        !matches!(self, Self::Off) || matches!(action, LightAction::Cycle)
    }

    /// The state the action lands on, or `None` when it has nothing to move.
    pub(crate) fn apply(self, action: LightAction) -> Option<Self> {
        match action {
            LightAction::Cycle => Some(cycle_mode(self)),
            LightAction::NextColor { forward } => advance_color(self, forward),
            LightAction::Step => advance_step(self),
            LightAction::Brightness { up } => swipe_brightness(self, up),
        }
    }

    pub const fn boot() -> Self {
        LightState::Breath(Breath {
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

    /// The setup access point is up and the device is waiting for a phone: a slow blue
    /// breath, the convention every provisioning firmware here shares.
    pub const SETUP: Self = Self::Breath(Breath {
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

    /// A credential is in hand and the radio is acting on it: the setup blue a step
    /// quicker, so the mode and the attempt are told apart without a second colour.
    pub const JOINING: Self = Self::Breath(Breath {
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

/// The one-shot news a lamp says instead of a page: a join that worked, a link that is not
/// there, or a credential the radio refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Joined,
    Offline,
    Refused,
}

impl NoticeKind {
    /// The news a failed attempt is: amber when the network could not be found, red when
    /// the radio refused what it was handed.
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

    /// What the lamp shows this many milliseconds into the announcement, or `None` once the
    /// news is over. The last flash ends on its dark half so the count matches the time.
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

/// A piece of one-shot news riding on a light-only board's lamp, with the step of it that
/// the clock last put up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Notice {
    pub kind: NoticeKind,
    /// Left `None` until the first frame that draws it.
    pub started_ms: Option<u32>,
    /// What the lamp shows at the current step; rewritten by the owner of the clock so
    /// reading the state never needs the time.
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
            let next = table_step(&SOLID_BRIGHTNESS_STEPS, brightness, 1);
            Some(LightState::Solid {
                color,
                brightness: next,
            })
        }
    }
}

fn swipe_brightness(current: LightState, up: bool) -> Option<LightState> {
    match current {
        LightState::Off => None,
        LightState::Breath(breath) => Some(LightState::Breath(shift_envelope(breath, up))),
        LightState::Solid { color, brightness } => {
            let next = table_step(&SOLID_BRIGHTNESS_STEPS, brightness, if up { 1 } else { -1 });
            Some(LightState::Solid {
                color,
                brightness: next,
            })
        }
    }
}

fn shift_envelope(breath: Breath, up: bool) -> Breath {
    let step = if up {
        i16::from(BRIGHTNESS_ENVELOPE_STEP)
    } else {
        -i16::from(BRIGHTNESS_ENVELOPE_STEP)
    };
    let guard = i16::from(BRIGHTNESS_ENVELOPE_STEP);
    let min = (i16::from(breath.min_brightness) + step).clamp(0, 255 - guard);
    let max = (i16::from(breath.max_brightness) + step).clamp(guard, 255);
    Breath {
        min_brightness: min as u8,
        max_brightness: max as u8,
        ..breath
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
    fn a_notice_starts_lit_and_unstamped() {
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

    const BREATH: LightState = LightState::boot();

    const EVERY_ACTION: [LightAction; 6] = [
        LightAction::Cycle,
        LightAction::NextColor { forward: true },
        LightAction::NextColor { forward: false },
        LightAction::Step,
        LightAction::Brightness { up: true },
        LightAction::Brightness { up: false },
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
            BREATH.apply(LightAction::NextColor { forward: true }),
            Some(LightState::Breath(COLOR_GROUPS[1].into_breath(breath)))
        );
        assert_eq!(
            BREATH.apply(LightAction::NextColor { forward: false }),
            Some(LightState::Breath(COLOR_GROUPS[3].into_breath(breath)))
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
        assert_eq!(solid(140).apply(LightAction::Step), Some(solid(200)));
        assert_eq!(solid(255).apply(LightAction::Step), Some(solid(32)));
    }

    #[test]
    fn brightness_shifts_the_envelope_in_breath_and_the_ladder_in_solid() {
        let breath = breathe();
        assert_eq!(
            BREATH.apply(LightAction::Brightness { up: true }),
            Some(LightState::Breath(Breath {
                min_brightness: breath.min_brightness + BRIGHTNESS_ENVELOPE_STEP,
                max_brightness: breath.max_brightness + BRIGHTNESS_ENVELOPE_STEP,
                ..breath
            }))
        );
        assert_eq!(
            BREATH.apply(LightAction::Brightness { up: false }),
            Some(LightState::Breath(Breath {
                min_brightness: 0,
                max_brightness: breath.max_brightness - BRIGHTNESS_ENVELOPE_STEP,
                ..breath
            }))
        );
        assert_eq!(
            solid(140).apply(LightAction::Brightness { up: true }),
            Some(solid(200))
        );
        assert_eq!(
            solid(140).apply(LightAction::Brightness { up: false }),
            Some(solid(100))
        );
        assert_eq!(
            solid(32).apply(LightAction::Brightness { up: false }),
            Some(solid(255))
        );
        assert_eq!(
            solid(255).apply(LightAction::Brightness { up: true }),
            Some(solid(32))
        );
    }

    #[test]
    fn the_envelope_keeps_its_span_at_both_walls() {
        let at_bottom = Breath {
            min_brightness: 8,
            max_brightness: 40,
            ..breathe()
        };
        let sunk = shift_envelope(at_bottom, false);
        assert_eq!(sunk.min_brightness, 0, "floor clamps at 0");
        assert_eq!(
            sunk.max_brightness, BRIGHTNESS_ENVELOPE_STEP,
            "ceiling rides the wall guard so the envelope keeps its span"
        );
        let at_top = Breath {
            min_brightness: 240,
            max_brightness: 255,
            ..breathe()
        };
        let lifted = shift_envelope(at_top, true);
        assert_eq!(
            lifted.min_brightness,
            255 - BRIGHTNESS_ENVELOPE_STEP,
            "floor parks at 255-guard so the envelope keeps its span"
        );
        assert_eq!(lifted.max_brightness, 255, "ceiling clamps at 255");
        let mut breath = breathe();
        for up in [true, false] {
            for _ in 0..32 {
                breath = shift_envelope(breath, up);
                assert!(
                    breath.max_brightness as u16 - breath.min_brightness as u16
                        >= u16::from(BRIGHTNESS_ENVELOPE_STEP),
                    "a shift keeps at least a full step of span"
                );
            }
        }
    }

    #[test]
    fn off_answers_only_cycle_and_can_agrees_with_apply() {
        assert!(LightState::Off.can(LightAction::Cycle));
        assert!(LightState::Off.apply(LightAction::Cycle).is_some());
        for state in [LightState::Off, solid(140), BREATH] {
            for action in EVERY_ACTION {
                assert_eq!(
                    state.can(action),
                    state.apply(action).is_some(),
                    "{state:?} {action:?}"
                );
            }
        }
        for action in EVERY_ACTION {
            if action != LightAction::Cycle {
                assert!(!LightState::Off.can(action), "{action:?}");
            }
        }
    }

    #[test]
    fn a_step_falls_back_to_head_when_current_is_not_in_the_table() {
        assert_eq!(
            table_step(&BREATH_PERIODS_MS, 9999, 1),
            BREATH_PERIODS_MS[1]
        );
        assert_eq!(table_step(&PALETTE, Rgb(0, 0, 0), 1), PALETTE[1]);
        assert_eq!(
            table_step(&SOLID_BRIGHTNESS_STEPS, 7, 1),
            SOLID_BRIGHTNESS_STEPS[1]
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
        assert_eq!(table_step(&SOLID_BRIGHTNESS_STEPS, 32, -1), 255);
        assert_eq!(table_step(&SOLID_BRIGHTNESS_STEPS, 140, -1), 100);
        assert_eq!(table_step(&SOLID_BRIGHTNESS_STEPS, 7, -1), 255);
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
            surface_roles: [SurfaceRole::Pages, SurfaceRole::Absent],
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
            surface_roles: [SurfaceRole::Pages, SurfaceRole::Lamp],
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
            "the first frame stamps the announcement"
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
        assert_eq!(manager.state().page(), Page::Splash);
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
        assert_eq!(manager.state().page(), Page::Ambient);
    }

    #[test]
    fn a_board_with_a_display_and_a_lamp_still_announces_on_the_lamp() {
        let mut manager = DeviceManager::boot(Boot {
            backed: Backed {
                network: true,
                ..Backed::NONE
            },
            surface_roles: [SurfaceRole::Pages, SurfaceRole::Lamp],
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
        assert_eq!(manager.state().page(), Page::Network);
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
