//! Entry point for the repository's tools.

use std::path::Path;

use iot_tool::{font, midi};

fn usage() -> String {
    format!(
        "usage: {name} <command>\n\ncommands:\n  font    rebuild the panel's glyph table (--from <release bdf> re-cuts it)\n  midi    rebuild the sound catalogue's .mid files",
        name = env!("CARGO_PKG_NAME"),
    )
}

fn main() -> std::process::ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        eprint!("{}", usage());
        return std::process::ExitCode::FAILURE;
    };

    match command.as_str() {
        "font" => {
            let result = match args.next().as_deref() {
                None => font::write(),
                Some("--from") => match args.next() {
                    Some(path) => font::recut(Path::new(&path)),
                    None => Err("font: --from needs a BDF path".into()),
                },
                Some(other) => Err(format!("font: unknown argument {other}")),
            };
            match result {
                Ok(()) => std::process::ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("font: {e}");
                    std::process::ExitCode::FAILURE
                }
            }
        }
        "midi" => match midi::write(&midi::sounds_dir()) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("midi: {e}");
                std::process::ExitCode::FAILURE
            }
        },
        _ => {
            eprint!("{}", usage());
            std::process::ExitCode::FAILURE
        }
    }
}
