//! The playback state: which sound is out and where it is in its lifecycle.
use super::*;

/// Playback phase the Speaker page drives. A sound is a one-shot: the driver
/// reports it finished, and only then is the next tap heard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackPhase {
    /// Nothing sounding: the next tap starts the page's sound.
    #[default]
    Idle,
    /// A sound is sounding. A tap in this phase is counted, not heard.
    Playing,
}

/// Playback state behind the Speaker page.
///
/// Small enough to sit in the state and its diagnostics snapshot alike: the
/// sounds themselves are the driver's, and this only records which one the page
/// holds, whether the output is latched quiet, and what the taps did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackState {
    pub phase: PlaybackPhase,
    /// The sound a tap plays, and the page's `SRC` row. Advances on every
    /// accepted tap, so the page shows what the *next* tap will play rather
    /// than echoing the last one.
    pub sound: Sound,
    /// The output latch. Held here rather than in the driver so a codec that
    /// resets itself cannot un-mute the speaker behind the panel's back.
    pub muted: bool,
    /// Sounds actually started; the panel's `PLY` row.
    pub plays: u16,
    /// Taps that arrived while a sound was sounding and were therefore not
    /// heard; the panel's second `PLY` figure, so a busy finger is visible as a
    /// number rather than as a sound that did not happen.
    pub dropped: u16,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self::boot()
    }
}

impl PlaybackState {
    /// The state a board boots into: idle, on the first sound, audible. Mute
    /// starts off because a boot that is silent would read as a dead speaker,
    /// and the codec's own reset already leaves the DAC unmuted.
    pub const BOOT: Self = Self {
        phase: PlaybackPhase::Idle,
        sound: Sound::Tap,
        muted: false,
        plays: 0,
        dropped: 0,
    };

    pub const fn boot() -> Self {
        Self::BOOT
    }
}
