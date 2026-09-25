//! Device-level diagnostics: one snapshot fuses every subsystem the device
//! exposes — the touch-path readout plus one [`LightSnapshot`] per light
//! surface — and fans out through pluggable [`DiagnosticsSink`]s.
//!
//! The data source stays the central `DeviceState` in `crate::state`; a sink
//! only consumes, so the panel today (a `DisplayLight`) can be joined by
//! log/HTTP/WebSocket sinks without touching the state layer. Sinks are told
//! apart by their registration binding (render token / connection handle),
//! never by an identity they carry.

use crate::drivers::input::{FingerLast, MAX_TRACKED_POINTS};
use crate::drivers::light::MAX_LIGHTS;

/// Breathing mode's full dimension set carried by a light snapshot so the
/// panel can print every parameter the mode exposes; all zero outside
/// breathing so the overlay's fixed rows still have a value to show.
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

/// The diagnostics' light snapshot — `(mode, brightness, hue)` plus the
/// breathing dimensions — mirrors the primary hue/key material of one
/// `state`'s light for the diagnostic digits: mode `0` off, `1` breathing,
/// `2` solid; brightness is the solid level or the breath's ceiling; hue is
/// the solid color or the breath's group head (the standalone hue when the
/// group is empty).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LightSnapshot {
    pub mode: u8,
    pub brightness: u8,
    pub hue: u8,
    /// Breathing dimensions; all zero outside breathing.
    pub breath: BreathSnapshot,
}

/// The touch-path diagnostics stamped on a light surface: counter gaps
/// between `taps` and `presses` localize dropped touches, `ghost` flags input
/// trouble the classifier saw, `points`/`finger` carry the live per-finger
/// readout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TouchDiagnostics {
    pub taps: u8,
    pub presses: u8,
    pub double_taps: u8,
    pub long_presses: u8,
    pub ghost: u8,
    /// Live contact position each tracked slot last reported, framebuffer
    /// space; `None` while the slot is free.
    pub points: [Option<(u16, u16)>; MAX_TRACKED_POINTS],
    /// `SwipeDirection::code` of the slot's latest frame-to-frame move
    /// (`0` while still or free), aligned with `points`.
    pub live_dir: [u8; MAX_TRACKED_POINTS],
    /// How many times two contacts were live in the same snapshot, saturated.
    pub two_finger_runs: u8,
    /// Applied raw-frame heartbeat; frozen while the controller stalls versus
    /// standing still on a real hold.
    pub frames: u16,
    /// Per-slot last resolved gesture, aligned with `points`.
    pub finger: [FingerLast; MAX_TRACKED_POINTS],
    /// Hold duration (ms) of the last resolved lift.
    pub held_ms: u16,
    /// Resolved swipes, bumped once per classified slide.
    pub swipes: u8,
    /// `SwipeDirection::code` of the last resolved swipe, `0` before any.
    pub last_swipe_dir: u8,
    /// Euclidean length (px) of the last resolved swipe.
    pub last_swipe_dist: u16,
    /// Where the last resolved gesture began, framebuffer space; feeds `XY`.
    pub last_gesture_origin: Option<(u16, u16)>,
    /// Trailing point of the last multi-point gesture; `None` for the
    /// single-point ones. Feeds the `XY2` row.
    pub last_gesture_end: Option<(u16, u16)>,
    /// Last raw chip gesture id (`0x10` up / `0x14` left / `0x18` down /
    /// `0x1C` right on the FT5x06 family), `0` when none.
    pub chip_gesture_id: u8,
}

/// The on-surface diagnostics payload: touch-path counters/readout plus one
/// [`LightSnapshot`] per light surface slot, fused so one diff carries both
/// the corner digits and the mode each light a gesture moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Diagnostics {
    pub touch: TouchDiagnostics,
    pub lights: [LightSnapshot; MAX_LIGHTS],
}

/// Consumer of the device diagnostic snapshot. The screen surface implements
/// it to overlay the digit rows; log/HTTP/WebSocket sinks can subscribe to
/// the same snapshot later. The default no-op keeps light-only surfaces
/// ignorant. A sink is bound to a slot by registration, never by an identity
/// it carries.
pub trait DiagnosticsSink {
    /// Deliver the latest device diagnostic snapshot.
    fn consume(&mut self, _diagnostics: &Diagnostics) {}
}
