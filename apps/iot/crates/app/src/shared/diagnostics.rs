//! Device-level diagnostics: one snapshot fuses every subsystem the device exposes — the
//! touch-path readout, display page, motion sample, audio envelope and one
//! [`LightSnapshot`] per light surface — and fans out through pluggable
//! [`DiagnosticsSink`]s.

use crate::pages::network::model::NetworkDraft;
use crate::shared::route::Page;
use crate::shared::state::{AudioPhase, Health};
use crate::state::DeviceState;
use core::sync::atomic::{AtomicU32, Ordering};
use iot_core::demand::EffectiveDemand;
use iot_core::drivers::input::{FingerLast, MAX_TRACKED_POINTS};
use iot_core::drivers::motion::{MotionCapabilities, MotionCounts, MotionSample};
use iot_core::drivers::network::{ApList, LinkStatus};
use iot_core::drivers::playback::{PlaybackPhase, Sound};
use iot_core::drivers::surface::MAX_LIGHTS;

/// Intents the input worker could not hand to the coordinator because the ring was full.
///
/// Process-wide and saturating: it answers "has input ever gone missing" for the diagnostics
/// snapshot. The per-window count in the `[INPUT]` line answers "is it happening now".
static DROPPED_INTENTS: AtomicU32 = AtomicU32::new(0);

/// Counts one intent the input worker failed to enqueue.
pub fn note_dropped_intent() {
    let _ = DROPPED_INTENTS.try_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
        (count < u32::MAX).then_some(count + 1)
    });
}

/// How many intents have been dropped since boot.
pub fn dropped_intents() -> u32 {
    DROPPED_INTENTS.load(Ordering::Relaxed)
}

/// Capture state stamped on the Audio page: the phase driving the panel plus the envelope
/// it should draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AudioDiagnostics {
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

/// Playback state stamped on the Speaker page: the phase the page is showing, the sound a
/// tap would play, the mute latch, and the two tallies that make a busy finger visible —
/// sounds started against taps that arrived over one and were therefore not heard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlaybackDiagnostics {
    pub phase: PlaybackPhase,
    pub sound: Sound,
    pub muted: bool,
    pub plays: u16,
    pub dropped: u16,
    /// Whether the output is answering, and why not when it is not.
    pub health: Health,
}

impl DeviceState {
    pub(crate) fn audio_diagnostics(&self) -> AudioDiagnostics {
        let shown = match self.audio.phase {
            AudioPhase::Stopped => &self.audio.view.captured,
            AudioPhase::Recording | AudioPhase::Idle => &self.audio.view.live,
        };
        AudioDiagnostics {
            phase: self.audio.phase,
            envelope: shown.envelope,
            elapsed_ms: shown.elapsed_ms,
            restarts: shown.restarts,
            dba_lsb: shown.dba_lsb,
            health: self.audio.health,
        }
    }

    pub(crate) fn playback_diagnostics(&self) -> PlaybackDiagnostics {
        PlaybackDiagnostics {
            phase: self.playback.phase,
            sound: self.playback.sound,
            muted: self.playback.muted,
            plays: self.playback.view.plays,
            dropped: self.playback.view.dropped,
            health: self.playback.health,
        }
    }
}

/// Breathing mode's full dimension set carried by a light snapshot so the panel can print
/// every parameter the mode exposes; all zero outside breathing so the overlay's fixed rows
/// still have a value to show.
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

/// The diagnostics' light snapshot — `(mode, brightness, hue)` plus the breathing
/// dimensions — mirrors the primary hue/key material of one `state`'s light for the
/// diagnostic digits: mode `0` off, `1` breathing, `2` solid; brightness the solid level or
/// the breath's ceiling; hue the solid color or the breath's group head.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LightSnapshot {
    pub mode: u8,
    pub brightness: u8,
    pub hue: u8,
    /// Breathing dimensions; all zero outside breathing.
    pub breath: BreathSnapshot,
}

/// The touch-path diagnostics stamped on a light surface, folded field by field out of
/// [`DeviceState`](crate::state::DeviceState): counter gaps between `taps` and `presses`
/// localize dropped touches, `ghost` flags input trouble the classifier saw,
/// `points`/`finger` carry the live per-finger readout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TouchDiagnostics {
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

/// The on-surface diagnostics payload: touch-path counters/readout, display page, motion
/// sample, and one [`LightSnapshot`] per light surface slot, fused so one diff carries the
/// corner digits and the current page data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Diagnostics {
    pub touch: TouchDiagnostics,
    pub demand: EffectiveDemand,
    pub lights: [LightSnapshot; MAX_LIGHTS],
    pub page: Page,
    /// Where the link stands and what the last scan saw, as the network task last reported
    /// them — copied whole rather than borrowed, so a sink holds no reference into the
    /// state layer.
    pub link: LinkStatus,
    /// The board's own address, for the Network page's header.
    pub mac: Option<[u8; 6]>,
    pub aps: Option<ApList>,
    /// What the Network page is in the middle of taking: the row a passphrase is being
    /// typed for, the draft, and the last refusal.
    pub draft: NetworkDraft,
    pub motion: Option<MotionSample>,
    pub motion_counts: MotionCounts,
    pub motion_caps: MotionCapabilities,
    pub audio: AudioDiagnostics,
    pub playback: PlaybackDiagnostics,
    /// Input intents the coordinator's ring had no room for, since boot.
    pub dropped_intents: u32,
}

/// Consumer of the device diagnostic snapshot.
pub trait DiagnosticsSink {
    fn consume(&mut self, _diagnostics: &Diagnostics) {}
}

#[cfg(test)]
mod tests {
    use super::{dropped_intents, note_dropped_intent};

    #[test]
    fn a_noted_drop_is_visible_and_the_count_never_falls() {
        let before = dropped_intents();
        note_dropped_intent();
        assert!(
            dropped_intents() > before,
            "the counter is a boot total, so a note can only raise it"
        );
    }
}
