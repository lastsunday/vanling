//! Device state, split by the domain that owns it: each module holds one record and the
//! behaviour that reads it, and [`DeviceManager`] only routes between them.

mod audio;

mod device;

mod entry;

mod light;

mod manager;

mod motion;

mod network;

mod playback;

mod touch;

mod wiring;

pub use self::audio::{AudioPhase, AudioState};

pub use self::device::DeviceState;

pub use self::entry::Entry;

pub use self::light::{Breath, LampLine, LightState, Notice, NoticeKind};

pub use self::manager::DeviceManager;

pub use self::motion::MotionState;

pub use self::network::{Confirm, NetworkCommand, NetworkDraft, NetworkState};

pub use self::playback::{PlaybackCommand, PlaybackPhase, PlaybackState};

pub use self::touch::{LivePoint, TouchState};

pub use self::wiring::{Backed, Boot, SurfaceRole};

pub use crate::page::DisplayPage;

/// Surface slots one device can carry, dimensioning [`DeviceState::lights`] and the render
/// slots.
pub const MAX_LIGHTS: usize = 2;

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
