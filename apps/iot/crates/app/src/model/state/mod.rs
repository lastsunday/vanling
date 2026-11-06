//! Device state, split by the domain that owns it: each module holds one record and the
//! behaviour that reads it, and [`DeviceManager`] only routes between them.

mod audio;

mod band;

mod command;

mod device;

mod entry;

mod health;

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

pub use iot_core::drivers::audio::AudioPhase;

pub use self::band::{Band, each_band, joining};

pub use self::command::Command;

pub use self::device::DeviceState;

pub use self::entry::Entry;

pub use self::health::Health;

pub use self::light::{Breath, LampLine, LightState, Notice, NoticeKind};

pub use self::light_control::{
    BREATH_PERIODS_MS, BRIGHTNESS_ENVELOPE_STEP, COLOR_GROUPS, ColorGroup, PALETTE,
    SOLID_BRIGHTNESS_STEPS,
};

pub use self::manager::DeviceManager;

pub use self::motion::MotionState;

pub use self::network::NetworkState;

pub use iot_core::drivers::network::NetworkCommand;

pub use self::network_page::{
    Answers, Confirm, Control, ControlKind, NetworkPage, SetupWait, Strip, answers, confirm_verb,
    strip,
};

pub use self::playback::{PlaybackCommand, PlaybackDiagnostics, PlaybackState, PlaybackView};

pub use iot_core::drivers::playback::PlaybackPhase;

pub use self::touch::{LivePoint, TouchState, TouchView};

pub use self::translate::translate;

pub use self::wiring::Boot;

pub use iot_core::drivers::board::Backed;

pub use iot_core::drivers::surface::{MAX_LIGHTS, SurfaceRole};

pub use crate::model::route::{DisplayPage, Navigator};

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
