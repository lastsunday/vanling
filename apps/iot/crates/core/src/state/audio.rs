//! The audio readout a page draws: the envelope shape and the phase the panel shows.
use super::*;

/// Capture phase the Audio page drives. The capture driver never stops — it
/// keeps folding samples into its envelope — so a phase only decides which
/// envelope the panel draws and whether the sweep follows the live one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioPhase {
    /// Nothing captured yet: the panel draws no sweep, only the tap hint.
    #[default]
    Idle,
    /// Capturing: the panel follows the live envelope.
    Recording,
    /// A capture that ran and stopped: the panel shows the latched envelope.
    Stopped,
}

/// Capture state behind the Audio page. The envelopes are summaries from the
/// driver, never samples, so this stays small enough to sit in the state and
/// its derived diagnostics snapshot alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioState {
    pub phase: AudioPhase,
    /// The most recent capture poll, always current even while stopped — the
    /// driver keeps running underneath, so resuming picks the sweep up wherever
    /// it has moved to rather than where the previous capture left it.
    pub live: AudioSample,
    /// The poll latched when a capture stopped, so a stopped capture keeps
    /// showing what it caught instead of following the still-running scope.
    pub captured: AudioSample,
}

impl Default for AudioState {
    fn default() -> Self {
        Self::boot()
    }
}

impl AudioState {
    /// The state a board boots into: idle, with a blank envelope on both sides
    /// so the Audio page draws a flat line until a capture runs.
    pub const BOOT: Self = Self {
        phase: AudioPhase::Idle,
        live: AudioSample {
            envelope: AudioEnvelope::ZERO,
            elapsed_ms: 0,
            restarts: 0,
            dba_lsb: 0,
        },
        captured: AudioSample {
            envelope: AudioEnvelope::ZERO,
            elapsed_ms: 0,
            restarts: 0,
            dba_lsb: 0,
        },
    };

    pub const fn boot() -> Self {
        Self::BOOT
    }
}
