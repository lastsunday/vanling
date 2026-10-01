//! Copies the chip-specific linker script fragments into `OUT_DIR` so rustc can
//! find them with `-C link-arg=-T<name>`, and adds the `-L` search path.
//!
//! `-C link-arg` in `.cargo/config.toml` only passes the script name, so the
//! file has to be somewhere the linker searches. Only the S3 board takes part
//! in this today; the other chips link no extra script.

use std::env;
use std::fs;
use std::path::PathBuf;

const SCRIPTS: &[&str] = &["esp32s3-main-stack.x"];

fn main() {
    println!("cargo:rerun-if-changed=linker");
    for script in SCRIPTS {
        println!("cargo:rerun-if-changed=linker/{script}");
    }

    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("xtensa") {
        return;
    }

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    for script in SCRIPTS {
        let destination = out.join(script);
        fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("linker")
                .join(script),
            &destination,
        )
        .unwrap_or_else(|error| panic!("copying linker/{script} into OUT_DIR: {error}"));
    }
    println!("cargo:rustc-link-search={}", out.display());
}
