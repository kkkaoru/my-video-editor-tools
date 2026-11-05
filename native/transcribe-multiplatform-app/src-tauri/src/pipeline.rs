//! One dropped video to finished subtitle files, with staged progress.

use crate::captions::{assign_speakers, build_cues, metrics_for, Metrics, SpeakerTurn};
use crate::catalog::{self, Backend, DiarizeKind, Hardware, ModelEntry};
use crate::engines::{self, DiarizeModel, Tools, VideoInfo};
use crate::error::{io_context, AppError, AppResult, UserError};
use crate::eta::{
    convert_step, export_step, PlannedStage, Progress, RateInputs, Stage, StageRates, Tracker,
};
use crate::export::{unused_base, write_outputs, Transcript};
use crate::face::{scan_faces, FaceDetector, FaceModelKind, FaceSample};
use crate::fonts::bold_widths;
use crate::morphology::Segmenter;
use crate::paths::AppPaths;
use crate::process::CancelToken;
use crate::settings::{Anchor, Settings};
use crate::speakers::{limit_speakers, speaker_total};
use crate::whisper_ggml;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Sortformer's activation threshold for the retry: below its default, so quieter voices count.
const RETRY_ONSET: f64 = 0.3;
/// A job folder untouched this long was left by a killed process; no step is silent for a day.
const ABANDONED_JOB_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// Reports a step's own progress; `None` when the tool gives no percentage.
type Report<'a> = &'a mut dyn FnMut(Stage, Option<f64>);

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct JobOutcome {
    pub outputs: Vec<PathBuf>,
    pub base: PathBuf,
    pub speaker_count: u32,
    /// What the user asked for, 0 when left automatic; more than `speaker_count` means some were not found.
    pub requested_speakers: u32,
    pub cue_count: usize,
    pub faceless: bool,
    /// The chosen font is not on this computer, so line widths were estimated.
    pub font_missing: bool,
}

/// Removes the job's working folder (its large WAV) however the job ends.
struct WorkDir(PathBuf);

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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

fn whisper_model(paths: &AppPaths, settings: &Settings, report: Report<'_>) -> AppResult<PathBuf> {
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
        report(Stage::ConvertModel, Some(fraction));
    })?;
    Ok(output)
}

fn check_fits(entry: &ModelEntry, hardware: &Hardware) -> AppResult<()> {
    catalog::blocked_reason(entry, hardware).map_or(Ok(()), |reason| Err(reason.into()))
}

/// The face model the settings ask for, found before any long work starts.
fn face_model(paths: &AppPaths, settings: &Settings) -> AppResult<(PathBuf, FaceModelKind)> {
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
    Ok((model, kind))
}

fn locate_faces(
    tools: &Tools,
    (model, kind): &(PathBuf, FaceModelKind),
    video: &Path,
    info: &VideoInfo,
    cancel: &CancelToken,
    report: Report<'_>,
) -> AppResult<Vec<FaceSample>> {
    let detector = FaceDetector::load(model, *kind)?;
    scan_faces(tools, &detector, video, info, cancel, &mut |fraction| {
        report(Stage::Faces, Some(fraction));
    })
}

fn wants_faces(settings: &Settings) -> bool {
    matches!(
        settings.placement.anchor,
        Anchor::FaceAbove | Anchor::FaceBelow
    )
}

/// Best effort: a folder that cannot be removed now is tried again by the next job.
fn remove_abandoned_jobs(jobs_dir: &Path, age: Duration) {
    let Ok(entries) = std::fs::read_dir(jobs_dir) else {
        return;
    };
    entries
        .flatten()
        .filter(|entry| {
            entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .is_ok_and(|modified| modified.elapsed().is_ok_and(|elapsed| elapsed >= age))
        })
        .for_each(|entry| drop(std::fs::remove_dir_all(entry.path())));
}

fn job_folder(paths: &AppPaths) -> AppResult<WorkDir> {
    remove_abandoned_jobs(&paths.jobs_dir(), ABANDONED_JOB_AGE);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let folder = paths
        .jobs_dir()
        .join(format!("{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&folder).map_err(io_context("create job folder"))?;
    Ok(WorkDir(folder))
}

/// The steps this job will run, with their expected times.
fn plan_job(
    rates: &StageRates,
    inputs: RateInputs,
    settings: &Settings,
    diarize_id: Option<&str>,
    (duration, needs_conversion): (f64, bool),
) -> Vec<PlannedStage> {
    let convert = needs_conversion.then(convert_step);
    let faces = wants_faces(settings).then(|| {
        let key = format!("faces/{}", settings.face_model);
        rates.scaled(Stage::Faces, &key, inputs, duration)
    });
    let diarize = diarize_id
        .map(|id| rates.scaled(Stage::Diarize, &format!("diarize/{id}"), inputs, duration));
    convert
        .into_iter()
        .chain([
            rates.scaled(Stage::ExtractAudio, "extract_audio", inputs, duration),
            rates.scaled(Stage::Transcribe, "transcribe", inputs, duration),
        ])
        .chain(diarize)
        .chain(faces)
        .chain([export_step()])
        .collect()
}

struct PreparedSpeakerModel {
    entry: &'static ModelEntry,
    main_file: PathBuf,
    encoder: Option<PathBuf>,
}

/// What one run of the speaker model needs beyond the model itself.
struct SpeakerRun<'a> {
    tools: &'a Tools,
    hardware: &'a Hardware,
    wav: &'a Path,
    settings: &'a Settings,
    work: &'a Path,
    cancel: &'a CancelToken,
}

fn prepare_speaker_model(
    paths: &AppPaths,
    settings: &Settings,
    hardware: &Hardware,
) -> AppResult<PreparedSpeakerModel> {
    let rows = catalog::statuses(hardware, paths, &settings.model_dirs);
    let entry = selected_entry(
        &settings.diarize_model,
        catalog::default_diarize_model(hardware, &rows),
        "diarize",
    )?;
    check_fits(entry, hardware)?;
    let main_file = catalog::locate(entry, paths, &settings.model_dirs).ok_or_else(|| {
        UserError::ModelMissing {
            model: entry.id.to_owned(),
        }
    })?;
    Ok(PreparedSpeakerModel {
        entry,
        main_file,
        encoder: catalog::speech_llm_encoder(&settings.model_dirs, paths),
    })
}

fn run_speaker_model(
    prepared: &PreparedSpeakerModel,
    run: &SpeakerRun<'_>,
    onset: Option<f64>,
    on_tick: &mut dyn FnMut(),
) -> AppResult<Vec<SpeakerTurn>> {
    let model = DiarizeModel {
        entry: prepared.entry,
        main_file: &prepared.main_file,
        encoder: prepared.encoder.as_deref(),
        device: catalog::device_for(prepared.entry, run.hardware, run.tools.backend),
        speaker_count: run.settings.speaker_count,
        onset,
    };
    engines::diarize(
        run.tools,
        &model,
        run.wav,
        &run.settings.language,
        run.work,
        run.cancel,
        on_tick,
    )
}

/// With a known count, a run that finds too few voices is repeated once with a lower activation
/// threshold; extra voices are then merged. Whatever is still missing is reported, never invented.
fn separate_speakers(
    prepared: &PreparedSpeakerModel,
    run: &SpeakerRun<'_>,
    on_tick: &mut dyn FnMut(),
) -> AppResult<Vec<SpeakerTurn>> {
    let wanted = run.settings.speaker_count;
    let first = run_speaker_model(prepared, run, None, on_tick)?;
    let can_retry = prepared.entry.diarize_kind == Some(DiarizeKind::Sortformer);
    let turns = if can_retry && speaker_total(&first) < wanted {
        run_speaker_model(prepared, run, Some(RETRY_ONSET), on_tick)?
    } else {
        first
    };
    Ok(limit_speakers(&turns, wanted))
}

fn needs_cuda_runtime(entry: &ModelEntry, tools: &Tools) -> AppResult<()> {
    if entry.needs_cuda && tools.backend != Backend::Cuda {
        return Err(UserError::ModelMissing {
            model: catalog::GEFORCE_RUNTIME_ID.to_owned(),
        }
        .into());
    }
    Ok(())
}

/// Runs the whole job. Models that are missing or too large, or a video too long for the model, fail
/// before any long work starts.
pub fn run_job(
    video: &Path,
    paths: &AppPaths,
    settings: &Settings,
    hardware: &Hardware,
    cancel: &CancelToken,
    sink: &mut dyn FnMut(Progress),
) -> AppResult<JobOutcome> {
    settings.validate()?;
    // One known speaker needs no separation at all: no model, no memory, no time.
    let speaker_model = if settings.speaker_count == 1 {
        None
    } else {
        Some(prepare_speaker_model(paths, settings, hardware)?)
    };
    let faces_from = wants_faces(settings)
        .then(|| face_model(paths, settings))
        .transpose()?;
    let tools = Tools::locate(paths, catalog::backend_for(hardware, paths))?;
    if let Some(prepared) = &speaker_model {
        needs_cuda_runtime(prepared.entry, &tools)?;
    }
    let info = engines::probe(&tools, video)?;
    if let Some(error) = speaker_model
        .as_ref()
        .and_then(|prepared| catalog::too_long(prepared.entry, info.duration))
    {
        return Err(error.into());
    }
    let inputs = RateInputs {
        accelerated: hardware.is_mac || matches!(tools.backend, Backend::Vulkan | Backend::Cuda),
        speech_llm: speaker_model
            .as_ref()
            .is_some_and(|prepared| prepared.entry.max_audio_seconds.is_some()),
    };
    let rates = StageRates::load(&paths.stage_rates_file());
    let needs_conversion = catalog::locate_whisper_ggml(paths, &settings.model_dirs).is_none();
    let plan = plan_job(
        &rates,
        inputs,
        settings,
        speaker_model.as_ref().map(|prepared| prepared.entry.id),
        (info.duration, needs_conversion),
    );
    let mut tracker = Tracker::new(plan, rates, info.duration);
    let mut report =
        |stage: Stage, fraction: Option<f64>| sink(tracker.update(stage, fraction, Instant::now()));
    let whisper = whisper_model(paths, settings, &mut report)?;
    cancel.check()?;

    let work = job_folder(paths)?;
    let wav = work.0.join("audio.wav");
    report(Stage::ExtractAudio, Some(0.0));
    engines::extract_audio(
        &tools,
        video,
        &wav,
        info.duration,
        cancel,
        &mut |fraction| {
            report(Stage::ExtractAudio, Some(fraction));
        },
    )?;
    report(Stage::Transcribe, Some(0.0));
    let mut words = engines::transcribe(
        &tools,
        &whisper,
        &wav,
        &settings.language,
        &work.0,
        cancel,
        &mut |fraction| report(Stage::Transcribe, Some(fraction)),
    )?;
    if words.is_empty() {
        return Err(UserError::NoWords.into());
    }
    let voice = engines::voice_spans(&tools, &wav)?;
    let turns = match &speaker_model {
        Some(prepared) => {
            report(Stage::Diarize, None);
            separate_speakers(
                prepared,
                &SpeakerRun {
                    tools: &tools,
                    hardware,
                    wav: &wav,
                    settings,
                    work: &work.0,
                    cancel,
                },
                &mut || report(Stage::Diarize, None),
            )?
        }
        None => Vec::new(),
    };
    if speaker_model.is_some() {
        assign_speakers(&mut words, &turns);
    } else {
        words.iter_mut().for_each(|word| word.speaker = Some(1));
    }
    let faces = match &faces_from {
        Some(model) => {
            report(Stage::Faces, Some(0.0));
            locate_faces(&tools, model, video, &info, cancel, &mut report)?
        }
        None => Vec::new(),
    };
    report(Stage::Export, Some(0.0));
    let glyphs = bold_widths(&settings.font_family);
    let (cues, faceless) = build_cues(
        &words,
        &faces,
        &voice,
        &info,
        &Metrics {
            glyphs,
            ..metrics_for(&info, settings)
        },
        settings,
        &Segmenter::for_app(paths),
    )?;
    let mut transcript = Transcript::new(
        video.to_path_buf(),
        info,
        metrics_for(&info, settings),
        words,
        faces,
        cues,
    );
    transcript.voice = voice;
    let base = unused_base(video);
    let outputs = write_outputs(&base, &transcript, settings)?;
    report(Stage::Export, Some(1.0));
    tracker.finish(Instant::now())?;
    Ok(JobOutcome {
        outputs,
        base,
        speaker_count: transcript.speaker_count,
        requested_speakers: settings.speaker_count,
        cue_count: transcript.cues.len(),
        faceless,
        font_missing: glyphs.is_none(),
    })
}

/// Lays out an earlier transcript again with new colours or position, without the speech models.
/// All outputs go to `base`.
pub fn relayout_to(
    transcript_file: &Path,
    base: &Path,
    paths: &AppPaths,
    settings: &Settings,
    cancel: &CancelToken,
    sink: &mut dyn FnMut(Progress),
) -> AppResult<JobOutcome> {
    settings.validate()?;
    let mut transcript = Transcript::load(transcript_file)?;
    let needs_faces = wants_faces(settings) && transcript.faces.is_empty();
    let faces_from = needs_faces
        .then(|| face_model(paths, settings))
        .transpose()?;
    let rates = StageRates::load(&paths.stage_rates_file());
    let inputs = RateInputs {
        accelerated: false,
        speech_llm: false,
    };
    let duration = transcript.info.duration;
    let faces_key = format!("faces/{}", settings.face_model);
    let plan: Vec<PlannedStage> = needs_faces
        .then(|| rates.scaled(Stage::Faces, &faces_key, inputs, duration))
        .into_iter()
        .chain([export_step()])
        .collect();
    let mut tracker = Tracker::new(plan, rates, duration);
    let mut report =
        |stage: Stage, fraction: Option<f64>| sink(tracker.update(stage, fraction, Instant::now()));
    if let Some(model) = &faces_from {
        let tools = Tools::locate(paths, Backend::Default)?;
        report(Stage::Faces, Some(0.0));
        transcript.faces = locate_faces(
            &tools,
            model,
            &transcript.source,
            &transcript.info,
            cancel,
            &mut report,
        )?;
    }
    report(Stage::Export, Some(0.0));
    let glyphs = bold_widths(&settings.font_family);
    let (cues, faceless) = build_cues(
        &transcript.words,
        &transcript.faces,
        &transcript.voice,
        &transcript.info,
        &Metrics {
            glyphs,
            ..metrics_for(&transcript.info, settings)
        },
        settings,
        &Segmenter::for_app(paths),
    )?;
    transcript.metrics = metrics_for(&transcript.info, settings);
    transcript.cues = cues;
    let outputs = write_outputs(base, &transcript, settings)?;
    report(Stage::Export, Some(1.0));
    tracker.finish(Instant::now())?;
    Ok(JobOutcome {
        outputs,
        base: base.to_path_buf(),
        speaker_count: transcript.speaker_count,
        requested_speakers: settings.speaker_count,
        cue_count: transcript.cues.len(),
        faceless,
        font_missing: glyphs.is_none(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn abandoned_job_folders_are_removed_and_recent_ones_stay() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target-test-abandoned");
        std::fs::create_dir_all(root.join("left-behind/nested")).unwrap();
        remove_abandoned_jobs(&root, Duration::from_secs(3600));
        assert!(root.join("left-behind").exists());
        remove_abandoned_jobs(&root, Duration::ZERO);
        assert!(!root.join("left-behind").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_missing_jobs_folder_has_nothing_to_remove() {
        remove_abandoned_jobs(Path::new("/nonexistent/jobs"), Duration::ZERO);
    }

    #[test]
    fn work_folder_is_removed_on_drop() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target-test-workdir");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        drop(WorkDir(root.clone()));
        assert!(!root.exists());
    }
}
