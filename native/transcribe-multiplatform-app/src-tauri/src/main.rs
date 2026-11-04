// Windows: no console window behind the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    transcribe_app::entry(&args).map_or_else(
        |error| {
            eprintln!("{error}");
            ExitCode::FAILURE
        },
        |()| ExitCode::SUCCESS,
    )
}
