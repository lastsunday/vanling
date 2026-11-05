//! The module graph the compiler will not enforce.
//!
//! Rust checks that crates do not form cycles; modules in one crate may. So the directions the
//! app's codemap relies on are asserted here, where a stray `use` fails the build instead of
//! quietly reversing an edge the next reader has to rediscover.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .expect("a crate under apps/iot/crates is always four levels below the root")
        .to_path_buf()
}

fn app_src() -> PathBuf {
    repo_root().join("apps/iot/crates/app/src")
}

fn core_src() -> PathBuf {
    repo_root().join("apps/iot/crates/core/src")
}

fn ui_src() -> PathBuf {
    repo_root().join("apps/iot/crates/ui/src")
}

fn sources(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    collect(dir, &mut found);
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found
}

fn collect(dir: &Path, found: &mut Vec<(PathBuf, String)>) {
    for entry in std::fs::read_dir(dir).expect("every source directory is readable") {
        let path = entry
            .expect("the source tree does not change while it is read")
            .path();
        if path.is_dir() {
            collect(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let text = std::fs::read_to_string(&path).expect("a source file is UTF-8");
            found.push((path, text));
        }
    }
}

fn assert_no_reference(files: Vec<(PathBuf, String)>, forbidden: &[&str]) {
    for (path, text) in files {
        for needle in forbidden {
            assert!(
                !text.contains(needle),
                "{} reaches {needle}; that edge is not in the codemap",
                path.display()
            );
        }
    }
}

/// The same sources, shipping code only: every `#[cfg(test)]` item and every comment is
/// removed. A test may name whatever it likes to build a fixture, and a doc comment may
/// link anywhere; the graph asserted here is the one the compiler sees.
fn shipping_sources(dir: &Path) -> Vec<(PathBuf, String)> {
    sources(dir)
        .into_iter()
        .filter(|(path, _)| {
            path.file_name()
                .is_some_and(|name| name != "test_support.rs")
        })
        .map(|(path, text)| (path, without_comments(&without_tests(&text))))
        .collect()
}

fn shipping_source(path: PathBuf) -> Vec<(PathBuf, String)> {
    let text = std::fs::read_to_string(&path).expect("the scanned source is readable");
    vec![(path, without_comments(&without_tests(&text)))]
}

/// The text with every comment replaced by one space. String and character literals are
/// copied verbatim, so a `//` inside one is not a comment.
fn without_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                out.push(b' ');
                i = line_end(bytes, i);
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                out.push(b' ');
                i = block_end(bytes, i);
            }
            b'"' => {
                let end = string_end(bytes, i);
                out.extend_from_slice(&bytes[i..end]);
                i = end;
            }
            b'r' => match raw_string_end(bytes, i) {
                Some(end) => {
                    out.extend_from_slice(&bytes[i..end]);
                    i = end;
                }
                None => {
                    out.push(bytes[i]);
                    i += 1;
                }
            },
            b'\'' => {
                let end = char_end(bytes, i);
                out.extend_from_slice(&bytes[i..end]);
                i = end;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).expect("dropping comments leaves valid UTF-8")
}

fn without_tests(text: &str) -> String {
    const MARK: &str = "#[cfg(test)]";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(MARK) {
        out.push_str(&rest[..at]);
        match item_end(&rest[at + MARK.len()..]) {
            Some(end) => rest = &rest[at + MARK.len() + end..],
            // An attribute with nothing after it cannot hide anything.
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Where the item after a `#[cfg(test)]` ends: past its `;` for a declaration, past its
/// matching `}` for a body. Braces in strings, characters and comments are not structure.
fn item_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b';' => return Some(i + 1),
            b'{' => {
                let mut depth = 0usize;
                while i < bytes.len() {
                    match bytes[i] {
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                return Some(i + 1);
                            }
                        }
                        b'/' if bytes.get(i + 1) == Some(&b'/') => {
                            i = line_end(bytes, i);
                            continue;
                        }
                        b'/' if bytes.get(i + 1) == Some(&b'*') => {
                            i = block_end(bytes, i);
                            continue;
                        }
                        b'"' => {
                            i = string_end(bytes, i);
                            continue;
                        }
                        b'r' => {
                            if let Some(end) = raw_string_end(bytes, i) {
                                i = end;
                                continue;
                            }
                        }
                        b'\'' => {
                            i = char_end(bytes, i);
                            continue;
                        }
                        _ => {}
                    }
                    i += 1;
                }
                return None;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => i = line_end(bytes, i),
            b'/' if bytes.get(i + 1) == Some(&b'*') => i = block_end(bytes, i),
            _ => i += 1,
        }
    }
    None
}

fn line_end(bytes: &[u8], i: usize) -> usize {
    bytes[i..]
        .iter()
        .position(|&byte| byte == b'\n')
        .map_or(bytes.len(), |at| i + at + 1)
}

fn block_end(bytes: &[u8], i: usize) -> usize {
    let mut at = i + 2;
    let mut depth = 1;
    while at < bytes.len() {
        if bytes[at..].starts_with(b"/*") {
            depth += 1;
            at += 2;
        } else if bytes[at..].starts_with(b"*/") {
            depth -= 1;
            at += 2;
            if depth == 0 {
                return at;
            }
        } else {
            at += 1;
        }
    }
    bytes.len()
}

fn string_end(bytes: &[u8], i: usize) -> usize {
    let mut at = i + 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            b'"' => return at + 1,
            _ => at += 1,
        }
    }
    bytes.len()
}

/// `Some(end)` only when the `r` at `i` opens a raw string; otherwise the caller reads it
/// as an ordinary identifier byte.
fn raw_string_end(bytes: &[u8], i: usize) -> Option<usize> {
    let mut at = i + 1;
    let mut hashes = 0;
    while bytes.get(at) == Some(&b'#') {
        hashes += 1;
        at += 1;
    }
    if bytes.get(at) != Some(&b'"') {
        return None;
    }
    at += 1;
    while at < bytes.len() {
        if bytes[at] == b'"'
            && bytes[at + 1..]
                .iter()
                .take(hashes)
                .filter(|&&b| b == b'#')
                .count()
                == hashes
        {
            return Some(at + 1 + hashes);
        }
        at += 1;
    }
    Some(bytes.len())
}

/// A character literal ends after its closing quote; a lifetime is left as one byte.
fn char_end(bytes: &[u8], i: usize) -> usize {
    if bytes.get(i + 1) == Some(&b'\\') {
        let mut at = i + 2;
        while at < bytes.len() {
            if bytes[at] == b'\'' {
                return at + 1;
            }
            at += 1;
        }
        return bytes.len();
    }
    if bytes.get(i + 2) == Some(&b'\'') {
        i + 3
    } else {
        i + 1
    }
}

#[test]
fn the_network_never_reaches_the_panel() {
    assert_no_reference(
        sources(&app_src().join("network")),
        &["crate::surface", "crate::frame_loop"],
    );
}

#[test]
fn the_panel_side_never_reaches_the_radio() {
    let src = app_src();
    let mut files = sources(&src.join("surface"));
    files.extend(sources(&src.join("playback")));
    files.extend(sources(&src.join("frame_loop")));
    files.extend(sources(&src.join("workers")));
    for name in ["input.rs", "boot.rs"] {
        let path = src.join(name);
        let text = std::fs::read_to_string(&path).expect("the file is part of the app");
        files.push((path, text));
    }
    assert_no_reference(files, &["crate::network"]);
}

#[test]
fn probes_stay_out_of_the_product_path() {
    let src = app_src();
    let probes = src.join("bench/probe");
    let product: Vec<_> = sources(&src)
        .into_iter()
        .filter(|(path, _)| !path.starts_with(&probes) && !path.starts_with(src.join("bin")))
        .collect();
    assert_no_reference(product, &["bench::probe", "probe::"]);
}

#[test]
fn the_core_never_knows_the_app() {
    assert_no_reference(
        sources(&repo_root().join("apps/iot/crates/core/src")),
        &["iot_app"],
    );
}

#[test]
fn the_pixel_layer_names_no_product() {
    assert_no_reference(
        sources(&ui_src()),
        &[
            "iot_app",
            "DisplayPage",
            "Diagnostics",
            "NetworkPage",
            "vanling",
        ],
    );
}

#[test]
fn the_worker_handles_compile_without_the_radio() {
    let lib = std::fs::read_to_string(app_src().join("lib.rs")).expect("the composition root");
    let lines: Vec<_> = lib.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.trim() == "pub mod workers;")
        .expect("the composition root declares the worker handles");
    assert!(
        !lines[at.saturating_sub(1)..at]
            .iter()
            .any(|line| line.trim_start().starts_with("#[cfg")),
        "the handles are the panel/worker boundary; gating them on wifi removes them from the host build"
    );
}

#[test]
fn the_worker_handles_never_reach_a_worker() {
    assert_no_reference(
        sources(&app_src().join("workers")),
        &[
            "crate::playback",
            "crate::network",
            "crate::surface",
            "crate::frame_loop",
            "crate::input",
        ],
    );
}

#[test]
fn the_page_table_is_a_leaf() {
    assert_no_reference(
        shipping_sources(&app_src().join("page")),
        &["crate::state", "crate::render", "crate::diagnostics"],
    );
}

#[test]
fn intents_name_what_is_asked_and_not_what_answers_it() {
    assert_no_reference(
        shipping_source(app_src().join("intent.rs")),
        &["crate::page", "crate::render", "crate::diagnostics"],
    );
}

#[test]
fn state_neither_draws_nor_snapshots_itself() {
    assert_no_reference(
        shipping_sources(&app_src().join("state")),
        &["crate::render", "crate::diagnostics"],
    );
}

#[test]
fn translation_dispatches_without_naming_page_geometry() {
    assert_no_reference(
        shipping_source(app_src().join("state/translate.rs")),
        &["keyboard::", "Band::", "controls::", "Confirm"],
    );
}

#[test]
fn drivers_never_reach_up() {
    let src = core_src().join("drivers");
    let upward = &["iot_app", "DisplayPage", "Diagnostics"];
    let mut plain = Vec::new();
    let mut surface = Vec::new();
    for (path, text) in shipping_sources(&src) {
        if path == src.join("surface.rs") {
            surface.push((path, text));
        } else {
            plain.push((path, text));
        }
    }
    assert_no_reference(plain, upward);
    assert_no_reference(surface, upward);
}

#[test]
fn diagnostics_snapshots_without_drawing() {
    assert_no_reference(
        shipping_source(app_src().join("diagnostics.rs")),
        &["crate::render"],
    );
}

/// Every module in the library: a directory or a top-level file, minus the binary's own root
/// and `bin/`.
fn app_modules(src: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for entry in std::fs::read_dir(src).expect("the app's source is readable") {
        let path = entry
            .expect("the source tree does not change while it is read")
            .path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name != "bin") {
                names.insert(path.file_name().unwrap().to_string_lossy().into_owned());
            }
        } else if let Some(stem) = path.file_stem() {
            let stem = stem.to_string_lossy();
            if stem != "main" {
                names.insert(stem.into_owned());
            }
        }
    }
    names
}

fn ident_prefix(text: &str) -> &str {
    let end = text
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_'))
        .map_or(text.len(), |(at, _)| at);
    &text[..end]
}

/// The library modules a source names, written either `crate::module` or `use module::…`.
fn local_references(text: &str, modules: &BTreeSet<String>) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(at) = rest.find("crate::") {
        rest = &rest[at + "crate::".len()..];
        let ident = ident_prefix(rest);
        if modules.contains(ident) {
            found.insert(ident.to_string());
        }
    }
    for line in text.lines() {
        let line = line.trim();
        let Some(item) = line
            .strip_prefix("pub use ")
            .or_else(|| line.strip_prefix("use "))
        else {
            continue;
        };
        let ident = ident_prefix(item);
        if modules.contains(ident) {
            found.insert(ident.to_string());
        }
    }
    found
}

#[test]
fn the_app_module_graph_is_the_one_the_codemap_draws() {
    let src = app_src();
    let modules = app_modules(&src);
    let allowed: BTreeSet<(String, String)> = [
        ("bench", "input"),
        ("bench", "playback"),
        ("bench", "controls"),
        ("frame_loop", "bench"),
        ("frame_loop", "input"),
        ("frame_loop", "surface"),
        ("frame_loop", "workers"),
        ("input", "workers"),
        ("lib", "boot"),
        ("lib", "frame_loop"),
        ("lib", "input"),
        ("lib", "playback"),
        ("lib", "surface"),
        ("lib", "workers"),
        ("network", "assets"),
        ("network", "bench"),
        ("network", "boot"),
        ("network", "input"),
        ("network", "workers"),
        ("bench", "intent"),
        ("bench", "state"),
        ("boot", "state"),
        ("diagnostics", "page"),
        ("diagnostics", "state"),
        ("frame_loop", "state"),
        ("input", "intent"),
        ("input", "state"),
        ("intent", "controls"),
        ("intent", "state"),
        ("lib", "state"),
        ("network", "intent"),
        ("network", "state"),
        ("playback", "input"),
        ("playback", "intent"),
        ("playback", "state"),
        ("playback", "workers"),
        ("render", "diagnostics"),
        ("render", "page"),
        ("render", "state"),
        ("state", "controls"),
        ("state", "intent"),
        ("state", "page"),
        ("surface", "render"),
        ("surface", "state"),
        ("surface", "ui"),
        ("ui", "assets"),
        ("ui", "controls"),
        ("ui", "diagnostics"),
        ("ui", "state"),
        ("ui", "surface"),
        ("workers", "state"),
    ]
    .into_iter()
    .map(|(from, to)| (from.to_string(), to.to_string()))
    .collect();

    let mut actual = BTreeSet::new();
    for (path, text) in shipping_sources(&src) {
        let rel = path.strip_prefix(&src).expect("a source below the app");
        let first = rel
            .components()
            .next()
            .expect("a source has a path")
            .as_os_str()
            .to_string_lossy()
            .into_owned();
        let from = first.strip_suffix(".rs").unwrap_or(&first).to_string();
        if from == "main" || !modules.contains(&from) {
            continue;
        }
        for target in local_references(&text, &modules) {
            if target != from {
                actual.insert((from.clone(), target));
            }
        }
    }

    let missing: Vec<_> = actual.difference(&allowed).collect();
    let stale: Vec<_> = allowed.difference(&actual).collect();
    assert!(
        missing.is_empty(),
        "module edges not in the codemap: {missing:?}"
    );
    assert!(
        stale.is_empty(),
        "codemap edges the sources no longer form: {stale:?}"
    );
}
