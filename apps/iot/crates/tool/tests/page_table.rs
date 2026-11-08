//! The route table is written by hand, and this test holds the parts the compiler cannot.
//!
//! `route()` is an exhaustive match, so a variant without a row does not compile, and the
//! page dispatch in `app/src/pages/mod.rs` is exhaustive too. What is *not* compiler-checked is that
//! `ALL` lists every variant, that the cycle stays a prefix of it, that a page is either
//! bootable or transient, and that a page still has a view module. This test reads the table
//! as source and checks those edges, so a half-added page fails here rather than on a bench.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .expect("a crate under apps/iot/crates is always four levels below the root")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()))
}

const PAGE: &str = "apps/iot/crates/app/src/shared/route/mod.rs";

fn source() -> String {
    read(PAGE)
}

/// The text between `start` and the first `end` after it.
fn span<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let from = source
        .find(start)
        .unwrap_or_else(|| panic!("{PAGE} has no {start:?}"))
        + start.len();
    let rest = &source[from..];
    let to = rest
        .find(end)
        .unwrap_or_else(|| panic!("{start:?} is not closed by {end:?}"));
    &rest[..to]
}

/// The identifiers after `prefix` in `text`, in order, one per item.
fn items(text: &str, prefix: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix(prefix) else {
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|first| first.is_ascii_alphanumeric() || *first == '_')
            .collect();
        assert!(!name.is_empty(), "{line:?} names nothing");
        found.push(name);
    }
    found
}

fn variants(source: &str) -> Vec<String> {
    let body = span(source, "pub enum Page {", "\n}");
    let mut variants = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        let variant = line
            .strip_suffix(',')
            .unwrap_or_else(|| panic!("{line:?} is not a variant"));
        assert!(
            variant.starts_with(|first: char| first.is_ascii_uppercase()),
            "{line:?} is not a variant"
        );
        variants.push(variant.to_string());
    }
    variants
}

/// `(variant, route const)` pairs from the exhaustive `route()` match, in order.
fn route_arms(source: &str) -> Vec<(String, String)> {
    let body = span(
        source,
        "const fn route(&self) -> &'static Route {",
        "\n    }",
    );
    let mut arms = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("Self::") else {
            continue;
        };
        let Some((variant, rest)) = rest.split_once(" => &") else {
            continue;
        };
        let arm = rest.strip_suffix(',').unwrap_or(rest);
        arms.push((variant.to_string(), arm.to_string()));
    }
    arms
}

struct Row {
    variant: String,
    path: String,
    bootable: bool,
    timed: bool,
}

/// One row per route constant, in `route()` order.
fn rows(source: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    for (variant, route) in route_arms(source) {
        let block = span(source, &format!("static {route}: Route = Route {{"), "\n};");
        let mut path = String::new();
        let mut bootable = false;
        let mut timed = false;
        for line in block.lines() {
            let field = line.trim();
            if let Some(value) = field.strip_prefix("path: \"") {
                path = value
                    .strip_suffix("\",")
                    .expect("a path is a terminated string literal")
                    .to_string();
            } else if field == "bootable: true," {
                bootable = true;
            } else if field.starts_with("timeout_ms: Some(") {
                timed = true;
            }
        }
        rows.push(Row {
            variant,
            path,
            bootable,
            timed,
        });
    }
    rows
}

#[test]
fn every_variant_has_exactly_one_route_and_one_all_entry() {
    let source = source();
    let variants = variants(&source);
    let rows = rows(&source);
    assert!(
        variants.len() >= 2,
        "the parser found {} variants; it is reading the wrong list",
        variants.len()
    );
    assert_eq!(variants.len(), rows.len(), "a variant is missing its route");

    let all = items(span(&source, "pub const ALL:", "];"), "Self::");
    assert_eq!(variants.len(), all.len(), "a variant is missing from ALL");
    for variant in &variants {
        assert_eq!(
            all.iter().filter(|listed| *listed == variant).count(),
            1,
            "{variant} is listed in ALL {} times",
            all.iter().filter(|listed| *listed == variant).count()
        );
        assert_eq!(
            rows.iter().filter(|row| row.variant == *variant).count(),
            1,
            "{variant} has more than one route arm"
        );
    }
}

#[test]
fn every_page_has_a_unique_path() {
    let rows = rows(&source());
    for row in &rows {
        assert!(!row.path.is_empty(), "{} has no path", row.variant);
    }
    for (position, row) in rows.iter().enumerate() {
        for other in &rows[position + 1..] {
            assert_ne!(
                row.path, other.path,
                "{} and {}",
                row.variant, other.variant
            );
        }
    }
}

#[test]
fn the_cycle_is_a_prefix_of_all_and_leaves_only_transients_out() {
    let source = source();
    let all = items(span(&source, "pub const ALL:", "];"), "Self::");
    let cycle = items(span(&source, "pub(crate) const CYCLE:", "];"), "Page::");
    assert!(!cycle.is_empty(), "the button would cycle through nothing");
    assert_eq!(
        &all[..cycle.len()],
        cycle.as_slice(),
        "CYCLE is not a prefix of ALL"
    );
    let rows = rows(&source);
    for (position, variant) in all.iter().enumerate() {
        if position < cycle.len() {
            continue;
        }
        let row = rows
            .iter()
            .find(|row| row.variant == *variant)
            .expect("every ALL entry has a route");
        assert!(
            row.timed,
            "{variant} is outside the cycle and has no timeout: the button could never reach \
             it and nothing would ever leave it"
        );
    }
}

#[test]
fn a_page_that_cannot_be_booted_says_why() {
    for row in rows(&source()) {
        assert!(
            row.bootable || row.timed,
            "{} is not bootable and has no timeout: a page is either somewhere the device may \
             come up, or a transient with a lifetime",
            row.variant
        );
    }
}

#[test]
fn every_page_has_a_view() {
    let dispatch = read("apps/iot/crates/app/src/pages/mod.rs");
    let page_root = repo_root().join("apps/iot/crates/app/src/pages");
    for row in rows(&source()) {
        assert!(
            dispatch.contains(&format!("Page::{}", row.variant)),
            "{} has no arm in app/src/pages/mod.rs::paint; the match must stay exhaustive",
            row.variant
        );
        let candidates = [
            page_root.join(format!("{}.rs", row.path)),
            page_root.join(&row.path).join("view.rs"),
            page_root.join(&row.path).join("view/mod.rs"),
        ];
        assert!(
            candidates.iter().any(|path| path.exists()),
            "{} names no page view next to {}",
            row.variant,
            candidates[0].display()
        );
    }
}
