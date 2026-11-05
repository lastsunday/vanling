//! The module graph the compiler will not enforce.
//!
//! Rust checks that crates do not form cycles; modules in one crate may. So the directions the
//! app's codemap relies on are asserted here, where a stray `use` fails the build instead of
//! quietly reversing an edge the next reader has to rediscover.

use std::collections::{BTreeMap, BTreeSet};
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
        sources(&app_src().join("runtime/feature/network")),
        &["crate::display", "crate::runtime::coordinator"],
    );
}

#[test]
fn the_panel_side_never_reaches_the_radio() {
    let src = app_src();
    let mut files = sources(&src.join("display"));
    files.extend(sources(&src.join("runtime/feature/playback")));
    files.extend(sources(&src.join("runtime/coordinator")));
    files.extend(sources(&src.join("runtime/workers")));
    for name in ["runtime/feature/input.rs", "model/boot.rs"] {
        let path = src.join(name);
        let text = std::fs::read_to_string(&path).expect("the file is part of the app");
        files.push((path, text));
    }
    assert_no_reference(files, &["crate::runtime::feature::network"]);
}

/// Probes are the bodies of separate binaries. The panel replay under `bench` is reached by the
/// product loop, so it is not one of them and is not filtered out here.
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
            "crate::display",
            "crate::runtime::coordinator",
        ],
    );
}

#[test]
fn the_page_table_is_a_leaf() {
    assert_no_reference(
        shipping_sources(&app_src().join("model/route")),
        &[
            "crate::model::state",
            "crate::display::render",
            "crate::display::diagnostics",
        ],
    );
}

#[test]
fn intents_name_what_is_asked_and_not_what_answers_it() {
    assert_no_reference(
        shipping_source(app_src().join("model/intent.rs")),
        &[
            "crate::model::route",
            "crate::display::render",
            "crate::display::diagnostics",
        ],
    );
}

#[test]
fn state_neither_draws_nor_snapshots_itself() {
    assert_no_reference(
        shipping_sources(&app_src().join("model/state")),
        &["crate::display::render", "crate::display::diagnostics"],
    );
}

#[test]
fn translation_dispatches_without_naming_page_geometry() {
    assert_no_reference(
        shipping_source(app_src().join("model/state/translate.rs")),
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
        shipping_source(app_src().join("display/diagnostics.rs")),
        &["crate::display::render"],
    );
}

/// The three layers: each one's direct children are nodes of the graph, and the rule between
/// the layers is the point of drawing it at all.
const LAYERS: [&str; 3] = ["model", "display", "runtime"];

/// Every module the library declares, by logical path (`model::state`), with the file that
/// defines it. `main.rs` and `bin/` are separate crates, so they are not in the tree; the
/// library root is named `lib`.
fn module_tree(src: &Path) -> BTreeMap<String, PathBuf> {
    let mut modules = BTreeMap::new();
    collect_modules(src, "", &mut modules);
    modules
}

fn collect_modules(dir: &Path, prefix: &str, modules: &mut BTreeMap<String, PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("every source directory is readable") {
        let path = entry
            .expect("the source tree does not change while it is read")
            .path();
        if path.is_dir() {
            let name = path
                .file_name()
                .expect("a directory has a name")
                .to_string_lossy();
            if name == "bin" {
                continue;
            }
            // A directory with no `mod.rs` holds no modules; `assets/` is bytes.
            let owner = path.join("mod.rs");
            if owner.is_file() {
                let module = join(prefix, &name);
                modules.insert(module.clone(), owner);
                collect_modules(&path, &module, modules);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let Some(stem) = path.file_stem() else {
                continue;
            };
            let stem = stem.to_string_lossy();
            if matches!(&*stem, "lib" | "main" | "mod") {
                continue;
            }
            modules.insert(join(prefix, &stem), path);
        }
    }
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}::{name}")
    }
}

/// A layer's grouped children are nodes too: grouping features under one directory must not
/// flatten their edges into a single node.
const GROUPS: [&str; 1] = ["runtime::feature"];

/// A node is a top-level module or a layer's direct child. Deeper modules belong to the node
/// that contains them, so moving a file inside a node does not redraw the graph.
fn is_node(module: &str) -> bool {
    let parts: Vec<&str> = module.split("::").collect();
    match parts.as_slice() {
        [first, ..] if first.is_empty() => false,
        [_] => true,
        [first, _] => LAYERS.contains(first) || GROUPS.contains(&module),
        [first, second, _] => GROUPS
            .iter()
            .any(|group| format!("{first}::{second}") == *group),
        _ => false,
    }
}

/// The node a module belongs to: the longest node path it starts with.
fn node_of(module: &str, nodes: &BTreeSet<String>) -> Option<String> {
    let mut end = module.len();
    loop {
        if nodes.contains(&module[..end]) {
            return Some(module[..end].to_string());
        }
        end = module[..end].rfind("::")?;
    }
}

fn ident_prefix(text: &str) -> &str {
    let end = text
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_'))
        .map_or(text.len(), |(at, _)| at);
    &text[..end]
}

/// The identifiers in a `a::b::c` path, starting at `text`.
fn segments(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = text;
    loop {
        let end = ident_prefix(rest).len();
        if end == 0 {
            return found;
        }
        found.push(&rest[..end]);
        match rest[end..].strip_prefix("::") {
            Some(tail) => rest = tail,
            None => return found,
        }
    }
}

/// Every `crate::a::b` written anywhere in the source, as a path.
fn crate_paths(text: &str) -> Vec<Vec<&str>> {
    const PREFIX: &str = "crate::";
    let mut paths = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(PREFIX) {
        rest = &rest[at + PREFIX.len()..];
        paths.push(segments(rest));
    }
    paths
}

/// The path every `use` names before its braces: `use super::{A, B}` names `super`.
fn use_paths(text: &str) -> Vec<Vec<&str>> {
    let mut paths = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(item) = line
            .strip_prefix("pub use ")
            .or_else(|| line.strip_prefix("use "))
        else {
            continue;
        };
        let end = item
            .char_indices()
            .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_' || *c == ':'))
            .map_or(item.len(), |(at, _)| at);
        let mut path = &item[..end];
        while let Some(trimmed) = path.strip_suffix("::") {
            path = trimmed;
        }
        if !path.is_empty() {
            paths.push(path.split("::").collect());
        }
    }
    paths
}

/// Where a path written in `module` lands: `crate::` from the root, `self`/`super` by walking,
/// and a bare first segment as Rust 2018's uniform path — the module's own child when it has
/// one, otherwise a top-level module. `None` for a path that names no module of this crate,
/// which is every `iot_core::`/`alloc::` import.
fn resolve(path: &[&str], module: &str, modules: &BTreeMap<String, PathBuf>) -> Option<String> {
    let mut base: Vec<&str> = module.split("::").filter(|part| !part.is_empty()).collect();
    let mut rest = path;
    match *path.first()? {
        "crate" => {
            base.clear();
            rest = &path[1..];
        }
        "self" => rest = &path[1..],
        "super" => {
            while let Some(&"super") = rest.first() {
                base.pop();
                rest = &rest[1..];
            }
        }
        first => {
            let child = join(module, first);
            if modules.contains_key(&child) {
                base.push(first);
                rest = &path[1..];
            } else if modules.contains_key(first) {
                base.clear();
                base.push(first);
                rest = &path[1..];
            } else {
                return None;
            }
        }
    }
    base.extend_from_slice(rest);
    let full = base.join("::");
    let mut end = full.len();
    loop {
        if modules.contains_key(&full[..end]) {
            return Some(full[..end].to_string());
        }
        end = full[..end].rfind("::")?;
    }
}

/// The library modules a source names, as logical module paths.
fn local_references(
    text: &str,
    module: &str,
    modules: &BTreeMap<String, PathBuf>,
) -> BTreeSet<String> {
    crate_paths(text)
        .into_iter()
        .chain(use_paths(text))
        .filter_map(|path| resolve(&path, module, modules))
        .collect()
}

/// The edges the sources form between the codemap's nodes.
fn app_edges(src: &Path) -> BTreeSet<(String, String)> {
    let modules = module_tree(src);
    let nodes: BTreeSet<String> = modules
        .keys()
        .filter(|module| is_node(module))
        .cloned()
        .collect();
    let owner: BTreeMap<&Path, &str> = modules
        .iter()
        .map(|(module, path)| (path.as_path(), module.as_str()))
        .collect();
    let mut edges = BTreeSet::new();
    for (path, text) in shipping_sources(src) {
        // `lib.rs` is the root; `main.rs` and the bins are other crates and own their edges.
        let module = match owner.get(path.as_path()) {
            Some(module) => module.to_string(),
            None if path == src.join("lib.rs") => String::new(),
            None => continue,
        };
        let from = if module.is_empty() {
            "lib".to_string()
        } else {
            node_of(&module, &nodes)
                .unwrap_or_else(|| panic!("{} belongs to no codemap node", path.display()))
        };
        for target in local_references(&text, &module, &modules) {
            let Some(to) = node_of(&target, &nodes) else {
                panic!(
                    "{} reaches {target}, which belongs to no codemap node",
                    path.display()
                );
            };
            if to != from {
                edges.insert((from.clone(), to));
            }
        }
    }
    edges
}

#[test]
fn the_app_module_graph_is_the_one_the_codemap_draws() {
    let allowed: BTreeSet<(String, String)> = [
        ("bench", "controls"),
        ("bench", "model::boot"),
        ("bench", "model::intent"),
        ("bench", "model::state"),
        ("bench", "runtime::feature::input"),
        ("bench", "runtime::feature::playback"),
        ("bench", "runtime::system"),
        ("display", "display::render"),
        ("display", "display::ui"),
        ("display", "model::state"),
        ("display::diagnostics", "model::route"),
        ("display::diagnostics", "model::state"),
        ("display::render", "display::diagnostics"),
        ("display::render", "model::route"),
        ("display::render", "model::state"),
        ("display::ui", "assets"),
        ("display::ui", "controls"),
        ("display::ui", "display::diagnostics"),
        ("display::ui", "model::state"),
        ("lib", "bench"),
        ("lib", "display"),
        ("lib", "model::boot"),
        ("lib", "model::state"),
        ("lib", "runtime::coordinator"),
        ("lib", "runtime::feature::input"),
        ("lib", "runtime::feature::playback"),
        ("lib", "runtime::workers"),
        ("model::boot", "model::state"),
        ("model::intent", "controls"),
        ("model::intent", "model::state"),
        ("model::state", "controls"),
        ("model::state", "model::intent"),
        ("model::state", "model::route"),
        ("runtime::coordinator", "display"),
        ("runtime::coordinator", "model::state"),
        ("runtime::coordinator", "runtime::feature::input"),
        ("runtime::coordinator", "runtime::system"),
        ("runtime::coordinator", "runtime::workers"),
        ("runtime::feature::input", "model::intent"),
        ("runtime::feature::input", "model::state"),
        ("runtime::feature::input", "runtime::workers"),
        ("runtime::feature::network", "assets"),
        ("runtime::feature::network", "bench"),
        ("runtime::feature::network", "model::intent"),
        ("runtime::feature::network", "model::state"),
        ("runtime::feature::network", "runtime::feature::input"),
        ("runtime::feature::network", "runtime::workers"),
        ("runtime::feature::playback", "model::intent"),
        ("runtime::feature::playback", "model::state"),
        ("runtime::feature::playback", "runtime::feature::input"),
        ("runtime::feature::playback", "runtime::workers"),
        ("runtime::system", "model::state"),
        ("runtime::system", "runtime::feature::input"),
        ("runtime::workers", "model::state"),
    ]
    .into_iter()
    .map(|(from, to)| (from.to_string(), to.to_string()))
    .collect();

    let actual = app_edges(&app_src());
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

/// The layers lean inward only: the model knows neither the runtime nor the display, and the
/// display never drives a worker. The one edge outward from the runtime is the frame loop
/// presenting what the model settled on.
#[test]
fn the_layers_lean_inward_only() {
    for (from, to) in app_edges(&app_src()) {
        let outward = if from.starts_with("model::") {
            to.starts_with("display") || to.starts_with("runtime")
        } else if from.starts_with("display") {
            to.starts_with("runtime")
        } else {
            false
        };
        assert!(!outward, "{from} reaches {to}; a layer only leans inward");
    }
}

/// The top of the tree is the three layers, the two leaves they share, and the entries.
#[test]
fn the_app_tree_is_three_layers_two_leaves_and_the_entries() {
    let src = app_src();
    let mut top: BTreeSet<String> = module_tree(&src)
        .keys()
        .filter(|module| !module.contains("::"))
        .cloned()
        .collect();
    top.insert("bin".to_string());
    top.insert("main".to_string());
    top.insert("lib".to_string());
    let expected: BTreeSet<String> = [
        "assets", "bench", "bin", "controls", "display", "lib", "main", "model", "runtime",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(top, expected, "the app's top level moved");
}
