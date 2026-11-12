//! The module graph the compiler will not enforce.
//!
//! Rust checks that crates do not form cycles; modules in one crate may. So the directions the
//! app's codemap relies on are asserted here, where a stray `use` fails the build instead of
//! quietly reversing an edge the next reader has to rediscover.

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
    repo_root().join("apps/iot/crates/core/src/ui")
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
            // A `tests/` directory is a test module's body, the same as a `#[cfg(test)]`
            // item: it may name anything to build a fixture, so it is not part of the graph.
            if path.file_name().is_some_and(|name| name == "tests") {
                continue;
            }
            collect(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            // The same for a file the parent pulls in with `#[cfg(test)] mod tests;`: the
            // marker lives in the parent, so the child would otherwise be scanned whole.
            if path.file_name().is_some_and(|name| name == "tests.rs") {
                continue;
            }
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

/// Every `.rs` under a page, except the page dispatch and the golden tests, which speak
/// for the whole product on purpose.
fn page_sources() -> Vec<(PathBuf, String, String)> {
    let root = app_src().join("pages");
    sources(&root)
        .into_iter()
        .filter_map(|(path, text)| {
            let relative = path.strip_prefix(&root).ok()?;
            if relative == Path::new("mod.rs") || relative == Path::new("tests.rs") {
                return None;
            }
            let name = relative
                .iter()
                .next()?
                .to_str()?
                .trim_end_matches(".rs")
                .to_string();
            Some((path, name, text))
        })
        .collect()
}

#[test]
fn the_network_never_reaches_the_presentation() {
    assert_no_reference(
        sources(&app_src().join("runtime/feature/network")),
        &[
            "crate::pages",
            "crate::render",
            "crate::runtime::coordinator",
        ],
    );
}

#[test]
fn the_presentation_never_reaches_the_radio() {
    let src = app_src();
    let mut files = sources(&src.join("pages"));
    files.extend(sources(&src.join("render")));
    files.extend(sources(&src.join("runtime/feature/playback")));
    files.extend(sources(&src.join("runtime/coordinator")));
    files.extend(sources(&src.join("runtime/workers")));
    for name in ["runtime/feature/input.rs", "state/boot.rs"] {
        let path = src.join(name);
        let text = std::fs::read_to_string(&path).expect("the file is part of the app");
        files.push((path, text));
    }
    assert_no_reference(files, &["crate::runtime::feature::network"]);
}

/// Probes are the bodies of separate binaries, and the panel replay is now the same shape: only
/// `bin/panel-bench.rs` reaches it, through `bench::run`.
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

/// The bench is a harness, not a layer: `bin/panel-bench.rs` reaches `bench::run`, and nothing
/// else in the crate names it. It used to be the other way round as well — `lib` registered the
/// replay's system and the network worker asked it for a build credential — which put
/// `runtime ↔ bench` in the product path.
#[test]
fn the_bench_is_reached_only_by_its_own_entry() {
    let src = app_src();
    let bench = src.join("bench");
    let bins = src.join("bin");
    let product: Vec<_> = shipping_sources(&src)
        .into_iter()
        .filter(|(path, _)| !path.starts_with(&bench) && !path.starts_with(&bins))
        .collect();
    assert_no_reference(product, &["crate::bench", "bench::"]);
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
        &["iot_app", "Page", "Diagnostics", "NetworkDraft", "vanling"],
    );
}

#[test]
fn a_page_does_not_import_another_page() {
    let prefix = "crate::pages::";
    for (path, name, text) in page_sources() {
        let text = without_comments(&text);
        let mut rest = text.as_str();
        while let Some(at) = rest.find(prefix) {
            rest = &rest[at + prefix.len()..];
            let other: String = rest
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                .collect();
            assert!(
                other.is_empty() || other == name,
                "{} reaches crate::pages::{other}; a page's directory is its own slice",
                path.display()
            );
        }
    }
}

/// The surface paints every page, but it learns the page's ink from the page that owns it.
#[test]
fn the_kit_names_no_page() {
    assert_no_reference(
        shipping_sources(&app_src().join("shared/kit")),
        &["crate::pages", "Page", "NetworkDraft", "Entry", "SetupWait"],
    );
}

#[test]
fn the_worker_handles_compile_without_the_radio() {
    let runtime =
        std::fs::read_to_string(app_src().join("runtime/mod.rs")).expect("the runtime module");
    let lines: Vec<_> = runtime.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.trim() == "pub mod workers;")
        .expect("the runtime declares the worker handles");
    assert!(
        !lines[at.saturating_sub(1)..at]
            .iter()
            .any(|line| line.trim_start().starts_with("#[cfg")),
        "the handles are the panel/worker boundary; gating them on wifi removes them from the host build"
    );

    let feature = std::fs::read_to_string(app_src().join("runtime/feature/mod.rs"))
        .expect("the feature module");
    let lines: Vec<_> = feature.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.trim() == "pub mod network;")
        .expect("the feature module declares the network worker");
    assert!(
        lines[at.saturating_sub(1)..at]
            .iter()
            .any(|line| line.trim_start().starts_with("#[cfg")),
        "the network worker is the radio; a host build must not compile it"
    );
}

#[test]
fn the_worker_handles_never_reach_a_worker() {
    assert_no_reference(
        sources(&app_src().join("runtime/workers")),
        &[
            "crate::runtime::feature",
            "crate::pages",
            "crate::render",
            "crate::runtime::coordinator",
        ],
    );
}

#[test]
fn the_page_table_is_a_leaf() {
    assert_no_reference(
        shipping_sources(&app_src().join("shared/route")),
        &[
            "crate::state",
            "crate::render",
            "crate::shared::diagnostics",
            "crate::pages",
        ],
    );
}

#[test]
fn intents_name_what_is_asked_and_not_what_answers_it() {
    assert_no_reference(
        shipping_sources(&app_src().join("shared/intent")),
        &[
            "crate::shared::route",
            "crate::render",
            "crate::shared::diagnostics",
            "crate::pages",
            "crate::state",
        ],
    );
}

#[test]
fn state_neither_draws_nor_snapshots_itself() {
    assert_no_reference(
        shipping_sources(&app_src().join("state")),
        &["crate::render", "crate::shared::diagnostics"],
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
    // The bare word names core's own `PageSurface` and `SurfaceRole::Pages`; a product page
    // can only reach a driver by naming a variant, so that is the token to forbid.
    let upward = &["iot_app", "Page::", "Diagnostics"];
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
        shipping_source(app_src().join("shared/diagnostics.rs")),
        &["crate::render"],
    );
}

/// The shared half is the leaf: it holds the records every layer reads, so it may not name a
/// layer that reads it. `DeviceState` and the Network draft live here for exactly this
/// reason — when they lived in `state`, the page and the snapshot had to reach back up.
#[test]
fn shared_never_reaches_up() {
    assert_no_reference(
        shipping_sources(&app_src().join("shared")),
        &[
            "crate::state",
            "crate::pages",
            "crate::render",
            "crate::runtime",
        ],
    );
}

/// A sixth bucket is a decision this test asks a reader to make on purpose.
#[test]
fn the_shared_buckets_are_the_five_the_taxonomy_names() {
    let shared = app_src().join("shared");
    let mut found: Vec<String> = std::fs::read_dir(&shared)
        .expect("the shared directory is readable")
        .map(|entry| {
            entry
                .expect("the source tree does not change while it is read")
                .path()
                .file_name()
                .and_then(|name| name.to_str())
                .expect("every entry is UTF-8")
                .trim_end_matches(".rs")
                .to_string()
        })
        .filter(|name| name != "mod")
        .collect();
    found.sort();
    assert_eq!(
        found,
        ["diagnostics", "intent", "kit", "model", "route"],
        "shared/ is one bucket per change reason; a new one must say why it is not a page's"
    );
}

/// A bucket with one reader is a page's private vocabulary; `bench`/`bin` do not count.
#[test]
fn every_shared_bucket_has_two_product_readers() {
    let src = app_src();
    let shared = src.join("shared");
    let readers: Vec<_> = shipping_sources(&src)
        .into_iter()
        .filter(|(path, _)| {
            !path.starts_with(src.join("bench")) && !path.starts_with(src.join("bin"))
        })
        .collect();
    for bucket in ["diagnostics", "intent", "kit", "model", "route"] {
        let dir = shared.join(bucket);
        let file = shared.join(format!("{bucket}.rs"));
        let own = if dir.is_dir() { dir } else { file };
        let needle = format!("shared::{bucket}");
        let count = readers
            .iter()
            .filter(|(path, text)| !path.starts_with(&own) && text.contains(&needle))
            .count();
        assert!(
            count >= 2,
            "shared/{bucket} has {count} product reader(s); a bucket needs at least two"
        );
    }
}

/// A page draws and translates; it does not run the device. Naming the state layer would put
/// the aggregate back above it, which is the cycle the split removed.
#[test]
fn pages_never_reach_up() {
    assert_no_reference(
        shipping_sources(&app_src().join("pages")),
        &["crate::state", "crate::render", "crate::runtime"],
    );
}

/// Render reads the shared records and calls the page painter. The coordinator owns the frame
/// order and the manager, so neither belongs to the view.
#[test]
fn render_never_reaches_up() {
    assert_no_reference(
        shipping_sources(&app_src().join("render")),
        &["crate::state", "crate::runtime"],
    );
}

/// A lamp paints one colour. Anything a panel adds on top of that colour, or a page to paint
/// at all, means the lamp slice reached into the panel instead of staying on its own side of
/// the split.
#[test]
fn the_lamp_never_reaches_the_panel() {
    assert_no_reference(
        shipping_sources(&app_src().join("render/lamp")),
        &[
            "crate::pages",
            "crate::render::screen",
            "policy",
            "painter",
            "repaint",
            "camera",
        ],
    );
}

/// The shared half is shared: if it named a surface, the two renderers would no longer stand
/// on the same bucket.
#[test]
fn what_every_surface_shares_knows_no_surface() {
    assert_no_reference(
        shipping_sources(&app_src().join("render/common")),
        &[
            "crate::render::screen",
            "crate::render::lamp",
            "crate::pages",
        ],
    );
}
