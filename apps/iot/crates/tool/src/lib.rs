//! The repository's own tools: things that read or write this source tree and
//! nothing else.
//!
//! One binary with subcommands rather than a crate per job, because adding a
//! crate to the workspace is a decision and adding a subcommand is a file. The
//! first is `midi`, which builds the sound catalogue from [`midi::RECIPES`].
//!
//! ```sh
//! cargo run -p iot-tool -- midi      # rebuild crates/bsp-esp/src/assets/*.mid
//! cargo test -p iot-tool             # the assets say what the recipes say
//! ```

pub mod midi;
