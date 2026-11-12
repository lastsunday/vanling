//! The records more than one page reads: what the workers write and the render loop observes,
//! plus the aggregate that composes them. The Network draft lives here too — the device owns
//! it, even though only its page edits it.
//!
//! Records only: nothing here paints, drives a pin or opens a file, so every record compiles
//! for a host test with no board attached.

mod audio;

mod command;

mod device;

mod draft;

mod effects;

mod health;

mod light;

mod motion;

mod network;

mod playback;

mod touch;

pub use self::audio::{AudioState, AudioView};

pub use iot_core::drivers::audio::AudioPhase;

pub use self::command::Command;

pub use self::device::DeviceState;

pub use self::draft::{Entry, LIST_ROWS, NetworkDraft, SetupWait};

pub(crate) use self::effects::Effects;

pub use self::health::Health;

pub use self::light::{Breath, COLOR_GROUPS, LampLine, LightState, Notice, NoticeKind, PALETTE};

pub use self::motion::MotionState;

pub use self::network::NetworkState;

pub use iot_core::drivers::network::NetworkCommand;

pub use self::playback::{PlaybackCommand, PlaybackState, PlaybackView};

pub use iot_core::drivers::playback::PlaybackPhase;

pub use self::touch::{LivePoint, TouchState, TouchView};

pub use iot_core::drivers::board::Backed;

pub use iot_core::drivers::surface::{MAX_LIGHTS, SurfaceRole};
