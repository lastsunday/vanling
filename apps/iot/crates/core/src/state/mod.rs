use alloc::vec::Vec;

use crate::demand::{DemandKey, EffectiveDemand, Override};

use crate::drivers::audio::{AudioEnvelope, AudioSample};

use crate::drivers::input::{
    FINGER_LONG_PRESS, FINGER_SWIPE, FINGER_TAP, FingerLast, GestureEvent, InputEvent,
    MAX_TRACKED_POINTS, MOVE_DEADBAND_PX, SwipeDirection, TouchEvent, TouchStatus,
    direction_between,
};

use crate::paint::{GROUP_CAPACITY, Rgb};

use crate::drivers::motion::{MotionCapabilities, MotionCounts, MotionSample};

use crate::drivers::network::{
    AccessPoint, ApList, Credentials, Front, JoinRequest, LinkPhase, LinkStatus, NetworkError,
    PASSWORD_BYTES, Password, Ssid,
};

use crate::drivers::playback::Sound;

use crate::intent::{BusinessIntent, OperationIntent};

use crate::keyboard::{Cycling, Key, Layer, MULTITAP_WINDOW_MS};

mod audio;

mod entry;

mod light;

mod network;

mod page;

mod playback;

pub use self::audio::{AudioPhase, AudioState};

pub use self::entry::Entry;

pub use self::light::{Breath, LampLine, LightState, Notice, NoticeKind};

pub use self::network::{Confirm, NetworkSetup};

pub use self::page::DisplayPage;

pub use self::playback::{PlaybackCommand, PlaybackPhase, PlaybackState};

/// Surface slots one device can carry, dimensioning [`DeviceState::lights`] and the render
/// slots.
pub const MAX_LIGHTS: usize = 2;

/// A live contact slot: the tracker `id` that owns it plus where it last landed, in
/// framebuffer space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LivePoint {
    pub id: u8,
    pub x: u16,
    pub y: u16,
}

/// What one wired surface is for: pages, ambient light, or nothing at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SurfaceRole {
    /// Draws the pages and puts the questions on them.
    #[default]
    Pages,
    /// Shows light only; the link's news lands here when there is no page to say it.
    Lamp,
    /// This slot carries no surface.
    Absent,
}

/// All state owned by the device manager.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceState {
    pub lights: [LightState; MAX_LIGHTS],
    /// What each slot's surface is for, fixed by the wiring rather than by the current
    /// appearance.
    pub surface_roles: [SurfaceRole; MAX_LIGHTS],
    /// The one-shot news a lamp says instead of a page: a join that worked, a link that is
    /// not there, or a refusal that just landed.
    pub notice: Option<Notice>,
    /// Whether this boot has already announced that the link is down.
    pub outage_announced: bool,
    pub page: DisplayPage,
    /// The link: the phase, the access point and the address the network task last saw.
    pub link: LinkStatus,
    /// The board's own address, as the network task reported it at boot: `None` until then,
    /// and for good with no radio.
    pub mac: Option<[u8; 6]>,
    /// The ranked scan list the network task last published; `None` means nothing has
    /// scanned.
    pub aps: Option<ApList>,
    /// What the page is in the middle of taking: the row that was picked, the passphrase
    /// being typed on its keyboard, and the last refusal.
    pub setup: NetworkSetup,
    /// Whether the button asked for setup mode on the splash, read once when the clock ends
    /// it.
    pub setup_wanted: bool,
    pub splash_reported: bool,
    /// What the panel asked the link for, waiting for the network task.
    pub join: Option<JoinRequest>,
    /// The panel's request to leave the current link.
    pub leave: bool,
    /// The panel's request to close setup mode and go back to the network this device
    /// keeps.
    pub setup_cancel: bool,
    /// The panel's request to offer setup mode through its other front end.
    pub switch_front: bool,
    /// The panel's request to put the setup access point up open or under a passphrase,
    /// carrying the state it should end in: `Some(true)` open, `Some(false)` secured,
    /// `None` nobody asked.
    pub setup_open: Option<bool>,
    /// The panel's request to look around now, which is the action strip's other verb.
    pub scan_now: bool,
    /// The panel's request to forget a saved network, named by the row a finger held.
    pub forget: Option<Ssid>,
    pub motion_enabled: bool,
    pub motion: Option<MotionSample>,
    /// How many times each semantic has been raised, arbitrated across the hardware engines
    /// and the core-side classifiers.
    pub motion_counts: MotionCounts,
    /// What this board's motion stack declares, so a readout can tell a semantic it cannot
    /// report apart from one whose threshold never fires.
    pub motion_caps: MotionCapabilities,
    /// Whether this board wired a capture source.
    pub audio_enabled: bool,
    pub audio: AudioState,
    /// Whether this board wired playback.
    pub playback_enabled: bool,
    pub playback: PlaybackState,
    /// Whether this board wired a camera.
    pub camera_enabled: bool,
    /// Whether this build has a link at all.
    pub network_enabled: bool,
    /// Most recent touch snapshot, mirroring the lights' absolute-target contract: the
    /// render loop always reads the latest whole event.
    pub touch: Option<TouchEvent>,
    /// Live contact positions, slot-aligned with the classifier's tracker ids and freed as
    /// soon as a finger lifts or resolves a gesture.
    pub touch_points: [Option<LivePoint>; MAX_TRACKED_POINTS],
    /// `SwipeDirection::code` of the last frame-to-frame movement each live slot made while
    /// its finger is down; `0` while still or free.
    pub live_dir: [u8; MAX_TRACKED_POINTS],
    /// Rises when two slots are live in the same snapshot; saturated (a run counter asks
    /// "ever concurrent", not "how many frames").
    pub two_finger_runs: u8,
    /// Applied raw-frame heartbeat: incremented on every stored touch snapshot.
    pub touch_frames: u16,
    /// Last resolved gesture of each tracked slot, aligned with `touch_points`.
    pub finger: [FingerLast; MAX_TRACKED_POINTS],
    /// Classifier-measured hold duration (ms) of the most recent resolved gesture lift, for
    /// the on-panel diagnostic digit.
    pub touch_held_ms: u16,
    pub tap_count: u8,
    /// Classifier press pulses, one per down edge.
    pub press_count: u8,
    pub long_press_count: u8,
    /// Input-path anomaly runs (rejected extra contacts, I2C failure runs); nonzero means
    /// the input path saw trouble.
    pub ghost_count: u8,
    pub swipe_count: u8,
    /// Direction + travel of the most recent resolved swipe.
    pub last_swipe: Option<(SwipeDirection, u16)>,
    /// Where the most recent resolved gesture began, framebuffer space.
    pub last_gesture_origin: Option<(u16, u16)>,
    /// Trailing point of the most recent multi-point gesture; `None` for the single-point
    /// gestures.
    pub last_gesture_end: Option<(u16, u16)>,
    /// Last raw touch-controller `GESTURE_ID`, stored verbatim so the overlay can compare
    /// the chip's engine with the software classifier.
    pub chip_gesture_id: u8,
}

impl DeviceState {
    /// Whether any wired surface draws the pages and puts the questions on them.
    pub fn has_pages(&self) -> bool {
        self.surface_roles.contains(&SurfaceRole::Pages)
    }

    /// Whether any wired surface is a lamp the link can speak through.
    pub fn has_lamp(&self) -> bool {
        self.surface_roles.contains(&SurfaceRole::Lamp)
    }

    /// What the link has to say on a lamp: the one-shot news, the setup mode, or the attempt.
    pub fn link_notice(&self) -> Option<LightState> {
        if !self.has_lamp() || !self.network_enabled {
            return None;
        }
        if let Some(notice) = self.notice {
            return Some(notice.light);
        }
        if self.link.setup && self.link.front == Front::AccessPoint {
            return Some(LightState::SETUP);
        }
        if self.link.phase == LinkPhase::Connecting && !self.link.saved {
            return Some(LightState::JOINING);
        }
        None
    }

    /// The page's demand with the board's wiring clamped on top.
    pub fn demand(&self, keys: &[DemandKey]) -> EffectiveDemand {
        self.page.demand(keys)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GestureRecord {
    id: u8,
    last: FingerLast,
    held_ms: u16,
    origin: (u16, u16),
    end: Option<(u16, u16)>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum NetworkCommand {
    Join(JoinRequest),
    Leave,
    LeaveSetup,
    SwitchFront,
    SetupOpen(bool),
    ScanNow,
    Forget(Ssid),
}

/// Single owner of device state.
#[cfg_attr(test, derive(Clone))]
pub struct DeviceManager {
    state: DeviceState,
    boot_page: DisplayPage,
    demand_keys: Vec<DemandKey>,
    override_: Override,
    demand: EffectiveDemand,
}

impl DeviceManager {
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

    pub fn new() -> Self {
        Self::with_motion(false, MotionCapabilities::EMPTY)
    }

    pub fn with_motion(motion_enabled: bool, motion_caps: MotionCapabilities) -> Self {
        let state = DeviceState {
            lights: [LightState::boot(); MAX_LIGHTS],
            surface_roles: [SurfaceRole::default(); MAX_LIGHTS],
            notice: None,
            outage_announced: false,
            page: DisplayPage::Splash,
            link: LinkStatus::default(),
            mac: None,
            aps: None,
            setup: NetworkSetup::default(),
            setup_wanted: false,
            join: None,
            leave: false,
            setup_cancel: false,
            switch_front: false,
            splash_reported: false,
            setup_open: None,
            scan_now: false,
            forget: None,
            motion_enabled,
            motion: None,
            motion_counts: MotionCounts::ZERO,
            motion_caps,
            audio_enabled: false,
            audio: AudioState::boot(),
            playback_enabled: false,
            playback: PlaybackState::boot(),
            camera_enabled: false,
            network_enabled: false,
            touch: None,
            touch_points: [None; MAX_TRACKED_POINTS],
            live_dir: [0; MAX_TRACKED_POINTS],
            two_finger_runs: 0,
            touch_frames: 0,
            finger: [FingerLast::empty(); MAX_TRACKED_POINTS],
            touch_held_ms: 0,
            tap_count: 0,
            press_count: 0,
            long_press_count: 0,
            ghost_count: 0,
            swipe_count: 0,
            last_swipe: None,
            last_gesture_origin: None,
            last_gesture_end: None,
            chip_gesture_id: 0,
        };
        let demand = Self::proposed(&state, &[], &Override::none());
        Self {
            state,
            boot_page: DisplayPage::Ambient,
            demand_keys: Vec::new(),
            override_: Override::none(),
            demand,
        }
    }

    pub fn with_demand_keys(mut self, keys: Vec<DemandKey>) -> Self {
        self.demand_keys = keys;
        self.propose_demand();
        self
    }

    fn proposed(state: &DeviceState, keys: &[DemandKey], override_: &Override) -> EffectiveDemand {
        let mut demand = state.demand(keys);
        override_.clone().apply(&mut demand);
        demand
    }

    fn propose_demand(&mut self) {
        self.demand = Self::proposed(&self.state, &self.demand_keys, &self.override_);
    }

    /// Chains the audio capability onto [`Self::with_motion`], so a board declares its
    /// capture source and the page that shows it in one place.
    pub fn with_audio(mut self, audio_enabled: bool) -> Self {
        self.state.audio_enabled = audio_enabled;
        self.propose_demand();
        self
    }

    /// Chains the playback capability on, so a board declares its speaker and the page that
    /// shows it in one place — the same shape as [`Self::with_audio`], and independent of
    /// it: a board can capture, play, both or neither.
    pub fn with_playback(mut self, playback_enabled: bool) -> Self {
        self.state.playback_enabled = playback_enabled;
        self
    }

    /// Chains the camera capability on, so a board declares its sensor and the page that
    /// shows it in one place — the same shape as [`Self::with_audio`], and independent of
    /// it.
    pub fn with_camera(mut self, camera_enabled: bool) -> Self {
        self.state.camera_enabled = camera_enabled;
        self.propose_demand();
        self
    }

    /// Chains the link capability on, so a build declares that it carries a radio and the
    /// page that shows what it is doing in one place — the same shape as
    /// [`Self::with_camera`], and independent of it.
    pub fn with_network(mut self, network_enabled: bool) -> Self {
        self.state.network_enabled = network_enabled;
        self.propose_demand();
        self
    }

    /// Chains the per-slot surface roles on, which the other four do not imply: a build can
    /// carry a screen with nothing else, a lamp with no screen at all, or both.
    pub fn with_surface_roles(mut self, roles: [SurfaceRole; MAX_LIGHTS]) -> Self {
        self.state.surface_roles = roles;
        self
    }

    /// Name the page the device lands on after the splash.
    pub fn with_boot_page(mut self, page: DisplayPage) -> Self {
        let page = match page {
            DisplayPage::Splash => DisplayPage::Ambient,
            page if page.is_backed(
                self.state.motion_enabled,
                self.state.audio_enabled,
                self.state.playback_enabled,
                self.state.camera_enabled,
                self.state.network_enabled,
            ) =>
            {
                page
            }
            _ => DisplayPage::Ambient,
        };
        self.boot_page = page;
        self
    }

    /// The page this device lands on once the splash is over.
    pub const fn boot_page(&self) -> DisplayPage {
        self.boot_page
    }

    /// Whether the button asked for setup mode while the splash was up.
    pub const fn setup_wanted(&self) -> bool {
        self.state.setup_wanted
    }

    /// Apply an absolute target to the `instance`-th light surface, clamped to
    /// the installed array so one out-of-range intent can never panic.
    pub fn apply_to(&mut self, instance: usize, state: LightState) {
        self.state.lights[instance.min(MAX_LIGHTS - 1)] = state;
    }

    pub fn apply_operation(&mut self, op: OperationIntent) {
        match op.event {
            InputEvent::Button(_) => {}
            InputEvent::Gesture(GestureEvent::Press { id, x, y }) => {
                let before = self.live_point_count();
                if let Some(slot) = self.upsert_point(id, x, y) {
                    self.state.live_dir[slot] = 0;
                }
                self.bump_two_finger_runs(before);
                self.state.press_count = self.state.press_count.wrapping_add(1);
            }
            InputEvent::Gesture(GestureEvent::Ghost) => {
                self.state.ghost_count = self.state.ghost_count.saturating_add(1);
            }
            InputEvent::Gesture(GestureEvent::Tap {
                id, x, y, held_ms, ..
            }) => {
                self.state.tap_count = self.tally_gesture(
                    self.state.tap_count,
                    GestureRecord {
                        id,
                        last: FingerLast {
                            kind: FINGER_TAP,
                            dir: 0,
                            value: held_ms,
                        },
                        held_ms,
                        origin: (x, y),
                        end: None,
                    },
                );
            }
            InputEvent::Gesture(GestureEvent::LongPress { id, x, y, held_ms }) => {
                self.state.long_press_count = self.tally_gesture(
                    self.state.long_press_count,
                    GestureRecord {
                        id,
                        last: FingerLast {
                            kind: FINGER_LONG_PRESS,
                            dir: 0,
                            value: held_ms,
                        },
                        held_ms,
                        origin: (x, y),
                        end: None,
                    },
                );
            }
            InputEvent::Gesture(GestureEvent::Swipe {
                id,
                x,
                y,
                end_x,
                end_y,
                direction,
                held_ms,
                distance_px,
            }) => {
                self.state.last_swipe = Some((direction, distance_px));
                self.state.swipe_count = self.tally_gesture(
                    self.state.swipe_count,
                    GestureRecord {
                        id,
                        last: FingerLast {
                            kind: FINGER_SWIPE,
                            dir: direction.code(),
                            value: distance_px,
                        },
                        held_ms,
                        origin: (x, y),
                        end: Some((end_x, end_y)),
                    },
                );
            }
            InputEvent::ChipGesture(id) => {
                self.state.chip_gesture_id = id;
            }
            InputEvent::Motion(batch) => {
                if self.state.motion_enabled {
                    self.state.motion = Some(batch.sample);
                    for event in batch.events.iter() {
                        self.state.motion_counts.bump(event);
                    }
                }
            }
            InputEvent::Touch(event) => {
                let before = self.live_point_count();
                for point in event.points.iter().take(usize::from(event.len)) {
                    match point.status {
                        TouchStatus::Down | TouchStatus::Contact => {
                            if let Some(slot) = self.point_slot(point.id)
                                && let Some(live) = self.state.touch_points[slot]
                                && let Some(dir) = direction_between(
                                    live.x,
                                    live.y,
                                    point.x,
                                    point.y,
                                    MOVE_DEADBAND_PX,
                                )
                            {
                                self.state.live_dir[slot] = dir.code();
                            }
                            self.upsert_point(point.id, point.x, point.y);
                        }
                        TouchStatus::Release => {
                            if let Some(slot) = self.point_slot(point.id) {
                                self.state.live_dir[slot] = 0;
                            }
                            self.clear_point(point.id);
                        }
                    }
                }
                self.bump_two_finger_runs(before);
                self.state.touch_frames = self.state.touch_frames.wrapping_add(1);
                self.state.touch = Some(event);
            }
        }
    }

    pub fn apply_business(&mut self, intent: BusinessIntent) {
        match intent {
            BusinessIntent::Playback(action) => self.apply_playback(action),
            BusinessIntent::Capture(action) => self.apply_capture_intent(action),
            BusinessIntent::LinkReport(status) => self.apply_link_report(status),
            BusinessIntent::Invalid => {}
            BusinessIntent::SetLight { instance, state } => {
                self.apply_to(usize::from(instance), state)
            }
            BusinessIntent::NextPage => {
                self.state.page = self.state.page.next_page(
                    self.state.motion_enabled,
                    self.state.audio_enabled,
                    self.state.playback_enabled,
                    self.state.camera_enabled,
                    self.state.network_enabled,
                );
            }
            BusinessIntent::RequestSetup => {
                if self.state.page == DisplayPage::Splash {
                    self.state.setup_wanted = true;
                }
            }
            BusinessIntent::SplashOver => {
                if self.state.page == DisplayPage::Splash {
                    self.state.page = if self.state.setup_wanted && self.state.network_enabled {
                        DisplayPage::Network
                    } else {
                        self.boot_page
                    };
                    // A device that boots with nothing to join has nothing to say after the
                    // splash except that: one amber burst, then the page's own light. A hold that
                    // asked for setup is not this — the access point is the answer to it, and the
                    // mode has its own colour.
                    if !self.state.setup_wanted
                        && self.state.network_enabled
                        && !self.state.link.saved
                        && !self.state.link.setup
                        && !self.state.outage_announced
                    {
                        self.state.outage_announced = true;
                        self.announce(NoticeKind::Offline);
                    }
                }
            }
            BusinessIntent::Mac(mac) => self.state.mac = Some(mac),
            BusinessIntent::NoCopy(ssid) => self.no_copy(ssid),
            BusinessIntent::Key { key, at_ms } => self.apply_key(key, at_ms),
            BusinessIntent::Choose(ap) => self.pick_row(ap),
            BusinessIntent::AskForget(ap) => self.hold_row(ap),
            BusinessIntent::Scroll { rows } => self.scroll(rows),
            BusinessIntent::Leave => self.leave_link(),
            BusinessIntent::Answer(confirm) => self.answer(confirm),
            BusinessIntent::CloseSetup => self.close_setup(),
            BusinessIntent::SwitchFront => {
                if (self.state.link.setup || self.state.link.browsing) && self.state.has_pages() {
                    self.state.switch_front = true;
                }
            }
            BusinessIntent::ScanNow => self.scan_now(),
            BusinessIntent::SetOpenAccessPoint(open) => self.set_open(open),
            BusinessIntent::Dismiss => self.state.setup.confirm = None,
        }
        self.propose_demand();
    }

    pub fn take_splash_end(&mut self) -> Option<bool> {
        if self.state.splash_reported || self.state.page == DisplayPage::Splash {
            return None;
        }
        self.state.splash_reported = true;
        Some(self.state.setup_wanted)
    }

    pub fn take_command(&mut self) -> Option<NetworkCommand> {
        if let Some(join) = self.take_join() {
            return Some(NetworkCommand::Join(join));
        }
        if self.take_leave() {
            return Some(NetworkCommand::Leave);
        }
        if self.take_setup_cancel() {
            return Some(NetworkCommand::LeaveSetup);
        }
        if self.take_switch_front() {
            return Some(NetworkCommand::SwitchFront);
        }
        if let Some(open) = self.take_setup_open() {
            return Some(NetworkCommand::SetupOpen(open));
        }
        if self.take_scan_now() {
            return Some(NetworkCommand::ScanNow);
        }
        self.take_forget().map(NetworkCommand::Forget)
    }

    pub fn take_join(&mut self) -> Option<JoinRequest> {
        self.state.join.take()
    }

    /// Take the panel's request to leave the network the link is on, if there is one.
    pub fn take_leave(&mut self) -> bool {
        core::mem::take(&mut self.state.leave)
    }

    /// Take the panel's request to close setup mode and go back to what this device keeps.
    pub fn take_setup_cancel(&mut self) -> bool {
        core::mem::take(&mut self.state.setup_cancel)
    }

    /// Take the panel's request to look around now, if there is one.
    pub fn take_scan_now(&mut self) -> bool {
        core::mem::take(&mut self.state.scan_now)
    }

    /// Take the panel's request to offer setup through its other front end.
    pub fn take_switch_front(&mut self) -> bool {
        core::mem::take(&mut self.state.switch_front)
    }

    /// Take the panel's request to put the setup access point up open (`Some(true)`) or
    /// under a passphrase (`Some(false)`), if it asked for either.
    pub fn take_setup_open(&mut self) -> Option<bool> {
        self.state.setup_open.take()
    }

    /// Take the panel's request to forget a saved network, if there is one.
    pub fn take_forget(&mut self) -> Option<Ssid> {
        self.state.forget.take()
    }

    fn live_point_count(&self) -> usize {
        self.state
            .touch_points
            .iter()
            .filter(|p| p.is_some())
            .count()
    }

    fn point_slot(&self, id: u8) -> Option<usize> {
        self.state
            .touch_points
            .iter()
            .position(|p| p.is_some_and(|l| l.id == id))
    }

    fn upsert_point(&mut self, id: u8, x: u16, y: u16) -> Option<usize> {
        let slot = self
            .point_slot(id)
            .or_else(|| self.state.touch_points.iter().position(Option::is_none));
        if let Some(slot) = slot {
            self.state.touch_points[slot] = Some(LivePoint { id, x, y });
        }
        slot
    }

    fn clear_point(&mut self, id: u8) {
        if let Some(slot) = self.point_slot(id) {
            self.state.touch_points[slot] = None;
        }
    }

    fn tally_gesture(&mut self, counter: u8, record: GestureRecord) -> u8 {
        self.state.touch_held_ms = record.held_ms;
        self.state.last_gesture_origin = Some(record.origin);
        self.state.last_gesture_end = record.end;
        self.record_gesture(record.id, record.last);
        counter.wrapping_add(1)
    }

    fn record_gesture(&mut self, id: u8, last: FingerLast) {
        if let Some(slot) = self.point_slot(id) {
            self.state.finger[slot] = last;
            self.state.touch_points[slot] = None;
            self.state.live_dir[slot] = 0;
        }
    }

    fn bump_two_finger_runs(&mut self, before: usize) {
        if before < 2 && self.live_point_count() >= 2 {
            self.state.two_finger_runs = self.state.two_finger_runs.saturating_add(1);
        }
    }

    pub fn state(&self) -> DeviceState {
        self.state.clone()
    }

    /// What the device is committed to, which is the proposal narrowed to what the parts
    /// can actually meet.
    pub fn demand(&self) -> &EffectiveDemand {
        &self.demand
    }

    /// Commits a demand the configurator could actually meet.
    pub fn commit_demand(&mut self, demand: EffectiveDemand) {
        self.demand = demand;
    }

    /// Pins what the build asks beyond the page's claim.
    pub fn with_override(mut self, override_: Override) -> Self {
        self.override_ = override_;
        self.propose_demand();
        self
    }

    pub fn light_state(&self) -> LightState {
        self.state.lights[0]
    }

    /// Clamped like [`Self::apply_to`]: an out-of-range read is the last surface,
    /// never a panic.
    #[cfg(test)]
    pub fn light_state_at(&self, instance: u8) -> LightState {
        self.state.lights[usize::from(instance).min(MAX_LIGHTS - 1)]
    }

    #[cfg(test)]
    pub fn touch_state(&self) -> Option<TouchEvent> {
        self.state.touch
    }

    #[cfg(test)]
    pub fn motion_state(&self) -> Option<MotionSample> {
        self.state.motion
    }

    #[cfg(test)]
    pub fn motion_counts_state(&self) -> MotionCounts {
        self.state.motion_counts
    }

    /// Playback state, read by the render loop to draw the Speaker page and to decide
    /// whether a play it just applied has to reach the driver.
    pub fn playback_state(&self) -> PlaybackState {
        self.state.playback
    }
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
