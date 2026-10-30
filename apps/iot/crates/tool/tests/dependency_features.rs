//! Features a workspace dependency has to carry for the firmware to work, checked where the
//! manifest is a file rather than something a `#[cfg]` can see.
//!
//! `edge-nal-embassy`'s own `proto-ipv4` is what lets the DHCP server address a reply. Dropping
//! it compiles clean and only fails on the device — the same shape as the environment inputs
//! `build_env.rs` guards, so the check lives beside it rather than in the crate that calls it.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .expect("a crate under apps/iot/crates is always four levels below the root")
        .to_path_buf()
}

fn manifest() -> String {
    std::fs::read_to_string(repo_root().join("apps/iot/Cargo.toml"))
        .expect("the iot workspace manifest declares every shared dependency")
}

/// The dependency's declaration, from its key through the brace that closes it.
fn declaration<'a>(manifest: &'a str, name: &str) -> &'a str {
    let mut start = None;
    let mut depth = 0i32;
    let mut offset = 0;
    for line in manifest.split_inclusive('\n') {
        if start.is_none() {
            let key = line.split('=').next().unwrap_or("").trim();
            if key == name {
                start = Some(offset);
            }
        }
        if let Some(from) = start {
            depth += line.matches('{').count() as i32;
            depth -= line.matches('}').count() as i32;
            if depth <= 0 {
                return &manifest[from..offset + line.len()];
            }
        }
        offset += line.len();
    }
    panic!("the workspace manifest does not declare {name}");
}

#[test]
fn the_dhcp_server_can_address_an_ipv4_reply() {
    let manifest = manifest();
    let declared = declaration(&manifest, "edge-nal-embassy");
    assert!(
        declared.contains("\"proto-ipv4\""),
        "edge-nal-embassy without its own proto-ipv4 cannot address a DHCP reply: {declared}"
    );
}
