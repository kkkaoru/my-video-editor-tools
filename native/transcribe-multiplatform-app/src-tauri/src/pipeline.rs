//! One dropped video to finished subtitle files, with staged progress.

use crate::captions::{assign_speakers, build_cues, metrics_for};
use crate::catalog::{self, Hardware, ModelEntry};
use crate::engines::{self, DiarizeModel, Tools};
use crate::error::{io_context, AppError, AppResult, UserError};
use crate::export::{unused_base, write_outputs, Transcript};
use crate::face::{scan_faces, FaceDetector, FaceModelKind, FaceSample};
use crate::paths::AppPaths;
use crate::process::CancelToken;
use crate::settings::{Anchor, Settings};
use crate::whisper_ggml;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Prepare,
    ConvertModel,
    ExtractAudio,
    Transcribe,
    Diarize,
    Faces,
    Export,
    Done,
}

/// Rough share of the bar per stage. Transcription and speakers dominate on every machine.
const STAGE_SPANS: [(Stage, f64, f64); 7] = [
    (Stage::Prepare, 0.0, 0.01),
    (Stage::ConvertModel, 0.01, 0.05),
    (Stage::ExtractAudio, 0.05, 0.08),
    (Stage::Transcribe, 0.08, 0.60),
    (Stage::Diarize, 0.60, 0.90),
    (Stage::Faces, 0.90, 0.98),
    (Stage::Export, 0.98, 1.0),
];

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Progress {
    pub stage: Stage,
    /// Whole job, 0 to 1.
    pub overall: f64,
    /// `false` while a tool reports no percentage: the UI shows motion and elapsed time.
    pub determinate: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct JobOutcome {
    pub outputs: Vec<PathBuf>,
    pub base: PathBuf,
    pub speaker_count: u32,
    pub cue_count: usize,
    pub faceless: bool,
}

/// Removes the job's working folder (its large WAV) however the job ends.
struct WorkDir(PathBuf);

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn overall(stage: Stage, fraction: f64) -> f64 {
    let (low, high) = STAGE_SPANS
        .iter()
        .find(|(name, _, _)| *name == stage)
        .map_or((1.0, 1.0), |span| (span.1, span.2));
    low + (high - low) * fraction.clamp(0.0, 1.0)
}

fn report(stage: Stage, fraction: f64, sink: &mut dyn FnMut(Progress)) {
    sink(Progress {
        stage,
        overall: overall(stage, fraction),
        determinate: true,
    });
}

fn selected_entry(
    id: &str,
    fallback: Option<&'static str>,
    role: &str,
) -> AppResult<&'static ModelEntry> {
    let chosen = if id.is_empty() { fallback } else { Some(id) };
    let chosen = chosen.ok_or_else(|| UserError::NoModelForRole {
        role: role.to_owned(),
    })?;
    catalog::find_entry(chosen)
        .ok_or_else(|| AppError::Invalid(format!("unknown {role} model {chosen}")))
}

fn whisper_model(
    paths: &AppPaths,
    settings: &Settings,
    sink: &mut dyn FnMut(Progress),
) -> AppResult<PathBuf> {
    if let Some(ready) = catalog::locate_whisper_ggml(paths, &settings.model_dirs) {
        return Ok(ready);
    }
    let source = catalog::locate_whisper_source(paths, &settings.model_dirs).ok_or_else(|| {
        UserError::ModelMissing {
            model: catalog::WHISPER_ID.to_owned(),
        }
    })?;
    let folder = paths.models_dir().join(catalog::WHISPER_ID);
    std::fs::create_dir_all(&folder).map_err(io_context("create model folder"))?;
    let output = folder.join(catalog::WHISPER_GGML_NAME);
    whisper_ggml::convert(&source, &output, |fraction| {
        report(Stage::ConvertModel, fraction, sink)
    })?;
    Ok(output)
}

fn check_fits(entry: &ModelEntry, hardware: &Hardware) -> AppResult<()> {
    catalog::blocked_reason(entry, hardware).map_or(Ok(()), |reason| Err(reason.into()))
}

fn locate_faces(
    tools: &Tools,
    paths: &AppPaths,
    settings: &Settings,
    video: &Path,
    info: &engines::VideoInfo,
    cancel: &CancelToken,
    sink: &mut dyn FnMut(Progress),
) -> AppResult<Vec<FaceSample>> {
    let entry = catalog::find_entry(&settings.face_model)
        .ok_or_else(|| AppError::Invalid(format!("unknown face model {}", settings.face_model)))?;
    let model = catalog::locate(entry, paths, &settings.model_dirs).ok_or_else(|| {
        UserError::ModelMissing {
            model: entry.id.to_owned(),
        }
    })?;
    let kind = if entry.id == catalog::AVATAR_YOLO_ID {
        FaceModelKind::AvatarYolo
    } else {
        FaceModelKind::Yunet
    };
    let detector = FaceDetector::load(&model, kind)?;
    scan_faces(tools, &detector, video, info, cancel, &mut |fraction| {
        report(Stage::Faces, fraction, sink)
    })
}

fn wants_faces(settings: &Settings) -> bool {
    matches!(
        settings.placement.anchor,
        Anchor::FaceAbove | Anchor::FaceBelow
    )
}

fn job_folder(paths: &AppPaths) -> AppResult<WorkDir> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let folder = paths
        .jobs_dir()
        .join(format!("{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&folder).map_err(io_context("create job folder"))?;
    Ok(WorkDir(folder))
}

/// Runs the whole job. Models that are missing or too large fail before any long work starts.
pub fn run_job(
    video: &Path,
    paths: &AppPaths,
    settings: &Settings,
    hardware: &Hardware,
    cancel: &CancelToken,
    sink: &mut dyn FnMut(Progress),
) -> AppResult<JobOutcome> {
    settings.validate()?;
    report(Stage::Prepare, 0.0, sink);
    let rows = catalog::statuses(hardware, paths, &settings.model_dirs);
    let diarize_entry = selected_entry(
        &settings.diarize_model,
        catalog::default_diarize_model(hardware, &rows),
        "diarize",
    )?;
    check_fits(diarize_entry, hardware)?;
    let main_file =
        catalog::locate(diarize_entry, paths, &settings.model_dirs).ok_or_else(|| {
            UserError::ModelMissing {
                model: diarize_entry.id.to_owned(),
            }
        })?;
    let encoder = catalog::speech_llm_encoder(&settings.model_dirs, paths);
    let use_cuda = hardware.nvidia_name.is_some()
        && catalog::find_entry(catalog::GEFORCE_RUNTIME_ID)
            .is_some_and(|entry| catalog::locate(entry, paths, &[]).is_some());
    let tools = Tools::locate(paths, use_cuda)?;
    let info = engines::probe(&tools, video)?;
    let whisper = whisper_model(paths, settings, sink)?;
    cancel.check()?;

    let work = job_folder(paths)?;
    let wav = work.0.join("audio.wav");
    engines::extract_audio(
        &tools,
        video,
        &wav,
        info.duration,
        cancel,
        &mut |fraction| report(Stage::ExtractAudio, fraction, sink),
    )?;
    let mut words = engines::transcribe(
        &tools,
        &whisper,
        &wav,
        &settings.language,
        &work.0,
        cancel,
        &mut |fraction| {
            report(Stage::Transcribe, fraction, sink);
        },
    )?;
    if words.is_empty() {
        return Err(UserError::NoWords.into());
    }
    sink(Progress {
        stage: Stage::Diarize,
        overall: overall(Stage::Diarize, 0.0),
        determinate: false,
    });
    let model = DiarizeModel {
        entry: diarize_entry,
        main_file: &main_file,
        encoder: encoder.as_deref(),
    };
    let turns = engines::diarize(&tools, &model, &wav, &settings.language, &work.0, cancel)?;
    assign_speakers(&mut words, &turns);
    // Faces are always scanned when wanted now; a later re-layout can reuse them.
    let faces = if wants_faces(settings) {
        locate_faces(&tools, paths, settings, video, &info, cancel, sink)?
    } else {
        Vec::new()
    };
    report(Stage::Export, 0.0, sink);
    let (cues, faceless) = build_cues(&words, &faces, &info, settings);
    let transcript = Transcript::new(
        video.to_path_buf(),
        info,
        metrics_for(&info, settings),
        words,
        faces,
        cues,
    );
    let base = unused_base(video);
    let outputs = write_outputs(&base, &transcript, settings)?;
    report(Stage::Done, 1.0, sink);
    Ok(JobOutcome {
        outputs,
        base,
        speaker_count: transcript.speaker_count,
        cue_count: transcript.cues.len(),
        faceless,
    })
}

/// Lays out an earlier transcript again with new colours or position, without the speech models.
pub fn relayout(
    transcript_file: &Path,
    paths: &AppPaths,
    settings: &Settings,
    cancel: &CancelToken,
    sink: &mut dyn FnMut(Progress),
) -> AppResult<JobOutcome> {
    settings.validate()?;
    let mut transcript = Transcript::load(transcript_file)?;
    if wants_faces(settings) && transcript.faces.is_empty() {
        let tools = Tools::locate(paths, false)?;
        transcript.faces = locate_faces(
            &tools,
            paths,
            settings,
            &transcript.source,
            &transcript.info,
            cancel,
            sink,
        )?;
    }
    report(Stage::Export, 0.0, sink);
    let (cues, faceless) = build_cues(
        &transcript.words,
        &transcript.faces,
        &transcript.info,
        settings,
    );
    transcript.metrics = metrics_for(&transcript.info, settings);
    transcript.cues = cues;
    let name = transcript_file.to_string_lossy();
    let base = PathBuf::from(name.strip_suffix(".transcript.json").unwrap_or(&name));
    let outputs = write_outputs(&base, &transcript, settings)?;
    report(Stage::Done, 1.0, sink);
    Ok(JobOutcome {
        outputs,
        base,
        speaker_count: transcript.speaker_count,
        cue_count: transcript.cues.len(),
        faceless,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_cover_the_bar_in_order() {
        assert_eq!(overall(Stage::Prepare, 0.0), 0.0);
        assert_eq!(overall(Stage::Transcribe, 0.5), 0.34);
        assert_eq!(overall(Stage::Export, 1.0), 1.0);
        assert_eq!(overall(Stage::Done, 0.0), 1.0);
    }

    #[test]
    fn fractions_outside_the_range_are_clamped() {
        assert_eq!(overall(Stage::Faces, 9.0), 0.98);
        assert_eq!(overall(Stage::Faces, -1.0), 0.9);
    }

    #[test]
    fn no_installed_model_names_the_role() {
        let error = selected_entry("", None, "diarize").unwrap_err();
        assert_eq!(error.to_string(), "no model is installed for diarize");
    }

    #[test]
    fn an_unknown_model_is_an_error() {
        let error = selected_entry("nope", None, "diarize")
            .unwrap_err()
            .to_string();
        assert_eq!(error, "unknown diarize model nope");
    }

    #[test]
    fn work_folder_is_removed_on_drop() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target-test-workdir");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        drop(WorkDir(root.clone()));
        assert!(!root.exists());
    }
}
