//! The records more than one page reads: what the workers write and the render loop observes,
//! plus the aggregates that compose them. The pages' published view lives here too, so the
//! snapshot the renderers diff can name it without naming a page.
//!
//! Records only: nothing here paints, drives a pin or opens a file, so every record compiles
//! for a host test with no board attached.

mod audio;

mod command;

mod device;

mod effects;

mod health;

mod light;

mod motion;

mod network;

mod network_view;

mod page;

mod playback;

mod touch;

pub use self::audio::{AudioState, AudioView};

pub use iot_core::drivers::audio::AudioPhase;

pub use self::command::Command;

pub use self::device::DeviceState;

pub(crate) use self::effects::Effects;

pub use self::health::Health;

pub use self::light::{Breath, COLOR_GROUPS, LampLine, LightState, Notice, NoticeKind, PALETTE};

pub use self::motion::MotionState;

pub use self::network::NetworkState;

pub use self::network_view::{Entry, LIST_ROWS, NetworkView, SetupWait};

pub use self::page::PageSnapshot;

pub use iot_core::drivers::network::NetworkCommand;

pub use self::playback::{PlaybackCommand, PlaybackState, PlaybackView};

pub use iot_core::drivers::playback::PlaybackPhase;

pub use self::touch::{LivePoint, TouchState, TouchView};

pub use iot_core::drivers::board::Backed;

pub use iot_core::drivers::surface::{MAX_LIGHTS, SurfaceRole};
