//! The glyph vocabulary the product's pages share: generic formatters, the sound labels,
//! and the light modes' words.

pub use iot_ui::text::*;

/// What a page prints for a reading the hardware does not provide.
pub const NOT_AVAILABLE: &[u8] = b"N/A";

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

/// The word for a light's mode, printed by the pages that report it.
pub const fn light_mode_word(mode: u8) -> &'static [u8] {
    match mode {
        1 => b"BRE",
        2 => b"SOL",
        _ => b"OFF",
    }
}
