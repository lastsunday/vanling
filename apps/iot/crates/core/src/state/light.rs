//! The light surfaces as the render loop reads them: the breathing schedule and the
//! behavior a page's snapshot carries.
use super::*;

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
        crate::drivers::light::breathe(
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
        crate::drivers::light::backlight_level(
            elapsed_ms,
            self.period_ms,
            self.min_brightness,
            self.max_brightness,
            floor_pct,
        )
    }
}

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

const JOINED_ON_MS: u32 = 100;
const JOINED_OFF_MS: u32 = 100;

const OFFLINE_ON_MS: u32 = 160;
const OFFLINE_OFF_MS: u32 = 160;

const REFUSED_ON_MS: u32 = 120;
const REFUSED_OFF_MS: u32 = 120;

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

    /// What the lamp shows this many milliseconds into the announcement, or `None` once the
    /// news is over.
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

impl DeviceManager {
    pub(super) fn announce(&mut self, kind: NoticeKind) {
        if !self.state.has_lamp() || !self.state.network_enabled {
            return;
        }
        self.state.notice = Some(Notice::new(kind));
    }

    pub fn tick_notice(&mut self, now_ms: u32) {
        let Some(mut notice) = self.state.notice else {
            return;
        };
        let started = *notice.started_ms.get_or_insert(now_ms);
        match notice.kind.light_at(now_ms.wrapping_sub(started)) {
            Some(light) => {
                notice.light = light;
                self.state.notice = Some(notice);
            }
            None => self.state.notice = None,
        }
    }

    pub fn notice_active(&self) -> bool {
        self.state.notice.is_some()
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
mod lamp {
    use super::*;
    use crate::state::test_support::*;

    use crate::drivers::network::Front;

    fn lamp_only() -> DeviceManager {
        DeviceManager::new()
            .with_network(true)
            .with_surface_roles([SurfaceRole::Lamp, SurfaceRole::Absent])
    }

    fn report(manager: &mut DeviceManager, status: LinkStatus) {
        manager.apply_business(BusinessIntent::LinkReport(status));
    }

    fn setup(manager: &mut DeviceManager) {
        report(
            manager,
            LinkStatus {
                setup: true,
                front: Front::AccessPoint,
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

        let mut screened = DeviceManager::new()
            .with_network(true)
            .with_surface_roles([SurfaceRole::Pages, SurfaceRole::Absent]);
        setup(&mut screened);
        assert_eq!(
            screened.state().link_notice(),
            None,
            "a display has a page to say it, so nothing is routed to a lamp"
        );

        let mut both = DeviceManager::new()
            .with_network(true)
            .with_surface_roles([SurfaceRole::Pages, SurfaceRole::Lamp]);
        setup(&mut both);
        assert_eq!(
            both.state().link_notice(),
            Some(LightState::SETUP),
            "a lamp still speaks beside a display"
        );

        let mut no_radio = DeviceManager::new()
            .with_network(false)
            .with_surface_roles([SurfaceRole::Lamp, SurfaceRole::Absent]);
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
        assert_eq!(manager.state().page, DisplayPage::Splash);
        manager.apply_business(BusinessIntent::SplashOver);
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
        assert_eq!(manager.state().page, DisplayPage::Ambient);
    }

    #[test]
    fn a_board_with_a_display_and_a_lamp_still_announces_on_the_lamp() {
        let mut manager = DeviceManager::new()
            .with_network(true)
            .with_surface_roles([SurfaceRole::Pages, SurfaceRole::Lamp]);
        manager.apply_business(BusinessIntent::SplashOver);
        assert_eq!(
            manager.state().notice.map(|notice| notice.kind),
            Some(NoticeKind::Offline)
        );
        assert_eq!(manager.state().link_notice(), Some(AMBER));
    }

    #[test]
    fn a_splash_hold_opens_setup_instead_of_flashing_offline() {
        let mut manager = lamp_only();
        manager.apply_business(BusinessIntent::RequestSetup);
        manager.apply_business(BusinessIntent::SplashOver);
        assert_eq!(manager.state().notice, None);
        assert_eq!(manager.state().page, DisplayPage::Network);
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
