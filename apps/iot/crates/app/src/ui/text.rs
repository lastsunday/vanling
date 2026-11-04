//! The glyph vocabulary the product's pages share: generic formatters, the network words,
//! and the sound labels.

pub use iot_ui::text::*;

pub use super::network_text::*;

/// The label the Speaker page prints for a sound.
pub const fn sound_label(sound: iot_core::drivers::playback::Sound) -> &'static [u8] {
    use iot_core::drivers::playback::Sound;
    match sound {
        Sound::Tap => b"TAP",
        Sound::Notice => b"NOTICE",
        Sound::Warn => b"WARN",
        Sound::Error => b"ERROR",
        Sound::Rise => b"RISE",
        Sound::Sweep => b"SWEEP",
    }
}
