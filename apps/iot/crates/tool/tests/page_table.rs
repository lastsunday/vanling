//! The page manifest is the one list, and this test holds the parts the compiler cannot.
//!
//! The enum, `ALL`, `CYCLE`, `index`, `name` and `PAGES` are all generated from the `pages!`
//! rows in `core/src/page/mod.rs`, so a page cannot be half-added there. What stays outside
//! the macro is the view (one arm in `ui/src/surface.rs`, one module in `ui/src/pages/`) and
//! the ordering rule `CYCLE` relies on. This test reads the manifest as source and checks
//! those edges, so a new row that forgets its view fails here rather than on a bench.

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

struct Row {
    variant: String,
    name: String,
    cycle: bool,
    boot: bool,
    timed: bool,
}

/// The rows of the `pages!` invocation, in declaration order.
fn manifest_rows(source: &str) -> Vec<Row> {
    let start = source
        .find("pages! {")
        .expect("the manifest is the only page list");
    let body = &source[start..];
    let end = body
        .find("\n}\n")
        .expect("the macro invocation closes at column 0");
    let body = &body[..end];

    let mut rows: Vec<Row> = Vec::new();
    for line in body.lines() {
        let indented = line.strip_prefix("    ");
        let Some(indented) = indented else {
            continue;
        };
        if let Some(variant) = indented.strip_suffix(" {")
            && variant.starts_with(|first: char| first.is_ascii_uppercase())
        {
            rows.push(Row {
                variant: variant.to_string(),
                name: String::new(),
                cycle: false,
                boot: false,
                timed: false,
            });
            continue;
        }
        let Some(row) = rows.last_mut() else {
            continue;
        };
        let field = indented.trim();
        if let Some(value) = field.strip_prefix("name: \"") {
            row.name = value
                .strip_suffix("\",")
                .expect("a name is a terminated string literal")
                .to_string();
        } else if field == "cycle: true," {
            row.cycle = true;
        } else if field == "boot: true," {
            row.boot = true;
        } else if field.starts_with("timeout_ms: Some(") {
            row.timed = true;
        }
    }
    rows
}

#[test]
fn the_manifest_has_a_row_per_page() {
    let source = read("apps/iot/crates/core/src/page/mod.rs");
    let rows = manifest_rows(&source);
    assert!(
        rows.len() >= 2,
        "the manifest parser found {} rows; it is reading the wrong list",
        rows.len()
    );
    for row in &rows {
        assert!(!row.name.is_empty(), "{} has no name", row.variant);
    }
}

#[test]
fn page_names_are_unique() {
    let source = read("apps/iot/crates/core/src/page/mod.rs");
    let rows = manifest_rows(&source);
    for (position, row) in rows.iter().enumerate() {
        for other in &rows[position + 1..] {
            assert_ne!(
                row.name, other.name,
                "{} and {}",
                row.variant, other.variant
            );
        }
    }
}

#[test]
fn the_cycle_is_a_prefix_of_the_manifest() {
    let source = read("apps/iot/crates/core/src/page/mod.rs");
    let rows = manifest_rows(&source);
    let mut left_the_cycle = false;
    for row in &rows {
        if row.cycle {
            assert!(
                !left_the_cycle,
                "{} cycles but follows a page that does not; `CYCLE` is generated as a \
                 prefix of the manifest",
                row.variant
            );
        } else {
            left_the_cycle = true;
        }
    }
}

#[test]
fn a_page_that_cannot_be_booted_says_why() {
    let source = read("apps/iot/crates/core/src/page/mod.rs");
    for row in manifest_rows(&source) {
        assert!(
            row.boot || row.timed,
            "{} is not bootable and has no timeout: a page is either somewhere the device \
             may come up, or a transient with a lifetime",
            row.variant
        );
    }
}

#[test]
fn every_page_has_a_view() {
    let source = read("apps/iot/crates/core/src/page/mod.rs");
    let surface = read("apps/iot/crates/ui/src/surface.rs");
    for row in manifest_rows(&source) {
        assert!(
            surface.contains(&format!("DisplayPage::{}", row.variant)),
            "{} has no arm in ui/src/surface.rs::render; the match must stay exhaustive",
            row.variant
        );
        if row.name == "ambient" {
            continue;
        }
        let module = format!("apps/iot/crates/ui/src/pages/{}.rs", row.name);
        assert!(
            repo_root().join(&module).exists(),
            "{} names no view module at {module}",
            row.variant
        );
    }
}
