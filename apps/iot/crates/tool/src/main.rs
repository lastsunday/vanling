//! Entry point for the repository's tools.

use iot_tool::midi;

fn usage() -> String {
    format!(
        "usage: {name} <command>\n\ncommands:\n  midi    rebuild the sound catalogue's .mid files",
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
        "midi" => match midi::write(&midi::assets_dir()) {
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
