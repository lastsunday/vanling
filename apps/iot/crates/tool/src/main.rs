//! Entry point for the repository's tools.

use std::path::Path;

use iot_tool::{font, midi, outline};

fn usage() -> String {
    format!(
        "usage: {name} <command>\n\ncommands:\n  font    rebuild the panel's glyph table (--from <bdf> re-cuts; --merge <bdf> --chars <list> adds)\n  outline rebuild the panel's vector table from the release OTF\n  midi    rebuild the sound catalogue's .mid files",
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
                Some("--merge") => match (args.next(), args.next().as_deref(), args.next()) {
                    (Some(bdf), Some("--chars"), Some(chars)) => {
                        let allow_missing = args.next().as_deref() == Some("--allow-missing");
                        font::merge(Path::new(&bdf), Path::new(&chars), allow_missing)
                    }
                    _ => Err("font: --merge needs a BDF and --chars <list>".into()),
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
        "outline" => match outline::write() {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("outline: {e}");
                std::process::ExitCode::FAILURE
            }
        },
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
