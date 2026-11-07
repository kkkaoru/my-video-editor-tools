//! Drop a video, get speaker-coloured outlined subtitles for DaVinci Resolve.

pub mod captions;
pub mod catalog;
pub mod download;
pub mod drt;
pub mod engines;
pub mod error;
pub mod eta;
pub mod export;
pub mod face;
pub mod fonts;
pub mod lanes;
pub mod morphology;
pub mod paths;
pub mod pipeline;
pub mod process;
pub mod profiles;
pub mod schedule;
pub mod separation;
pub mod service;
pub mod settings;
pub mod shell;
pub mod speakers;
pub mod updater;
pub mod whisper_ggml;
pub mod whisper_seam;

use error::AppResult;
use eta::Progress;
use paths::AppPaths;
use pipeline::JobOutcome;
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
        [flag, request] if flag == separation::SEPARATE_FLAG => {
            separation::run_request(Path::new(request))
        }
        _ => window(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn no_arguments_open_the_window() {
        let opened = Cell::new(false);
        let result = entry(&[], || {
            opened.set(true);
            Ok(())
        });
        assert!(result.is_ok());
        assert!(opened.get());
    }

    #[test]
    fn the_flag_without_a_video_also_opens_the_window() {
        let opened = Cell::new(false);
        let result = entry(&[OsString::from("--headless")], || {
            opened.set(true);
            Ok(())
        });
        assert!(result.is_ok());
        assert!(opened.get());
    }

    #[test]
    fn a_window_failure_is_returned() {
        let error = entry(&[], || {
            Err(error::AppError::Invalid("no display".to_owned()))
        })
        .unwrap_err()
        .to_string();
        assert_eq!(error, "no display");
    }
}
