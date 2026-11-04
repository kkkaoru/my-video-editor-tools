//! Drop a video, get speaker-coloured outlined subtitles for DaVinci Resolve.

pub mod captions;
pub mod catalog;
pub mod download;
pub mod engines;
pub mod error;
pub mod export;
pub mod face;
pub mod paths;
pub mod pipeline;
pub mod process;
pub mod service;
pub mod settings;
pub mod shell;
pub mod whisper_ggml;

use error::AppResult;
use paths::AppPaths;
use pipeline::{JobOutcome, Progress};
use process::CancelToken;
use settings::Settings;
use std::ffi::OsString;
use std::path::Path;

const HEADLESS_FLAG: &str = "--headless";

/// `--headless VIDEO`: the same pipeline without a window, for smoke tests and CI.
pub fn run_headless(video: &Path) -> AppResult<JobOutcome> {
    let paths = AppPaths::detect()?;
    let settings = Settings::load(&paths.settings_file())?;
    let hardware = catalog::detect_hardware()?;
    let mut sink = |progress: Progress| {
        if let Ok(line) = serde_json::to_string(&progress) {
            println!("{line}");
        }
    };
    pipeline::run_job(
        video,
        &paths,
        &settings,
        &hardware,
        &CancelToken::default(),
        &mut sink,
    )
}

fn print_outcome(outcome: &JobOutcome) -> AppResult<()> {
    let line = serde_json::to_string(outcome)
        .map_err(|error| error::AppError::Invalid(error.to_string()))?;
    println!("{line}");
    Ok(())
}

/// Runs the requested mode: headless with the flag and a video, otherwise `window`.
pub fn entry(args: &[OsString], window: impl FnOnce() -> AppResult<()>) -> AppResult<()> {
    match args {
        [flag, video] if flag == HEADLESS_FLAG => print_outcome(&run_headless(Path::new(video))?),
        _ => window(),
    }
}
