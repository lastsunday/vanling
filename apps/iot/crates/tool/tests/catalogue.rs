use std::collections::BTreeSet;

use iot_tool::midi::{RECIPES, REST, assets_dir, file_name, mid};

#[test]
fn the_files_are_what_the_recipes_generate() {
    for (sound, track_name, notes) in RECIPES {
        let path = assets_dir().join(file_name(sound));
        let on_disk = std::fs::read(&path).unwrap_or_else(|e| {
            panic!(
                "{}: {e} — run `cargo run -p iot-tool -- midi`",
                path.display()
            )
        });
        assert_eq!(
            on_disk,
            mid(track_name, notes),
            "{} does not match the {sound:?} recipe",
            path.display(),
        );
    }
}

#[test]
fn every_midi_file_has_a_recipe() {
    let found: BTreeSet<String> = std::fs::read_dir(assets_dir())
        .unwrap_or_else(|e| panic!("assets directory: {e}"))
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".mid"))
        .collect();
    assert_eq!(found, recipe_files(), "the recipes and the files disagree");
}

fn recipe_files() -> BTreeSet<String> {
    RECIPES
        .iter()
        .map(|&(sound, ..)| file_name(sound))
        .collect()
}

#[test]
fn the_recipes_are_told_apart_on_several_dimensions_at_once() {
    let mut registers: Vec<(u8, u8, &str)> = RECIPES
        .iter()
        .map(|&(sound, _, notes)| {
            let sounded: Vec<u8> = notes
                .iter()
                .map(|&(note, _)| note)
                .filter(|&note| note != REST)
                .collect();
            (
                *sounded.iter().min().unwrap(),
                *sounded.iter().max().unwrap(),
                sound,
            )
        })
        .collect();
    registers.sort_by_key(|&(lo, ..)| lo);

    for pair in registers.windows(2) {
        let ((_, hi, lower), (lo, _, upper)) = (&pair[0], &pair[1]);
        assert!(
            lo.saturating_sub(*hi) >= 3,
            "{lower} tops out at {hi} and {upper} starts at {lo}: less than three \
             semitones of register between them, which is too close to tell apart",
        );
    }

    let mut lengths: Vec<u32> = RECIPES
        .iter()
        .map(|&(_, _, notes)| notes.iter().map(|&(_, ms)| u32::from(ms)).sum())
        .collect();
    let before = lengths.len();
    lengths.sort_unstable();
    lengths.dedup();
    assert_eq!(
        lengths.len(),
        before,
        "two recipes run the same length, which is one cue gone"
    );
}

// Every millisecond survives the trip out to ticks and back. A tick is 25/24
// ms, so a value landing between the two rounding directions comes back a
// millisecond short — and a recipe that disagrees with its own file by 1 ms is
// one nobody can reason about.
#[test]
fn every_duration_survives_the_trip_to_ticks_and_back() {
    const DIV: u32 = 480;
    const QUARTER_MS: u32 = 500;

    let ticks = |ms: u16| (u32::from(ms) * DIV + QUARTER_MS / 2) / QUARTER_MS;
    let back = |t: u32| (t * QUARTER_MS / DIV) as u16;

    for (sound, _, notes) in RECIPES {
        for &(note, ms) in notes {
            let kind = if note == REST { "rest" } else { "note" };
            assert_eq!(
                back(ticks(ms)),
                ms,
                "{sound} has a {ms} ms {kind}, which does not come back as itself",
            );
        }
    }
}

#[test]
fn every_recipe_has_a_track_name() {
    for (sound, track_name, _) in RECIPES {
        assert!(
            track_name.starts_with(&format!("vanling {}", sound.to_lowercase())),
            "{sound} is called {track_name:?} in a DAW, which does not say what it is",
        );
    }
}

#[test]
fn the_catalogue_names_the_same_files() {
    let catalogue = catalogue_source();
    let named: BTreeSet<&str> = catalogue
        .match_indices("assets/")
        .map(|(at, _)| {
            let rest = &catalogue[at + "assets/".len()..];
            &rest[..rest.find('"').expect("an asset path with no closing quote")]
        })
        .collect();

    let wanted = recipe_files();
    let wanted: BTreeSet<&str> = wanted.iter().map(String::as_str).collect();

    assert_eq!(
        named, wanted,
        "iot-app includes files these recipes do not produce",
    );
}

fn catalogue_source() -> String {
    let path = assets_dir().join("../audio/sounds.rs");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}
