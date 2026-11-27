//! The frame snapshot: everything one frame of pages draws from, extracted together so the
//! render layer never borrows the state it paints.

use crate::shared::i18n::Lang;
use crate::shared::model::DeviceState;
use crate::shared::model::{
    AudioPhase, Backed, Backlight, GlassBreath, GlowScope, Health, NetworkView,
};
use crate::shared::route::PageId;
use crate::shared::ui::feed::{More, Refresh};
use crate::shared::ui::keyboard::{FIELD_BYTES, Text};
use crate::shared::ui::widgets::date_picker::MonthView;
use crate::shared::ui::widgets::time_picker::TimeView;
use crate::shared::ui::widgets::wheel::WheelView;
use iot_core::demand::EffectiveDemand;
use iot_core::drivers::input::{FingerLast, MAX_TRACKED_POINTS};
use iot_core::drivers::motion::{MotionCapabilities, MotionCounts, MotionSample};
use iot_core::drivers::network::{ApList, LinkStatus};
use iot_core::drivers::playback::{PlaybackPhase, Sound};
use iot_core::drivers::surface::MAX_LIGHTS;
use iot_core::ui::{MAX_SCROLL_AREAS, Rect, ThemeId};

/// What the Audio page draws: the phase driving the panel plus the envelope it should draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AudioSnapshot {
    pub phase: AudioPhase,
    pub envelope: iot_core::drivers::audio::AudioEnvelope,
    pub elapsed_ms: u32,
    /// How many times the capture's DMA had to be re-armed.
    pub restarts: u16,
    /// A-weighted sound level of the shown capture, in LSB, slow-tracked.
    pub dba_lsb: u16,
    /// Whether the capture part is answering, and why not when it is not.
    pub health: Health,
}

/// What the Speaker page draws: the phase, the sound a tap would play, the mute latch, and
/// the two tallies that make a busy finger visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlaybackSnapshot {
    pub phase: PlaybackPhase,
    pub sound: Sound,
    pub muted: bool,
    pub plays: u16,
    pub dropped: u16,
    /// Whether the output is answering, and why not when it is not.
    pub health: Health,
}

impl DeviceState {
    pub(crate) fn audio_snapshot(&self) -> AudioSnapshot {
        let shown = match self.audio.phase {
            AudioPhase::Stopped => &self.audio.view.captured,
            AudioPhase::Recording | AudioPhase::Idle => &self.audio.view.live,
        };
        AudioSnapshot {
            phase: self.audio.phase,
            envelope: shown.envelope,
            elapsed_ms: shown.elapsed_ms,
            restarts: shown.restarts,
            dba_lsb: shown.dba_lsb,
            health: self.audio.health,
        }
    }

    pub(crate) fn playback_snapshot(&self) -> PlaybackSnapshot {
        PlaybackSnapshot {
            phase: self.playback.phase,
            sound: self.playback.sound,
            muted: self.playback.muted,
            plays: self.playback.view.plays,
            dropped: self.playback.view.dropped,
            health: self.playback.health,
        }
    }
}

/// The breathing dimensions a light snapshot carries, zero outside breathing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BreathSnapshot {
    pub period_ms: u16,
    pub hue_period_ms: u16,
    pub hue_span: u8,
    pub group_len: u8,
    pub min_brightness: u8,
    pub max_brightness: u8,
    pub saturation: u8,
}

/// One light's `(mode, brightness, hue)` plus its breathing dimensions: mode `0` off,
/// `1` breathing, `2` solid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LightSnapshot {
    pub mode: u8,
    pub brightness: u8,
    pub hue: u8,
    pub breath: BreathSnapshot,
}

/// The touch-path counters and live points the debug overlay draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TouchSnapshot {
    pub taps: u8,
    pub presses: u8,
    pub long_presses: u8,
    pub ghost: u8,
    pub points: [Option<(u16, u16)>; MAX_TRACKED_POINTS],
    pub live_dir: [u8; MAX_TRACKED_POINTS],
    pub two_finger_runs: u8,
    pub frames: u16,
    pub finger: [FingerLast; MAX_TRACKED_POINTS],
    pub held_ms: u16,
    pub swipes: u8,
    pub last_swipe_dir: u8,
    pub last_swipe_dist: u16,
    pub last_gesture_origin: Option<(u16, u16)>,
    pub last_gesture_end: Option<(u16, u16)>,
    /// Last raw chip gesture id (`0x10` up / `0x14` left / `0x18` down / `0x1C` right on
    /// the FT5x06 family), `0` when none.
    pub chip_gesture_id: u8,
}

/// The most scroll regions one page may publish. The text page uses two.
pub const TEXT_AREAS: usize = 2;

/// How many lines the wrapped run may be pre-broken into: the sample paragraph at 24 px.
/// Every line is one declaration box, so this and [`MAX_NODES`] move together; a longer
/// paragraph truncates with `…` at this many lines.
pub const TEXT_LINES: usize = 25;

/// How many standing glyph rows the page draws.
pub const TEXT_ROWS: usize = 18;

/// One line of the wrapped run, as a byte range and the width it was broken to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextLine {
    pub start: u16,
    pub end: u16,
    pub width: u16,
}

/// What one scroll region publishes to the view: where it sits, what it shows, how far it
/// can go, whether a thumb belongs beside it, and the two pieces that move with the offset
/// besides the box itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScrollAreaView {
    pub rect: Rect,
    /// The region's header line, whose reading includes the offset.
    pub header: Rect,
    /// The thumb's column beside the box.
    pub gutter: Rect,
    pub offset: i16,
    pub max: i16,
    pub bar: bool,
}

/// A frame that is the previous one moved inside its scroll boxes: what each box shows and how
/// far its content went. A page answers `None` when anything else changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScrollShift {
    pub areas: [ScrollAreaView; MAX_SCROLL_AREAS],
    pub shift: [i16; MAX_SCROLL_AREAS],
    pub count: u8,
}

impl ScrollShift {
    pub fn new(areas: &[ScrollAreaView], shift: &[i16]) -> Self {
        let mut moved = Self::default();
        let count = areas.len().min(shift.len()).min(MAX_SCROLL_AREAS);
        moved.areas[..count].copy_from_slice(&areas[..count]);
        moved.shift[..count].copy_from_slice(&shift[..count]);
        moved.count = count as u8;
        moved
    }

    pub fn views(&self) -> &[ScrollAreaView] {
        &self.areas[..usize::from(self.count)]
    }

    pub fn shifts(&self) -> &[i16] {
        &self.shift[..usize::from(self.count)]
    }
}

/// What the Text page publishes: one view per region, the run pre-broken into lines, the
/// glyph rows' widths, and whether a glide is in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextSnapshot {
    pub areas: [ScrollAreaView; TEXT_AREAS],
    pub lines: [TextLine; TEXT_LINES],
    pub line_count: u8,
    pub rows: [u16; TEXT_ROWS],
    /// The coverage line's static reading: how many sample glyphs the face lacks, and whether
    /// every printable ASCII code is present. Computed once, because the face never changes.
    pub missing: u16,
    pub ascii: bool,
    pub moving: bool,
}

/// What the Menu publishes: its one scroll view and whether a glide is in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MenuSnapshot {
    pub area: ScrollAreaView,
    pub moving: bool,
}

/// What the Widgets page publishes: its scroll view, the controls' values, and the phase of the
/// spinner the page owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WidgetsSnapshot {
    pub area: ScrollAreaView,
    pub moving: bool,
    pub spinning: bool,
    /// Bits for the buttons and chips the page keeps selected: filled, outlined, bare, chip A,
    /// chip B.
    pub selected: u8,
    pub switch: bool,
    pub check: bool,
    pub radio: u8,
    pub slider: u8,
    pub focus: bool,
    pub choice: u8,
    pub tile: bool,
    /// The control under the finger, by tag; zero when none.
    pub pressed: u16,
    pub spinner: u8,
    /// What the focused field holds, and the keys' own shift and layer state.
    pub text: Text<FIELD_BYTES>,
}

/// What the Advanced page publishes: its scroll view, the navigation and composed picks, and
/// the pager's index plus the live drag the finger is making.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AdvancedSnapshot {
    pub area: ScrollAreaView,
    pub moving: bool,
    pub nav: u8,
    pub tab: u8,
    pub segment: u8,
    pub step: u8,
    pub open: bool,
    pub action: bool,
    /// The card the pager centres.
    pub pager: u8,
    /// The finger's live horizontal travel, in panel pixels.
    pub drag: i16,
    /// The carousel's own card, its live drag, and whether it is turning itself.
    pub carousel: u8,
    pub carousel_drag: i16,
    pub autoplay: bool,
    /// How far the carousel's dwell has run, in whole percent.
    pub progress: u8,
}

/// What the Planner page publishes: the month on show, the day and hour picked, the rating,
/// the colour the picker holds, and the picker dialog when one is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannerSnapshot {
    pub area: ScrollAreaView,
    pub moving: bool,
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub rating: u8,
    pub hue: u8,
    pub sat: u8,
    pub val: u8,
    /// The inline hour wheel.
    pub inline: WheelView,
    pub dialog: PickerView,
}

impl Default for PlannerSnapshot {
    fn default() -> Self {
        Self {
            area: ScrollAreaView::default(),
            moving: false,
            year: 2026,
            month: 11,
            day: 26,
            hour: 9,
            minute: 5,
            second: 30,
            rating: 4,
            hue: 170,
            sat: 200,
            val: 240,
            inline: WheelView {
                position: 9 * 256,
                count: 24,
                wrap: true,
                moving: false,
            },
            dialog: PickerView::default(),
        }
    }
}

/// Which picker dialog is up; `None` keeps the page alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PickerOpen {
    #[default]
    None,
    Date,
    Month,
    Time,
}

/// A picker dialog's drawn state: its kind, the wheels it shows, and whether it still moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PickerView {
    pub open: PickerOpen,
    pub date: [WheelView; 3],
    pub month: MonthView,
    pub time: TimeView,
    pub moving: bool,
}

/// What the Records page publishes: the tree row, the timeline entry and the heatmap cell the
/// finger picked, plus the reorder list's order and its lifted row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordsSnapshot {
    pub area: ScrollAreaView,
    pub moving: bool,
    pub tree: Option<u8>,
    pub entry: Option<u8>,
    pub cell: Option<u16>,
    pub order: [u8; 5],
    pub lifted: Option<u8>,
    pub target: Option<u8>,
    pub seed: u16,
}

impl Default for RecordsSnapshot {
    fn default() -> Self {
        Self {
            area: ScrollAreaView::default(),
            moving: false,
            tree: None,
            entry: None,
            cell: None,
            order: [0, 1, 2, 3, 4],
            lifted: None,
            target: None,
            seed: 7,
        }
    }
}

/// What the Feedback page publishes: the three notices the finger can take down or raise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FeedbackSnapshot {
    pub area: ScrollAreaView,
    pub moving: bool,
    pub banner: bool,
    pub toast: bool,
    pub skeleton: bool,
    pub overlay: bool,
}

/// What the Extended page publishes: the field draft, the chips, and the keypad's last key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExtendedSnapshot {
    pub area: ScrollAreaView,
    pub moving: bool,
    pub focus: bool,
    pub chips: u8,
    pub form_error: bool,
    pub number: u16,
    pub key: Option<u8>,
    pub text: Text<FIELD_BYTES>,
}

/// What the Data page publishes: the marks' picks, the row that slid, and the overlay latches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DataSnapshot {
    pub area: ScrollAreaView,
    pub moving: bool,
    pub table: Option<u8>,
    pub bar: Option<u8>,
    pub gauge: u8,
    pub reveal: i16,
    pub empty_error: bool,
    pub dialog: bool,
    pub menu: u8,
    pub sheet: u8,
    pub menu_open: bool,
    pub sheet_open: bool,
    /// The trigger's bottom edge in panel pixels: where the menu hangs.
    pub menu_anchor: i16,
}

/// What the List page publishes: the feed's two edges and the row under the finger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ListSnapshot {
    pub area: ScrollAreaView,
    pub moving: bool,
    pub rows: u16,
    /// The stub data the painter rebuilds its rows from: the loaded prefix and the deletions.
    pub loaded: u16,
    pub removed: u64,
    pub selected: Option<u16>,
    pub reveal: Option<(u16, i16)>,
    pub pull: i16,
    pub refresh: Refresh,
    pub more: More,
}

/// The Lighting page's own choice: the breath it draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightingView {
    pub breath: GlassBreath,
}

impl Default for LightingView {
    fn default() -> Self {
        Self {
            breath: GlassBreath::CALM,
        }
    }
}

/// What each page publishes beyond the state it read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PageSnapshot {
    pub network: NetworkView,
    pub text: TextSnapshot,
    pub menu: MenuSnapshot,
    pub widgets: WidgetsSnapshot,
    pub advanced: AdvancedSnapshot,
    pub feedback: FeedbackSnapshot,
    pub extended: ExtendedSnapshot,
    pub data: DataSnapshot,
    pub planner: PlannerSnapshot,
    pub records: RecordsSnapshot,
    pub list: ListSnapshot,
    pub lighting: LightingView,
}

impl PageSnapshot {
    /// The page's scroll boxes and whether one is still gliding: the only pair a frame that
    /// merely scrolled may differ in. A page keeps its own box count, so `Text` is two.
    pub fn scroll(&self, page: PageId) -> (&[ScrollAreaView], bool) {
        match page {
            PageId::Text => (&self.text.areas, self.text.moving),
            PageId::Menu => (core::slice::from_ref(&self.menu.area), self.menu.moving),
            PageId::Widgets => (
                core::slice::from_ref(&self.widgets.area),
                self.widgets.moving,
            ),
            PageId::Advanced => (
                core::slice::from_ref(&self.advanced.area),
                self.advanced.moving,
            ),
            PageId::Feedback => (
                core::slice::from_ref(&self.feedback.area),
                self.feedback.moving,
            ),
            PageId::Extended => (
                core::slice::from_ref(&self.extended.area),
                self.extended.moving,
            ),
            PageId::Data => (core::slice::from_ref(&self.data.area), self.data.moving),
            PageId::Planner => (
                core::slice::from_ref(&self.planner.area),
                self.planner.moving,
            ),
            PageId::Records => (
                core::slice::from_ref(&self.records.area),
                self.records.moving,
            ),
            PageId::List => (core::slice::from_ref(&self.list.area), self.list.moving),
            _ => (&[], false),
        }
    }

    /// The same records with every offset zeroed and every glide flag cleared, so two frames
    /// that compare equal here differ only in where their boxes are scrolled to.
    pub fn settled(&self) -> Self {
        let mut out = *self;
        out.text.areas = out.text.areas.map(settled_area);
        out.text.moving = false;
        out.menu.area = settled_area(out.menu.area);
        out.menu.moving = false;
        out.widgets.area = settled_area(out.widgets.area);
        out.widgets.moving = false;
        out.advanced.area = settled_area(out.advanced.area);
        out.advanced.moving = false;
        out.feedback.area = settled_area(out.feedback.area);
        out.feedback.moving = false;
        out.extended.area = settled_area(out.extended.area);
        out.extended.moving = false;
        out.data.area = settled_area(out.data.area);
        out.data.moving = false;
        out.planner.area = settled_area(out.planner.area);
        out.planner.moving = false;
        out.records.area = settled_area(out.records.area);
        out.records.moving = false;
        out.list.area = settled_area(out.list.area);
        out.list.moving = false;
        out
    }
}

fn settled_area(area: ScrollAreaView) -> ScrollAreaView {
    ScrollAreaView { offset: 0, ..area }
}

/// One frame's drawing data, extracted from the state so a renderer holds no reference into
/// it. `demand` stays because the screen gates repaint and camera cadence on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameSnapshot {
    pub touch: TouchSnapshot,
    pub demand: EffectiveDemand,
    pub lights: [LightSnapshot; MAX_LIGHTS],
    /// The breath the glass should draw: the alarm's, else the page's own, else none.
    pub glass_breath: Option<GlassBreath>,
    /// The panel's backlight, independent of the breath above.
    pub backlight: Backlight,
    /// Where the breathing glass is allowed to appear.
    pub glow: GlowScope,
    pub page: PageId,
    /// What the board wired, for pages that list or gate on it.
    pub backed: Backed,
    /// The words the page prints.
    pub lang: Lang,
    pub theme: ThemeId,
    /// The page wants the light and the backlight held still for as long as it is up.
    pub steady: bool,
    pub link: LinkStatus,
    pub mac: Option<[u8; 6]>,
    pub aps: Option<ApList>,
    pub pages: PageSnapshot,
    pub motion: Option<MotionSample>,
    pub motion_counts: MotionCounts,
    pub motion_caps: MotionCapabilities,
    pub audio: AudioSnapshot,
    pub playback: PlaybackSnapshot,
}

impl FrameSnapshot {
    /// Whether the shown page has motion of its own to advance, which keeps the loop awake
    /// between snapshots.
    pub fn animating(&self) -> bool {
        match self.page {
            PageId::Text => self.pages.text.moving,
            PageId::Menu => self.pages.menu.moving,
            PageId::Widgets => self.pages.widgets.moving || self.pages.widgets.spinning,
            PageId::Advanced => self.pages.advanced.moving || self.pages.advanced.autoplay,
            PageId::Feedback => self.pages.feedback.moving,
            PageId::Extended => self.pages.extended.moving,
            PageId::Data => self.pages.data.moving,
            PageId::Planner => self.pages.planner.moving || self.pages.planner.dialog.moving,
            PageId::Records => self.pages.records.moving,
            PageId::List => self.pages.list.moving,
            _ => false,
        }
    }
}
