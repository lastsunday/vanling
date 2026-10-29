//! The sound catalogue as source files: the recipes, and the MIDI files built from them.

use std::path::{Path, PathBuf};

/// Stands for silence in a [`Recipe`].
pub const REST: u8 = 0;

/// What one sound is made of: its name, the track name a DAW shows when the file is opened,
/// and `(MIDI note, ms)` pairs in play order, with [`REST`] for a gap.
pub type Recipe = (&'static str, &'static str, &'static [(u8, u16)]);

/// Every sound, and what its file has to contain.
pub const RECIPES: [Recipe; 5] = [
    (
        "Tap",
        "vanling tap - a minor third rising, the only three-note figure",
        &[(86, 100), (REST, 25), (88, 100), (REST, 25), (90, 100)],
    ),
    ("Notice", "vanling notice - one note", &[(83, 200)]),
    (
        "Warn",
        "vanling warn - four strikes, the highest",
        &[
            (93, 87),
            (REST, 12),
            (93, 87),
            (REST, 12),
            (93, 87),
            (REST, 12),
            (93, 87),
        ],
    ),
    (
        "Error",
        "vanling error - a major third falling",
        &[(80, 112), (REST, 25), (76, 125)],
    ),
    // Google's brand sound, and the lowest figure here so it cannot be mistaken
    // for the tap pitched down. Its bottom note is the one this speaker
    // reproduces worst, so the inharmonic partial carries it, not the note.
    (
        "Rise",
        "vanling rise - a bare octave, the lowest",
        &[(61, 137), (REST, 25), (73, 150)],
    ),
];

pub fn assets_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/src/assets")
}

pub fn file_name(sound: &str) -> String {
    format!("{}.mid", sound.to_lowercase())
}

/// A Standard MIDI File for one recipe: format 0, one track, 480 ticks a quarter, 120 BPM,
/// a track name a DAW can show.
pub fn mid(track_name: &str, notes: &[(u8, u16)]) -> Vec<u8> {
    const DIV: u32 = 480;
    const QUARTER_MS: u32 = 500;

    let vlq = |n: u32| -> Vec<u8> {
        let mut out = vec![(n & 0x7f) as u8];
        let mut rest = n >> 7;
        while rest > 0 {
            out.insert(0, (rest & 0x7f) as u8 | 0x80);
            rest >>= 7;
        }
        out
    };
    // Rounded rather than truncated, or a note written as 100 ms comes back as
    // 99 and the table stops matching its own file.
    let ticks = |ms: u16| (u32::from(ms) * DIV + QUARTER_MS / 2) / QUARTER_MS;

    let mut track = vec![0x00, 0xff, 0x51, 0x03, 0x07, 0xa1, 0x20];
    let name = track_name.as_bytes();
    track.extend([0x00, 0xff, 0x03, name.len() as u8]);
    track.extend_from_slice(name);

    let mut pending = 0u32;
    for &(note, ms) in notes {
        if note == REST {
            pending += ticks(ms);
            continue;
        }
        track.extend(vlq(pending));
        track.extend([0x90, note, 0x64]);
        pending = 0;
        track.extend(vlq(ticks(ms)));
        track.extend([0x80, note, 0x40]);
    }
    track.extend([0x00, 0xff, 0x2f, 0x00]);

    let mut file = b"MThd".to_vec();
    file.extend(6u32.to_be_bytes());
    file.extend([0, 1]);
    file.extend(DIV.to_be_bytes());
    file.extend(b"MTrk");
    file.extend((track.len() as u32).to_be_bytes());
    file.extend(track);
    file
}

/// Writes every recipe into `dir`, reporting what it wrote.
pub fn write(dir: &Path) -> std::io::Result<()> {
    for (sound, track_name, notes) in RECIPES {
        let path = dir.join(file_name(sound));
        let bytes = mid(track_name, notes);
        std::fs::write(&path, &bytes)?;
        println!("{}: {} B", path.display(), bytes.len());
    }
    Ok(())
}
