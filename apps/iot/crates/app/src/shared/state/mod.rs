//! The state more than one page reads: the records the workers write and the render loop
//! observes. A page's own draft or list position belongs to its feature instead.
//!
//! Nothing here paints, drives a pin or opens a file, so every record compiles for a host
//! test with no board attached.

mod audio;

mod command;

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

pub(crate) use self::effects::Effects;

pub use self::health::Health;

pub use self::light::{Breath, LampLine, LightState, Notice, NoticeKind};

pub use self::motion::MotionState;

pub use self::network::NetworkState;

pub use iot_core::drivers::network::NetworkCommand;

pub use self::playback::{PlaybackCommand, PlaybackState, PlaybackView};

pub use iot_core::drivers::playback::PlaybackPhase;

pub use self::touch::{LivePoint, TouchState, TouchView};

pub use iot_core::drivers::board::Backed;

pub use iot_core::drivers::surface::{MAX_LIGHTS, SurfaceRole};
