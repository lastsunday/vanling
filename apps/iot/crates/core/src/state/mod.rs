use alloc::vec::Vec;

use crate::demand::{DemandKey, EffectiveDemand, Override};

use crate::drivers::audio::{AudioEnvelope, AudioSample};

use crate::drivers::input::{
    FINGER_LONG_PRESS, FINGER_SWIPE, FINGER_TAP, FingerLast, GestureEvent, InputEvent,
    MAX_TRACKED_POINTS, MOVE_DEADBAND_PX, SwipeDirection, TouchEvent, TouchStatus,
    direction_between,
};

use crate::drivers::light::{GROUP_CAPACITY, MAX_LIGHTS, Rgb};

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

pub use self::playback::{PlaybackPhase, PlaybackState};

/// A live contact slot: the tracker `id` that owns it plus where it last
/// landed, in framebuffer space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LivePoint {
    pub id: u8,
    pub x: u16,
    pub y: u16,
}

/// Whether the board has a panel: the screen pages are drawn on and questions are put on. Without
/// one, a verb acts instead of raising a box nobody can answer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Panel {
    /// A screen: the pages are drawn on it, and so is the box a hold or a leave asks in.
    #[default]
    Wired,
    /// No screen. The pages are still state — the link still reports, the list still ranks — but
    /// nothing is drawn and no question can be answered.
    Absent,
}

impl Panel {
    /// Whether the pages have somewhere to be drawn, and a question somewhere to be asked.
    pub const fn is_wired(self) -> bool {
        matches!(self, Self::Wired)
    }
}

/// All state owned by the device manager. The light surfaces come as one
/// array slot per instance (`MAX_LIGHTS` cap); unwired slots sit in the boot
/// state. Touch stays device-level: one panel per device, its digits and
/// readout beside the per-surface lights in the same snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceState {
    pub lights: [LightState; MAX_LIGHTS],
    /// The one-shot news a board with no screen says with its lamp instead of a page: a join
    /// that worked, a link that is not there, or a refusal that just landed. `None` is the
    /// ordinary case, and it is what hands the surface back to the page's own light. See
    /// [`Self::status_light`].
    pub notice: Option<Notice>,
    /// Whether this boot has already announced that the link is down. An outage is news once,
    /// not once per retry: a saved network that cannot associate republishes `Failed` every
    /// few seconds, and a flash each time would keep the product light off the surface the
    /// user is looking at. Cleared when the link connects. See [`Self::status_light`].
    pub outage_announced: bool,
    pub page: DisplayPage,
    /// The link: the phase, the access point and the address the network task last saw.
    /// `Off` on a board that wired no radio, which is what makes "has this device been on
    /// the air" answerable without asking the driver.
    pub link: LinkStatus,
    /// The board's own address, as the network task reported it at boot: `None` until then, and
    /// for good with no radio. Drawn on the header and written into the setup page.
    pub mac: Option<[u8; 6]>,
    /// The ranked scan list the network task last published; `None` means nothing has scanned.
    /// `None` and an empty list draw differently: one says nothing about the room, the other says
    /// it is empty. A report, not a request.
    pub aps: Option<ApList>,
    /// What the page is in the middle of taking: the row that was picked, the passphrase being
    /// typed on its keyboard, and the last refusal. Drawn, so it rides the diagnostics snapshot
    /// with the list.
    pub setup: NetworkSetup,
    /// Whether the button asked for setup mode on the splash, read once when the clock ends it.
    pub setup_wanted: bool,
    pub splash_reported: bool,
    /// What the panel asked the link for, waiting for the network task. A command, not drawn state;
    /// the two shapes differ only in how much help they carry.
    pub join: Option<JoinRequest>,
    /// The panel's request to leave the current link. A latch, because there is nothing to carry.
    /// Leaving is how setup mode is entered from a link.
    pub leave: bool,
    /// The panel's request to close setup mode and go back to the network this device keeps.
    ///
    /// The other half of [`Self::leave`], and a flag of its own because it is read by a different
    /// loop: a leave is acted on by the link that is up, a cancel by the access point that is.
    pub setup_cancel: bool,
    /// The panel's request to offer setup mode through its other front end.
    ///
    /// One request rather than a direction, because the direction is the link's: the page switches
    /// from whatever it is showing to whatever it is not, and the network task publishes which.
    pub switch_front: bool,
    /// The panel's request to put the setup access point up open or under a passphrase, carrying
    /// the state it should end in: `Some(true)` open, `Some(false)` secured, `None` nobody asked.
    ///
    /// The target rather than a flip: the passphrase is the station's, and whether it is on is
    /// read off the link — a second copy of that answer would be a panel that could disagree with
    /// the radio about whether the network is open. Naming the target also makes a double tap one
    /// request instead of two reversals.
    pub setup_open: Option<bool>,
    /// The panel's request to look around now, which is the action strip's other verb.
    ///
    /// A latch for the leave's reason: the survey is the radio's own loop, and what the panel can
    /// say about it is that it should not wait out the pause it is in.
    pub scan_now: bool,
    /// The panel's request to forget a saved network, named by the row a finger held.
    ///
    /// Carries the name because a forget is about the store rather than about the link: the network
    /// to drop is not the one the device is on, and nothing else in the state names it.
    pub forget: Option<Ssid>,
    pub motion_enabled: bool,
    pub motion: Option<MotionSample>,
    /// How many times each semantic has been raised, arbitrated across the
    /// hardware engines and the core-side classifiers. Diagnostic only: the
    /// business mapping is not wired yet, so nothing here moves a light.
    pub motion_counts: MotionCounts,
    /// What this board's motion stack declares, so a readout can tell a
    /// semantic it cannot report apart from one whose threshold never fires.
    pub motion_caps: MotionCapabilities,
    /// Whether this board wired a capture source. Gates the Audio page and the
    /// capture poll's write, so a board without a microphone neither shows the
    /// page nor stores the envelope.
    pub audio_enabled: bool,
    pub audio: AudioState,
    /// Whether this board wired playback. Gates the Speaker page and every
    /// playback move, the same way `audio_enabled` gates the Audio page — a
    /// board with no speaker shows no page and counts no plays.
    pub playback_enabled: bool,
    pub playback: PlaybackState,
    /// Whether this board wired a camera. Gates the Camera page, the same way
    /// `audio_enabled` gates the Audio page — a board with no sensor shows no
    /// page.
    pub camera_enabled: bool,
    /// Whether this build has a link at all. Gates the Network page the way
    /// `camera_enabled` gates the Camera page: a build with no radio has no link to
    /// show, and its `link` phase stays `Off` for ever.
    pub network_enabled: bool,
    /// Whether this board has a screen. Not a page gate like the four above — the pages are still
    /// state without one — but the answer to whether a question can be asked at all: a box needs
    /// somewhere to be drawn and a finger to answer it. What reads it is the strip's `LEAVE`, which
    /// a board with no screen takes rather than asks about.
    pub panel: Panel,
    /// Most recent touch snapshot, mirroring the lights' absolute-target
    /// contract: the render loop always reads the latest whole event.
    pub touch: Option<TouchEvent>,
    /// Live contact positions, slot-aligned with the classifier's tracker ids
    /// and freed as soon as a finger lifts or resolves a gesture.
    pub touch_points: [Option<LivePoint>; MAX_TRACKED_POINTS],
    /// `SwipeDirection::code` of the last frame-to-frame movement each live
    /// slot made while its finger is down; `0` while still or free.
    pub live_dir: [u8; MAX_TRACKED_POINTS],
    /// Rises when two slots are live in the same snapshot; saturated (a run
    /// counter asks "ever concurrent", not "how many frames").
    pub two_finger_runs: u8,
    /// Applied raw-frame heartbeat: incremented on every stored touch
    /// snapshot. Frozen while the controller stalls, still while a held finger
    /// rests — the on-panel digit tells the two apart.
    pub touch_frames: u16,
    /// Last resolved gesture of each tracked slot, aligned with `touch_points`.
    pub finger: [FingerLast; MAX_TRACKED_POINTS],
    /// Classifier-measured hold duration (ms) of the most recent resolved
    /// gesture lift, for the on-panel diagnostic digit.
    pub touch_held_ms: u16,
    pub tap_count: u8,
    /// Classifier press pulses, one per down edge. Press without a tap = the
    /// classifier swallowed it; no press at all = the controller never
    /// reported it — the drop-layer split.
    pub press_count: u8,
    pub long_press_count: u8,
    /// Input-path anomaly runs (rejected extra contacts, I2C failure runs);
    /// nonzero means the input path saw trouble.
    pub ghost_count: u8,
    pub swipe_count: u8,
    /// Direction + travel of the most recent resolved swipe.
    pub last_swipe: Option<(SwipeDirection, u16)>,
    /// Where the most recent resolved gesture began, framebuffer space.
    pub last_gesture_origin: Option<(u16, u16)>,
    /// Trailing point of the most recent multi-point gesture; `None` for the
    /// single-point gestures.
    pub last_gesture_end: Option<(u16, u16)>,
    /// Last raw touch-controller `GESTURE_ID`, stored verbatim so the overlay
    /// can compare the chip's engine with the software classifier. Never
    /// drives the light.
    pub chip_gesture_id: u8,
}

impl DeviceState {
    /// What a board with no screen has instead of one: the lamp reporting the link.
    ///
    /// `None` hands the surface back to the page. A board with a screen says all of this on
    /// it — and its light is the surface the pages draw — so the readout is a light-only
    /// board's, not a product-wide rule. A device that keeps a network and is merely off it is
    /// not a mode either: it gets one announcement and then the page's own light back.
    ///
    /// Priority is the news, then the mode, then the attempt:
    /// * [`Notice`] — a join, a missing link or a refusal, a second or so and then gone;
    /// * [`LightState::SETUP`] — the access point is up and a phone is being waited for;
    /// * [`LightState::JOINING`] — a credential the store does not keep yet is being tried.
    ///   A saved credential is not this: booting to a network is the product's own work, and
    ///   the page keeps its light while it runs.
    pub fn status_light(&self) -> Option<LightState> {
        if self.panel.is_wired() || !self.network_enabled {
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
    ///
    /// The single place a proposed demand is derived. [`DeviceManager`] holds the result as
    /// its demand state rather than calling this on every read, so a configurator has
    /// something to refine and somewhere to write back what the part could actually meet.
    pub fn demand(&self, keys: &[DemandKey]) -> EffectiveDemand {
        self.page.demand(keys)
    }
}

/// The per-lift data a resolved touch gesture carries into its tally: the
/// slot's last [`FingerLast`], the hold and endpoints; the counter bump and
/// target light are passed alongside.
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

/// Single owner of device state. Producers hand in commands or reports and the state layer takes
/// them whole, so the device never remembers "last breath". `Clone` is test-only, for the sweep
/// over reachable moves.
#[cfg_attr(test, derive(Clone))]
pub struct DeviceManager {
    state: DeviceState,
    /// Where the device lands after the splash: `IOT_BOOT_PAGE`, gated on the board's capabilities.
    /// Apart from [`DeviceState::page`] because the first page is always the splash.
    boot_page: DisplayPage,
    demand_keys: Vec<DemandKey>,
    /// What the build asks beyond what the page would.
    ///
    /// Held here rather than applied once at boot because the render loop
    /// republishes on every page change, so a one-shot patch would last a tick.
    override_: Override,
    /// What the device is committed to, as distinct from what was asked for.
    ///
    /// Held rather than re-derived on read so a configurator has something to refine: it
    /// writes the demand it could actually meet back here, and every consumer downstream
    /// reads that rather than the proposal. Recomputed wherever the proposal changes, which
    /// is why no other field of this struct may be written without
    /// [`Self::propose_demand`] following.
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
    /// Warm sunset: amber, golden, deep rose — low-blue palette favored by
    /// circadian research for a calm ambient breathing glow.
    pub const DEFAULT_GROUP: [u8; Breath::MAX_GROUP] = [16, 32, 3, 0, 0, 0, 0];
    pub const DEFAULT_GROUP_LEN: u8 = 3;

    /// Full-brightness entry point for solid mode; dimming is a long-press
    /// advance from here.
    pub const SOLID_DEFAULT_BRIGHTNESS: u8 = 255;

    /// Backlight floor for the breathing envelope: the panel never sinks below
    /// this percent while pixels are lit, so the trough stays a dim glow
    /// instead of a hard on/off blip.
    pub const BACKLIGHT_FLOOR_PCT: u8 = 12;

    pub fn new() -> Self {
        Self::with_motion(false, MotionCapabilities::EMPTY)
    }

    pub fn with_motion(motion_enabled: bool, motion_caps: MotionCapabilities) -> Self {
        let state = DeviceState {
            lights: [LightState::boot(); MAX_LIGHTS],
            notice: None,
            outage_announced: false,
            // Every device starts on the splash: the page is the one window in which the button
            // asks for setup mode, and a build's own page is what it lands on afterwards.
            page: DisplayPage::Splash,
            link: LinkStatus::default(),
            mac: None,
            aps: None,
            panel: Panel::default(),
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

    /// Chains the audio capability onto [`Self::with_motion`], so a board
    /// declares its capture source and the page that shows it in one place.
    /// Kept separate because most boards have motion and no microphone, and
    /// the app's composition point reads better as two explicit facts.
    pub fn with_audio(mut self, audio_enabled: bool) -> Self {
        self.state.audio_enabled = audio_enabled;
        self.propose_demand();
        self
    }

    /// Chains the playback capability on, so a board declares its speaker and
    /// the page that shows it in one place — the same shape as
    /// [`Self::with_audio`], and independent of it: a board can capture, play,
    /// both or neither.
    pub fn with_playback(mut self, playback_enabled: bool) -> Self {
        self.state.playback_enabled = playback_enabled;
        self
    }

    /// Chains the camera capability on, so a board declares its sensor and the
    /// page that shows it in one place — the same shape as
    /// [`Self::with_audio`], and independent of it.
    pub fn with_camera(mut self, camera_enabled: bool) -> Self {
        self.state.camera_enabled = camera_enabled;
        self.propose_demand();
        self
    }

    /// Chains the link capability on, so a build declares that it carries a radio
    /// and the page that shows what it is doing in one place — the same shape as
    /// [`Self::with_camera`], and independent of it. A board can have a radio and no
    /// panel, a panel and no radio, or both.
    pub fn with_network(mut self, network_enabled: bool) -> Self {
        self.state.network_enabled = network_enabled;
        self.propose_demand();
        self
    }

    /// Chains the panel on, which the other four do not imply: a build can carry a screen and none
    /// of the surfaces, and the C6 carries the radio and the light with no screen at all.
    ///
    /// Nothing is re-proposed: the panel is what draws a demand rather than something to meet.
    pub fn with_panel(mut self, panel: Panel) -> Self {
        self.state.panel = panel;
        self
    }

    /// Name the page the device lands on after the splash. A page the board cannot back falls back
    /// to `Ambient`, and so does the splash, which the landing report would otherwise have to leave
    /// again. This is a build-time convenience; the product's capabilities still decide.
    pub fn with_boot_page(mut self, page: DisplayPage) -> Self {
        // The same gate the cycle walks with, so a page a build can name is a page the board can
        // show.
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

    /// The page this device lands on once the splash is over. See [`Self::with_boot_page`].
    pub const fn boot_page(&self) -> DisplayPage {
        self.boot_page
    }

    /// Whether the button asked for setup mode while the splash was up.
    ///
    /// Read by the render loop when it reports the splash over, so the network task starts setup
    /// mode instead of joining: one answer for the landing and the link rather than two readers of
    /// a page that is gone.
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
            // A raw chip gesture read-back is diagnostic-only: store it, never
            // move the light.
            InputEvent::ChipGesture(id) => {
                self.state.chip_gesture_id = id;
            }
            InputEvent::Motion(batch) => {
                if self.state.motion_enabled {
                    self.state.motion = Some(batch.sample);
                    // Every semantic in the batch counts, not just the one a
                    // single-value readout would have kept: a shake and the
                    // tilt it causes travel together, and dropping either would
                    // under-report what the recognizer actually raised.
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
            // The button's request while the splash is up. Latching it here rather than acting on
            // it is what keeps the page up for its whole window: the landing below is the one thing
            // that reads it, so a finger that held on the splash cannot shorten the splash.
            BusinessIntent::RequestSetup => {
                if self.state.page == DisplayPage::Splash {
                    self.state.setup_wanted = true;
                }
            }
            // The clock's report that the splash is over, and the only thing that ends it: the page
            // is what makes the button's hold mean setup mode, so nothing a finger can do may leave
            // it. Ends once — a second report is not a second landing — and only from the splash,
            // because a device that has moved on has no page to come back to.
            //
            // The request decides the landing: a device asked to be configured lands on the page
            // that offers it, and one that was not lands on the page its build named. An ask on a
            // board with no radio lands like no ask, because there is nothing a page could set up.
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
            // Also a report, and the one that never changes: kept as it came, so the page can be
            // drawn the first time it is reached rather than after a second round from the radio.
            BusinessIntent::Mac(mac) => self.state.mac = Some(mac),
            // The store's own answer to a join the page asked it for: the row is one to type for,
            // which is where that tap would have landed had the store shown no mark on it.
            BusinessIntent::NoCopy(ssid) => self.no_copy(ssid),
            BusinessIntent::Key { key, at_ms } => self.apply_key(key, at_ms),
            BusinessIntent::Choose(ap) => self.pick_row(ap),
            BusinessIntent::AskForget(ap) => self.hold_row(ap),
            BusinessIntent::Scroll { rows } => self.scroll(rows),
            BusinessIntent::Leave => self.leave_link(),
            BusinessIntent::Answer(confirm) => self.answer(confirm),
            BusinessIntent::CloseSetup => self.close_setup(),
            // A request made while setup mode was up can still be in the bus when the radio
            // reports the mode gone; switching then would ask a front end nobody is showing. And
            // only a wired panel has another front to switch to: on a board without one, the hold
            // is not a way into the access point. `browsing` keeps the offline page's `AP` cell
            // working — that one is an explicit tap, not a hold.
            BusinessIntent::SwitchFront => {
                if (self.state.link.setup || self.state.link.browsing)
                    && self.state.panel.is_wired()
                {
                    self.state.switch_front = true;
                }
            }
            BusinessIntent::ScanNow => self.scan_now(),
            BusinessIntent::SetOpenAccessPoint(open) => self.set_open(open),
            // The one thing a question's own answer can be other than yes. Nothing else to do:
            // there is no state but the question, and dropping it is the whole of the answer.
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
    ///
    /// Taken rather than read, for the reason the credential is: it is a command, and a second take
    /// must not leave twice.
    pub fn take_leave(&mut self) -> bool {
        core::mem::take(&mut self.state.leave)
    }

    /// Take the panel's request to close setup mode and go back to what this device keeps.
    ///
    /// Taken for the leave's reason: a command, and a second take must not cancel twice.
    pub fn take_setup_cancel(&mut self) -> bool {
        core::mem::take(&mut self.state.setup_cancel)
    }

    /// Take the panel's request to look around now, if there is one.
    ///
    /// Taken for the leave's reason: a command, and a survey round asked for twice is one round.
    pub fn take_scan_now(&mut self) -> bool {
        core::mem::take(&mut self.state.scan_now)
    }

    /// Take the panel's request to offer setup through its other front end.
    ///
    /// Taken for the leave's reason.
    pub fn take_switch_front(&mut self) -> bool {
        core::mem::take(&mut self.state.switch_front)
    }

    /// Take the panel's request to put the setup access point up open (`Some(true)`) or under a
    /// passphrase (`Some(false)`), if it asked for either.
    pub fn take_setup_open(&mut self) -> Option<bool> {
        self.state.setup_open.take()
    }

    /// Take the panel's request to forget a saved network, if there is one.
    ///
    /// Taken for the same reason, and carrying the name because the store is what acts on it: the
    /// state knows which row a finger held, and the network task is the one holding the list.
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

    /// The slot owning `id`, if that tracker is live.
    fn point_slot(&self, id: u8) -> Option<usize> {
        self.state
            .touch_points
            .iter()
            .position(|p| p.is_some_and(|l| l.id == id))
    }

    /// A point with no free slot to spare is dropped rather than evicted, so a
    /// live finger never loses its row to a newer one.
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

    /// Resolving a gesture means the finger lifted, so its live point and
    /// movement arrow both go back to `-` until the next press.
    fn record_gesture(&mut self, id: u8, last: FingerLast) {
        if let Some(slot) = self.point_slot(id) {
            self.state.finger[slot] = last;
            self.state.touch_points[slot] = None;
            self.state.live_dir[slot] = 0;
        }
    }

    /// Saturating: it answers "ever concurrent", not "how many frames".
    fn bump_two_finger_runs(&mut self, before: usize) {
        if before < 2 && self.live_point_count() >= 2 {
            self.state.two_finger_runs = self.state.two_finger_runs.saturating_add(1);
        }
    }

    pub fn state(&self) -> DeviceState {
        self.state.clone()
    }

    /// What the device is committed to, which is the proposal narrowed to what the parts
    /// can actually meet. Not interchangeable with [`Self::state`]'s copy: a state is what
    /// the device *reached* and a demand is what it will be held to.
    ///
    /// Read this, not a fresh derivation, so a configurator's refusal is already accounted
    /// for by everyone downstream.
    pub fn demand(&self) -> &EffectiveDemand {
        &self.demand
    }

    /// Commits a demand the configurator could actually meet. The only way a demand stops
    /// being what was asked for, and the reason this is held rather than re-derived.
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

    /// Playback state, read by the render loop to draw the Speaker page and to
    /// decide whether a play it just applied has to reach the driver.
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
