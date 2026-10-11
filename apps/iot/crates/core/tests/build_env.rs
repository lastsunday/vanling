//! Build-time configuration has to reach the compiler, and reaching it is not one step.
//!
//! `option_env!` is expanded where the crate is compiled, and the S3 build runs inside a
//! container. A variable set on the host is only seen if `FORWARDED_ENV` in
//! `scripts/iot-xtensa.sh` names it, because `docker run` passes no environment of its own.
//!
//! Missing that step fails silently. `option_env!` answers `None`, both variables fall back
//! to their documented default, and — for `IOT_DEMAND` — the `is_some()` guard in front of
//! the "named no plane this firmware recognises" warning is false when the variable never
//! arrived, so not even that is printed.
//!
//! For the product that is a device quietly booting the wrong way. For a probe it is a
//! whole measurement table taken on a build nobody asked for, reporting numbers that look
//! entirely ordinary — and `records/iot` is filled from probe output, so there the input
//! poisons the evidence rather than merely misbehaving.
//!
//! The host targets do not go through Docker, so neither symptom shows up in a compile, and
//! nothing else in the repository covers it. Hence this.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// `crates/core`, four levels below the repository root.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .expect("crates/core is always four levels below the root")
        .to_path_buf()
}

/// Every name a crate reads at compile time through `option_env!`.
fn option_env_inputs() -> BTreeSet<String> {
    fn walk(dir: &Path, found: &mut BTreeSet<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let Ok(source) = fs::read_to_string(&path) else {
                    continue;
                };
                for (_, name) in source.match_indices("option_env!(\"").map(|(at, _)| {
                    let rest = &source[at + "option_env!(\"".len()..];
                    let end = rest.find('"').expect("a name literal is closed");
                    (at, rest[..end].to_string())
                }) {
                    found.insert(name);
                }
            }
        }
    }

    let mut found = BTreeSet::new();
    walk(&repo_root().join("apps/iot/crates"), &mut found);
    found
}

/// Every name `scripts/iot-xtensa.sh` forwards into the container.
fn forwarded_env() -> BTreeSet<String> {
    let script = fs::read_to_string(repo_root().join("scripts/iot-xtensa.sh"))
        .expect("the xtensa wrapper is the thing that decides what reaches the compiler");
    let declaration = script
        .lines()
        .find_map(|line| line.trim().strip_prefix("FORWARDED_ENV=("))
        .expect("FORWARDED_ENV is what this whole check compares against");
    declaration
        .trim_end_matches(')')
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

#[test]
fn every_option_env_input_reaches_the_container() {
    let read = option_env_inputs();
    let forwarded = forwarded_env();

    assert!(
        !read.is_empty(),
        "no option_env! found under apps/iot/crates, so this check is searching for nothing \
         rather than confirming anything"
    );

    let missing: Vec<&String> = read.difference(&forwarded).collect();
    assert!(
        missing.is_empty(),
        "read with option_env! but not in FORWARDED_ENV ({} in scripts/iot-xtensa.sh): the \
         Docker build silently drops {} and the firmware falls back to its defaults — add the \
         name there",
        forwarded.len(),
        missing
            .iter()
            .map(|name| name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

#[test]
fn nothing_is_forwarded_that_nothing_reads() {
    // The weaker direction, and reported rather than asserted: a name can legitimately be
    // forwarded ahead of the crate that reads it. What it usually means is a rename that only
    // half landed, where the variable is set but the code no longer sees it.
    for name in forwarded_env().difference(&option_env_inputs()) {
        eprintln!(
            "note: FORWARDED_ENV names {name} but nothing reads it with option_env! — a \
             rename that only half landed?"
        );
    }
}
