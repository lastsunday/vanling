//! Device state, split by the domain that owns it: each module holds one record and the
//! behaviour that reads it, and [`DeviceManager`] only routes between them.

mod audio;

mod command;

mod device;

mod entry;

mod health;

mod light;

mod manager;

mod motion;

mod network;

mod network_page;

mod playback;

mod touch;

mod wiring;

pub use self::audio::{AudioState, AudioView};

pub use iot_core::drivers::audio::AudioPhase;

pub use self::command::Command;

pub use self::device::DeviceState;

pub use self::entry::Entry;

pub use self::health::Health;

pub use self::light::{Breath, LampLine, LightState, Notice, NoticeKind};

pub use self::manager::DeviceManager;

pub use self::motion::MotionState;

pub use self::network::NetworkState;

pub use iot_core::drivers::network::NetworkCommand;

pub use self::network_page::{Confirm, LIST_ROWS, NetworkPage, SetupWait};

pub use self::playback::{PlaybackCommand, PlaybackState, PlaybackView};

pub use iot_core::drivers::playback::PlaybackPhase;

pub use self::touch::{LivePoint, TouchState, TouchView};

pub use self::wiring::Boot;

pub use iot_core::drivers::board::Backed;

pub use iot_core::drivers::surface::{MAX_LIGHTS, SurfaceRole};

pub use crate::model::route::{DisplayPage, Navigator};

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
