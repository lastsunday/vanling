//! The sound catalogue, as the MIDI files that are its source of truth.
//!
//! Ungated, unlike the speaker driver that plays them, so a file that would not
//! parse is caught by `moon run iot:test-bsp` rather than by the first tap on
//! hardware. Parsing belongs to [`iot_core::drivers::playback::from_midi`];
//! this is where the bytes live and which file each sound is.

use iot_core::drivers::playback::Sound;

/// The catalogue in cycle order. One [`const`] per file so a sound that loses
/// its file fails to compile rather than playing the wrong one.
const TAP: &[u8] = include_bytes!("assets/tap.mid");
const NOTICE: &[u8] = include_bytes!("assets/notice.mid");
const WARN: &[u8] = include_bytes!("assets/warn.mid");
const ERROR: &[u8] = include_bytes!("assets/error.mid");
const RISE: &[u8] = include_bytes!("assets/rise.mid");

/// `None` for [`Sound::Sweep`], which is generated from a band rather than
/// played from a file — a bench excitation, kept in the catalogue only because a
/// sound is what `play` takes.
pub const fn file_of(sound: Sound) -> Option<&'static [u8]> {
    match sound {
        Sound::Tap => Some(TAP),
        Sound::Notice => Some(NOTICE),
        Sound::Warn => Some(WARN),
        Sound::Error => Some(ERROR),
        Sound::Rise => Some(RISE),
        Sound::Sweep => None,
    }
}

/// The Speaker page's name for a sound. Here rather than in the panel because
/// whether it fits the block is not something the panel can check on a host.
pub const fn label_of(sound: Sound) -> &'static [u8] {
    match sound {
        Sound::Tap => b"TAP",
        Sound::Notice => b"NOTICE",
        Sound::Warn => b"WARN",
        Sound::Error => b"ERROR",
        Sound::Rise => b"RISE",
        Sound::Sweep => b"SWEEP",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;
    use core::str;
    use iot_core::drivers::playback::{SampleSource, Tone, from_midi, ramp_of};
    use iot_core::overlay::{Block, block_value_x, block_x, level_value_end};

    /// A sound, parsed and rendered as the driver would render it.
    fn render(sound: Sound) -> Vec<i16> {
        let file = file_of(sound).unwrap_or_else(|| panic!("{sound:?} is not a file"));
        let notes = from_midi(file).unwrap_or_else(|| panic!("{sound:?}'s file does not parse"));
        let samples: usize = notes.iter().map(|n| n.ms as usize * 48).sum();
        let mut tone = Tone::playing(notes);
        let mut out = vec![0_i16; samples];
        assert_eq!(tone.fill(&mut out), samples, "{sound:?} filled short");
        assert!(tone.done(), "{sound:?} did not run out");
        out
    }

    #[test]
    fn every_catalogue_sound_names_a_file() {
        for sound in Sound::ALL {
            assert!(
                file_of(sound).is_some(),
                "{sound:?} is in the tap cycle and has to have a file to play",
            );
        }
        assert!(
            file_of(Sound::Sweep).is_none(),
            "the bench excitation is generated, not played from a file",
        );
    }

    #[test]
    fn every_catalogue_sound_opens_and_closes_on_silence() {
        // The property that costs a sound its life on the speaker: a waveform
        // that opens or ends on a step is a click, and a click is louder than
        // the note it interrupts. Held per sound, so a file edited without this
        // in mind fails here.
        for sound in Sound::ALL {
            let samples = render(sound);
            assert_eq!(samples[0], 0, "{sound:?} opens on a step, not on zero");
            let last = samples[samples.len() - 1].unsigned_abs();
            assert!(
                last < 200,
                "{sound:?} ends on {last} LSB, which is a cut rather than a release",
            );
        }
    }

    #[test]
    fn no_catalogue_sound_reaches_the_rail() {
        // The DAC volume register is the level control and this is only the
        // headroom left to it, so a sound that reached full scale would clip on
        // the speaker rather than in the samples.
        for sound in Sound::ALL {
            let samples = render(sound);
            let loudest = samples.iter().map(|&s| s.unsigned_abs()).max().unwrap();
            assert!(
                loudest < 32_767,
                "{sound:?} peaks at {loudest} LSB, which is the rail",
            );
        }
    }

    #[test]
    fn every_catalogue_sound_is_a_one_shot_of_the_right_size() {
        // The ceiling is generous against the 392 ms the warn ships at and tight
        // enough to catch a note pasted in with the wrong unit.
        for sound in Sound::ALL {
            let samples = render(sound);
            let ms = samples.len() / 48;
            assert!(
                (100..=400).contains(&ms),
                "{sound:?} runs {ms} ms, which is not a one-shot",
            );
        }
    }

    #[test]
    fn no_note_is_too_short_to_be_heard_as_its_own() {
        // Below about 80 ms a note stops being an event and becomes part of the sound
        // around it, which is what makes two of them hard to tell apart.
        for sound in Sound::ALL {
            let notes = from_midi(file_of(sound).unwrap()).unwrap();
            for note in notes.iter().filter(|note| note.hz != 0) {
                assert!(
                    note.ms >= 83,
                    "{sound:?} has a {} ms note, under the 82.5 ms floor",
                    note.ms,
                );
            }
        }
    }

    #[test]
    fn a_note_ramps_at_both_of_its_ends() {
        // No version of MIDI carries an envelope, so the rule is what has to hold
        // rather than the individual notes.
        for sound in Sound::ALL {
            let notes = from_midi(file_of(sound).unwrap()).unwrap();
            for note in &notes {
                if note.hz == 0 {
                    assert_eq!(note.ramp_ms, 0, "{sound:?}: a rest has no ends to ramp");
                } else {
                    assert_eq!(note.ramp_ms, ramp_of(note.ms), "{sound:?}: {note:?}");
                }
            }
        }
    }

    #[test]
    fn a_sound_name_fits_its_block() {
        // **Checked from here because the panel cannot check itself.** The block pitch
        // is sized on seven glyphs for a coordinate pair, and `NOTICE` takes six
        // of the seven. The failure is silent: the name is stamped, the pixels
        // that would land in the next block are dropped, and the page reads as
        // if it were shorter.
        let value_x = block_value_x(Block::First);
        let next_block = block_x(Block::Second);
        for sound in Sound::ALL {
            let label = label_of(sound);
            let end = level_value_end(value_x, label.len());
            assert!(
                end < next_block,
                "{:?} prints {:?}, which ends at column {end} and reaches into \
                 the next block at {next_block}",
                sound,
                str::from_utf8(label).unwrap_or("<非 ASCII>"),
            );
        }
        assert_eq!(
            label_of(Sound::Notice).len(),
            6,
            "the widest name is the one with a glyph of margin left; if this \
             number moves, the slot is either nearly overflowing or has slack \
             nobody has to design around",
        );
    }

    #[test]
    fn the_catalogue_weighs_a_few_hundred_bytes() {
        // What the move to MIDI bought: five sounds that used to need a stored sample
        // each.
        let total: usize = Sound::ALL
            .iter()
            .map(|&sound| file_of(sound).unwrap().len())
            .sum();
        assert!(
            total < 1024,
            "the catalogue is {total} B, which is no longer 'a few hundred bytes'",
        );
    }
}
