//! The frame snapshot: everything one frame of pages draws from, extracted together so the
//! render layer never borrows the state it paints.

use crate::shared::model::DeviceState;
use crate::shared::model::{AudioPhase, Health, NetworkView};
use crate::shared::route::PageId;
use iot_core::demand::EffectiveDemand;
use iot_core::drivers::input::{FingerLast, MAX_TRACKED_POINTS};
use iot_core::drivers::motion::{MotionCapabilities, MotionCounts, MotionSample};
use iot_core::drivers::network::{ApList, LinkStatus};
use iot_core::drivers::playback::{PlaybackPhase, Sound};
use iot_core::drivers::surface::MAX_LIGHTS;

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

/// What each page publishes beyond the state it read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PageSnapshot {
    pub network: NetworkView,
}

/// One frame's drawing data, extracted from the state so a renderer holds no reference into
/// it. `demand` stays because the screen gates repaint and camera cadence on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameSnapshot {
    pub touch: TouchSnapshot,
    pub demand: EffectiveDemand,
    pub lights: [LightSnapshot; MAX_LIGHTS],
    pub page: PageId,
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
