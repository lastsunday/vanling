//! What a gesture asks the device to do: translation, the Network page's bands and verbs,
//! and the input path that writes the record before asking what the gesture meant.
//!
//! Nothing here owns state. `model` does, and this module only reads it and answers with an
//! [`ActionIntent`](crate::model::intent::ActionIntent).

mod apply;
mod audio;
mod light;
pub mod network;
mod playback;
mod translate;

pub use self::light::{
    BREATH_PERIODS_MS, BRIGHTNESS_ENVELOPE_STEP, COLOR_GROUPS, ColorGroup, PALETTE,
    SOLID_BRIGHTNESS_STEPS,
};
pub use self::network::{
    Answers, Band, Control, ControlKind, Strip, answers, confirm_verb, each_band, joining, strip,
};
pub use self::translate::translate;
