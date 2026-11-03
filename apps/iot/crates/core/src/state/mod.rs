//! Device state, split by the domain that owns it: each module holds one record and the
//! behaviour that reads it, and [`DeviceManager`] only routes between them.

mod audio;

mod band;

pub mod controls;

mod device;

mod entry;

mod light;

mod light_control;

mod manager;

mod motion;

mod network;

mod network_page;

mod playback;

mod touch;

mod translate;

mod wiring;

pub use self::audio::{AudioDiagnostics, AudioState, AudioView};

pub use crate::drivers::audio::AudioPhase;

pub use self::band::{Band, each_band, joining};

pub use self::device::DeviceState;

pub use self::entry::Entry;

pub use self::light::{Breath, LampLine, LightState, Notice, NoticeKind};

pub use self::light_control::{
    BREATH_PERIODS_MS, BRIGHTNESS_ENVELOPE_STEP, COLOR_GROUPS, ColorGroup, PALETTE,
    SOLID_BRIGHTNESS_STEPS,
};

pub use self::manager::DeviceManager;

pub use self::motion::MotionState;

pub use self::network::NetworkState;

pub use crate::drivers::network::NetworkCommand;

pub use self::network_page::{Confirm, NetworkPage};

pub use self::playback::{PlaybackCommand, PlaybackDiagnostics, PlaybackState, PlaybackView};

pub use crate::drivers::playback::PlaybackPhase;

pub use self::touch::{LivePoint, TouchState, TouchView};

pub use self::translate::translate;

pub use self::wiring::Boot;

pub use crate::drivers::board::Backed;

pub use crate::drivers::surface::{MAX_LIGHTS, SurfaceRole};

pub use crate::page::{DisplayPage, Navigator};

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
