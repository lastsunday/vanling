//! The light surfaces as the render loop reads them: the breathing schedule and the
//! behavior a page's snapshot carries.
use super::*;

/// A breathing schedule: brightness envelope plus the hue path it travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Breath {
    pub period_ms: u32,
    pub hue_period_ms: u32,
    /// Hue sweep width; `0` means a static standalone color. Ignored when
    /// `group_len > 0`.
    pub hue_span: u8,
    /// Trough value of the brightness envelope. A nonzero floor keeps the
    /// breathing glow lit instead of fading fully to black.
    pub min_brightness: u8,
    pub max_brightness: u8,
    pub saturation: u8,
    /// Base hue the sweep starts from (or holds when `hue_span == 0`). Ignored
    /// when `group_len > 0`.
    pub hue: u8,
    /// Color waypoints on the 256-step hue wheel. `group_len` picks the
    /// trajectory: `0` = the `hue`/`hue_span` sweep above; `1` = a static
    /// standalone color, `group[0]`; `>=2` = rotate through the waypoints,
    /// walking each `group[i] -> group[i+1]` segment over
    /// `hue_period_ms / group_len` and wrapping `group[last] -> group[0]`.
    pub group: [u8; Self::MAX_GROUP],
    pub group_len: u8,
}

impl Breath {
    pub const MAX_GROUP: usize = GROUP_CAPACITY;
}

/// Light behavior, read by the render loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightState {
    Off,
    Solid { color: Rgb, brightness: u8 },
    Breath(Breath),
}

impl LightState {
    pub const fn boot() -> Self {
        LightState::Breath(Breath {
            period_ms: DeviceManager::DEFAULT_PERIOD_MS,
            hue_period_ms: DeviceManager::DEFAULT_HUE_PERIOD_MS,
            hue_span: DeviceManager::DEFAULT_HUE_SPAN,
            min_brightness: DeviceManager::DEFAULT_MIN_BRIGHTNESS,
            max_brightness: DeviceManager::DEFAULT_MAX_BRIGHTNESS,
            saturation: DeviceManager::DEFAULT_SATURATION,
            hue: DeviceManager::DEFAULT_HUE,
            group: DeviceManager::DEFAULT_GROUP,
            group_len: DeviceManager::DEFAULT_GROUP_LEN,
        })
    }

    /// The setup access point is up and the device is waiting for a phone: a slow blue
    /// breath, the convention every provisioning firmware here shares. A breath rather than a
    /// blink because waiting is not a fault, and slow and dim so it cannot be mistaken for the
    /// ambient glow's travelling rainbow.
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

    /// A credential is in hand and the radio is acting on it: the setup blue a step quicker, so
    /// the mode and the attempt are told apart without a second colour.
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

/// One green flash of a join that worked. Short: success is an event, not a mode, and the page
/// wants its surface back.
const JOINED_ON_MS: u32 = 100;
const JOINED_OFF_MS: u32 = 100;

/// One amber flash of "there is no connection": nothing saved, or the network is not in range.
/// The slowest of the three, because it is the one a person has to do something about.
const OFFLINE_ON_MS: u32 = 160;
const OFFLINE_OFF_MS: u32 = 160;

/// One red flash of a refusal, and the dark between two of them. Long enough that a person who
/// just put the phone down still sees one: the access point is already gone by the time this
/// runs, so the lamp is the only answer left.
const REFUSED_ON_MS: u32 = 120;
const REFUSED_OFF_MS: u32 = 120;

/// Flashes in one announcement. Three is what "a few" looks like, and the same count for all
/// three kinds keeps the number a property of the lamp rather than of the news.
const FLASHES: u32 = 3;

/// The one-shot news a light-only board's lamp says instead of a page: a join that worked, a
/// link that is not there, or a credential the radio refused. They are the same edges the
/// sounds ride.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    /// Three short green flashes.
    Joined,
    /// Three amber flashes: nothing to join, or the network is not in range.
    Offline,
    /// Three short red flashes.
    Refused,
}

impl NoticeKind {
    /// The news a failed attempt is: amber when the network could not be found, red when the
    /// radio refused what it was handed. `NotFound` is the one a person fixes by moving the
    /// device rather than by retyping a passphrase.
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

    /// What the lamp shows this many milliseconds into the announcement, or `None` once the
    /// news is over. The last flash ends on its own lit edge, so the page gets the surface back
    /// without a dark pause between the two.
    pub const fn light_at(self, elapsed_ms: u32) -> Option<LightState> {
        let (on_ms, off_ms, color) = match self {
            Self::Joined => (
                JOINED_ON_MS,
                JOINED_OFF_MS,
                LightState::Solid {
                    color: Rgb(0, 255, 0),
                    brightness: 64,
                },
            ),
            Self::Offline => (
                OFFLINE_ON_MS,
                OFFLINE_OFF_MS,
                LightState::Solid {
                    color: Rgb(255, 155, 0),
                    brightness: 96,
                },
            ),
            Self::Refused => (
                REFUSED_ON_MS,
                REFUSED_OFF_MS,
                LightState::Solid {
                    color: Rgb(255, 0, 0),
                    brightness: 96,
                },
            ),
        };
        let step = on_ms + off_ms;
        let flash = elapsed_ms / step;
        if flash >= FLASHES {
            return None;
        }
        if flash + 1 == FLASHES && elapsed_ms % step >= on_ms {
            return None;
        }
        if elapsed_ms % step < on_ms {
            Some(color)
        } else {
            Some(LightState::Off)
        }
    }
}

/// A piece of one-shot news riding on a light-only board's lamp, with the step of it that the
/// clock last put up.
///
/// The clock belongs to the caller that owns time — the render loop — so nothing here needs a
/// timer: the first frame stamps `started_ms`, every later one rewrites `light`, and the
/// announcement is dropped when [`NoticeKind::light_at`] runs out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Notice {
    pub kind: NoticeKind,
    /// Left `None` until the first frame that draws it.
    pub started_ms: Option<u32>,
    /// What the lamp shows at the current step; rewritten by the owner of the clock so reading
    /// the state never needs the time.
    pub light: LightState,
}

impl Notice {
    pub const fn new(kind: NoticeKind) -> Self {
        let light = match kind.light_at(0) {
            Some(light) => light,
            // Both announcements start lit; the arm is here because the clock reads this as an
            // `Option` and a default is cheaper than a panic.
            None => LightState::Off,
        };
        Self {
            kind,
            started_ms: None,
            light,
        }
    }
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
        // Waiting for a phone and acting on a credential are two moments of one mode, so the
        // colour is shared and the pace is what tells them apart. The resting colour is the
        // page's own: an offline device is not a mode.
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

    /// The shape every announcement shares: three flashes of the same light, the last one
    /// closing on its lit edge so the page gets the surface back without a dark pause.
    fn flashes(kind: NoticeKind, on: u32, off: u32, lit: LightState) {
        let step = on + off;
        let dark = Some(LightState::Off);
        assert_eq!(kind.light_at(0), Some(lit));
        assert_eq!(kind.light_at(on - 1), Some(lit));
        assert_eq!(kind.light_at(on), dark);
        assert_eq!(kind.light_at(step - 1), dark);
        assert_eq!(kind.light_at(step), Some(lit));
        assert_eq!(kind.light_at((FLASHES - 1) * step), Some(lit));
        assert_eq!(kind.light_at((FLASHES - 1) * step + on - 1), Some(lit));
        assert_eq!(kind.light_at((FLASHES - 1) * step + on), None);
        assert_eq!(kind.light_at(FLASHES * step), None);
    }

    #[test]
    fn a_join_that_worked_is_three_green_flashes_and_then_over() {
        flashes(
            NoticeKind::Joined,
            JOINED_ON_MS,
            JOINED_OFF_MS,
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
            OFFLINE_ON_MS,
            OFFLINE_OFF_MS,
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
            REFUSED_ON_MS,
            REFUSED_OFF_MS,
            LightState::Solid {
                color: Rgb(255, 0, 0),
                brightness: 96,
            },
        );
    }

    #[test]
    fn a_network_that_is_not_there_is_amber_and_a_refusal_is_red() {
        // Not found is the one failure a person fixes by moving the device; everything else
        // is the radio refusing what it was handed.
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
    fn a_notice_starts_lit_and_unstamped() {
        for kind in [NoticeKind::Joined, NoticeKind::Offline, NoticeKind::Refused] {
            let notice = Notice::new(kind);
            assert_eq!(notice.started_ms, None);
            assert_eq!(Some(notice.light), kind.light_at(0));
        }
    }
}
