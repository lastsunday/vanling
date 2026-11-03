//! The component feature matrix is written twice: once as `[features]` in
//! `bsp-esp/Cargo.toml` and once as the arrays in `scripts/iot-feature-check.sh`.
//!
//! `moon run iot:check-features` only builds what the script names, so a component left out
//! of the script is never built on its own — and a board feature is by construction the union
//! of what that board wires, so nothing else notices. That is the blind spot the script was
//! written to close (`screen` importing the backlight). This test keeps the two lists equal:
//! adding a component feature without listing it fails here, before the matrix silently
//! stops covering it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .expect("a crate under apps/iot/crates is always four levels below the root")
        .to_path_buf()
}

fn manifest_features(manifest: &str) -> Vec<(String, Vec<String>)> {
    let mut features = Vec::new();
    let mut in_features = false;
    let mut pending: Option<(String, String)> = None;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if let Some((key, mut value)) = pending.take() {
            value.push(' ');
            value.push_str(trimmed);
            if trimmed.contains(']') {
                features.push((key, split_list(&value)));
            } else {
                pending = Some((key, value));
            }
            continue;
        }
        if trimmed.starts_with('[') {
            in_features = trimmed == "[features]";
            continue;
        }
        if !in_features || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim().to_string();
        let value = value.trim();
        if value.starts_with('[') && !value.contains(']') {
            pending = Some((key, value.to_string()));
        } else {
            features.push((key, split_list(value)));
        }
    }
    features
}

fn split_list(value: &str) -> Vec<String> {
    value
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|item| item.trim().trim_matches('"').to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

/// The first token of every entry in the script's three component arrays. A chip-qualified
/// entry carries its feature set after a comma (`wifi,esp-hal/esp32s3,…`); the component is
/// the part before it.
fn script_components(script: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for variable in ["HOST_COMPONENTS", "CHIP_COMPONENTS", "C6_COMPONENTS"] {
        let open = format!("{variable}=(");
        let mut collecting = false;
        let mut body = String::new();
        for line in script.lines() {
            let trimmed = line.trim();
            if !collecting {
                if let Some(rest) = trimmed.strip_prefix(&open) {
                    collecting = true;
                    body.push_str(rest);
                } else {
                    continue;
                }
            } else if !trimmed.starts_with('#') {
                body.push(' ');
                body.push_str(trimmed);
            }
            if collecting && trimmed.contains(')') {
                collecting = false;
                for token in body.split(|c: char| c.is_whitespace() || c == '"' || c == ')') {
                    let first = token.split(',').next().unwrap_or("").trim();
                    if !first.is_empty() && !first.starts_with('#') {
                        names.insert(first.to_string());
                    }
                }
                body.clear();
            }
        }
    }
    names
}

#[test]
fn every_component_feature_is_built_alone_by_the_matrix() {
    let bsp = std::fs::read_to_string(repo_root().join("apps/iot/crates/bsp-esp/Cargo.toml"))
        .expect("the bsp manifest declares the component features");
    let script = std::fs::read_to_string(repo_root().join("scripts/iot-feature-check.sh"))
        .expect("the matrix script is part of the repository");

    let features = manifest_features(&bsp);
    // A board feature is where the per-chip radio blob is contributed; everything else that
    // is not `default` is a component or a virtual component.
    let boards: BTreeSet<String> = features
        .iter()
        .filter(|(_, deps)| deps.iter().any(|dep| dep.starts_with("esp-radio/")))
        .map(|(name, _)| name.clone())
        .collect();
    assert!(
        !boards.is_empty(),
        "no board feature was found; the marker for one moved"
    );
    let declared: BTreeSet<String> = features
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| name != "default" && !boards.contains(name))
        .collect();

    let scripted = script_components(&script);

    let missing: Vec<_> = declared.difference(&scripted).collect();
    let extra: Vec<_> = scripted.difference(&declared).collect();
    assert!(
        missing.is_empty(),
        "component features declared in bsp-esp/Cargo.toml but never built alone by \
         scripts/iot-feature-check.sh: {missing:?}"
    );
    assert!(
        extra.is_empty(),
        "the matrix builds features bsp-esp/Cargo.toml does not declare: {extra:?}"
    );
}
