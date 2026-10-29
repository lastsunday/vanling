//! The sound catalogue, as the MIDI files that are its source of truth.

use alloc::boxed::Box;

use iot_core::drivers::playback::{
    SampleSource, Sound, Speaker, SpeakerFault, SpeakerOp, Sweep, Tone, from_midi,
};

/// The catalogue in cycle order: one `const` per file, so a sound that loses its file fails to
/// compile rather than playing the wrong one.
const TAP: &[u8] = include_bytes!("assets/tap.mid");
const NOTICE: &[u8] = include_bytes!("assets/notice.mid");
const WARN: &[u8] = include_bytes!("assets/warn.mid");
const ERROR: &[u8] = include_bytes!("assets/error.mid");
const RISE: &[u8] = include_bytes!("assets/rise.mid");

/// 300 Hz–6 kHz: outside this the codec's filters are still settling, and the deconvolved answer
/// describes them instead of the room. 0.55 ms of resolution between reflections.
pub const SWEEP_FROM_HZ: u32 = 300;
pub const SWEEP_TO_HZ: u32 = 6_000;

/// 630 whole cycles of the band, so consecutive sweeps join without a step.
pub const SWEEP_MS: u32 = 200;

/// A file that would not parse is refused rather than played as silence: the page would sit in
/// `Playing` for ever waiting on a voice that was never built.
pub fn source_of(sound: Sound) -> Result<Box<dyn SampleSource>, SpeakerFault> {
    match sound {
        Sound::Sweep => Ok(Box::new(Sweep::new(SWEEP_FROM_HZ, SWEEP_TO_HZ, SWEEP_MS))),
        _ => {
            let Some(file) = file_of(sound) else {
                return Err(SpeakerFault::new(
                    SpeakerOp::Play,
                    "a catalogue sound names no file to play",
                ));
            };
            let Some(notes) = from_midi(file) else {
                return Err(SpeakerFault::new(
                    SpeakerOp::Play,
                    "a sound file would not parse",
                ));
            };
            Ok(Box::new(Tone::playing(notes)))
        }
    }
}

pub fn play(speaker: &mut dyn Speaker, sound: Sound) -> Result<(), SpeakerFault> {
    speaker.play(source_of(sound)?)
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;
    use iot_core::drivers::playback::{SampleSource, Tone, from_midi, ramp_of};

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
    fn the_catalogue_weighs_a_few_hundred_bytes() {
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
