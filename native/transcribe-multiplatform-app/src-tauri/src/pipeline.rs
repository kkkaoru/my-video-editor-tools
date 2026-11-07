//! One dropped video to finished subtitle files, with staged progress.

use crate::captions::{
    assign_speakers, build_cues, metrics_for, steadied_turns, Metrics, SpeakerTurn, Word,
};
use crate::catalog::{self, Backend, Hardware, ModelEntry, RunsOn, SpeakerPlan};
use crate::engines::{self, DiarizeModel, Tools, VideoInfo, WhisperClip};
use crate::error::{io_context, AppError, AppResult, UserError};
use crate::eta::{
    convert_step, export_step, PlannedStage, Progress, RateInputs, Stage, StageRates, Tracker,
};
use crate::export::{unused_base, write_outputs, Transcript};
use crate::face::{scan_faces, FaceDetector, FaceModelKind, FaceSample};
use crate::fonts::bold_widths;
use crate::lanes::{self, LaneEngine, LaneStats};
use crate::morphology::Segmenter;
use crate::paths::AppPaths;
use crate::process::CancelToken;
use crate::schedule::{self, ExecutionPlan};
use crate::separation::{self, SeparationClip, SeparationRun};
use crate::settings::{Anchor, Settings, SubtitleFormat};
use crate::speakers::limit_speakers;
use crate::whisper_ggml;
use serde::Serialize;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A job folder untouched this long was left by a killed process; no step is silent for a day.
const ABANDONED_JOB_AGE: Duration = Duration::from_secs(24 * 60 * 60);

const COREML_RUNNER: &str = "separate-coreml/separate-coreml";
/// How often steps running at once look for a cancelled job.
const STEP_POLL: Duration = Duration::from_millis(100);

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
    /// Speakers were asked for and the model heard none, so every caption has one colour.
    pub speakers_missing: bool,
    /// What running MossFormer2 where people talk at once did; all zero when it did not run.
    pub lanes: LaneStats,
    /// What the job ran for several people; `None` for one.
    pub speaker_plan: Option<SpeakerPlan>,
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

pub(crate) fn wants_faces(settings: &Settings) -> bool {
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
    (speaker_plan, execution): (Option<&SpeakerPlan>, &ExecutionPlan),
    (duration, needs_conversion): (f64, bool),
) -> Vec<PlannedStage> {
    let convert = needs_conversion.then(convert_step);
    let scheduled = execution.groups.iter().flatten().map(|stage| {
        let key = match stage {
            Stage::Diarize => {
                format!("diarize/{}", speaker_plan.map_or("", |plan| plan.diarize))
            }
            Stage::Faces => format!("faces/{}", settings.face_model),
            Stage::Separate
                if speaker_plan.map(|plan| plan.separator)
                    == Some(catalog::MOSSFORMER2_COREML_ID) =>
            {
                Stage::COREML_SEPARATE_KEY.to_owned()
            }
            Stage::Separate => Stage::SEPARATE_KEY.to_owned(),
            Stage::Retranscribe => Stage::RETRANSCRIBE_KEY.to_owned(),
            _ => "transcribe".to_owned(),
        };
        rates.scaled(*stage, &key, inputs, duration)
    });
    convert
        .into_iter()
        .chain([rates.scaled(Stage::ExtractAudio, "extract_audio", inputs, duration)])
        .chain(scheduled)
        .chain([export_step()])
        .collect()
}

struct PreparedSpeakerModel {
    entry: &'static ModelEntry,
    encoder: Option<PathBuf>,
}

/// What one run of the speaker model needs beyond the model itself.
struct SpeakerRun<'a> {
    tools: &'a Tools,
    hardware: &'a Hardware,
    wav: &'a Path,
    settings: &'a Settings,
    cancel: &'a CancelToken,
    paths: &'a AppPaths,
    work: &'a Path,
}

struct RowRun<'a> {
    tools: &'a Tools,
    paths: &'a AppPaths,
    settings: &'a Settings,
    whisper: &'a Path,
    wav: &'a Path,
    work: &'a Path,
    duration: f64,
    segmenter: &'a Segmenter,
    cancel: &'a CancelToken,
    separator: &'static str,
    deep: bool,
}

fn catalogued(id: &str) -> AppResult<&'static ModelEntry> {
    catalog::find_entry(id).ok_or_else(|| AppError::Invalid(format!("{id} is not in the catalog")))
}

fn separate_on_onnx(
    run: &RowRun<'_>,
    clips: &[SeparationClip],
    tick: &dyn Fn(Stage, f64),
) -> AppResult<()> {
    let exe = std::env::current_exe().map_err(io_context("find the app"))?;
    let entry = catalogued(catalog::MOSSFORMER2_ID)?;
    let model = catalog::locate(entry, run.paths, &run.settings.model_dirs).ok_or_else(|| {
        UserError::ModelMissing {
            model: entry.id.to_owned(),
        }
    })?;
    let library = separation::runtime_library(run.paths)?;
    separation::separate_clips(
        &SeparationRun {
            flag: Some(separation::SEPARATE_FLAG),
            exe: &exe,
            library: &library,
            model: &model,
            runs_on: RunsOn::Cpu,
            work: run.work,
            cancel: run.cancel,
        },
        clips,
        &mut |fraction| tick(Stage::Separate, fraction),
    )
}

fn separate_on_coreml(
    run: &RowRun<'_>,
    clips: &[SeparationClip],
    tick: &dyn Fn(Stage, f64),
) -> AppResult<()> {
    let entry = catalogued(catalog::MOSSFORMER2_COREML_ID)?;
    let model = catalog::locate(entry, run.paths, &run.settings.model_dirs).ok_or_else(|| {
        UserError::ModelMissing {
            model: entry.id.to_owned(),
        }
    })?;
    separation::separate_clips(
        &SeparationRun {
            flag: None,
            exe: &run.paths.bundled_runtime_dir().join(COREML_RUNNER),
            library: Path::new(""),
            model: &model,
            runs_on: RunsOn::Gpu,
            work: run.work,
            cancel: run.cancel,
        },
        clips,
        &mut |fraction| tick(Stage::Separate, fraction),
    )
}

/// MossFormer2 in a child process of this app, and Whisper again on each of its tracks.
fn rows_for_overlaps(
    words: Vec<Word>,
    turns: &[SpeakerTurn],
    run: &RowRun<'_>,
    tick: &dyn Fn(Stage, f64),
) -> AppResult<(Vec<Word>, LaneStats)> {
    // Looked up only when someone does talk at once. A runner that fails once is not tried again.
    let coreml_failed = Cell::new(false);
    let mut separate = |clips: &[SeparationClip]| {
        let on_coreml = run.separator == catalog::MOSSFORMER2_COREML_ID && !coreml_failed.get();
        if !on_coreml {
            return separate_on_onnx(run, clips, tick);
        }
        match separate_on_coreml(run, clips, tick) {
            Err(failure) if !matches!(failure, AppError::Cancelled) => {
                coreml_failed.set(true);
                separate_on_onnx(run, clips, tick).map_err(|fallback| match fallback {
                    AppError::User(UserError::ModelMissing { .. }) => AppError::Tool {
                        tool: "separate-coreml".to_owned(),
                        detail: format!(
                            "{failure}\nMossFormer2 for ONNX Runtime is not installed to fall back on"
                        ),
                    },
                    other => other,
                })
            }
            result => result,
        }
    };
    let mut transcribe = |clips: &[WhisperClip]| {
        engines::transcribe_many(
            run.tools,
            run.whisper,
            clips,
            &run.settings.language,
            run.cancel,
            &mut |fraction| tick(Stage::Retranscribe, fraction),
        )
    };
    lanes::with_rows(
        words,
        turns,
        run.wav,
        run.duration,
        run.work,
        run.segmenter,
        &mut LaneEngine {
            deep: run.deep,
            separate: &mut separate,
            transcribe: &mut transcribe,
        },
    )
}

fn prepare_speaker_model(
    plan: SpeakerPlan,
    paths: &AppPaths,
    settings: &Settings,
    hardware: &Hardware,
) -> AppResult<PreparedSpeakerModel> {
    let entry = selected_entry(plan.diarize, None, "diarize")?;
    check_fits(entry, hardware)?;
    let encoder = catalog::speech_llm_encoder(&settings.model_dirs, paths).ok_or_else(|| {
        UserError::ModelMissing {
            model: entry.id.to_owned(),
        }
    })?;
    Ok(PreparedSpeakerModel {
        entry,
        encoder: Some(encoder),
    })
}

/// Extra voices are merged. Whatever is still missing is reported, never invented.
fn separate_speakers(
    prepared: &PreparedSpeakerModel,
    run: &SpeakerRun<'_>,
    on_tick: &mut dyn FnMut(),
) -> AppResult<Vec<SpeakerTurn>> {
    let model = DiarizeModel {
        entry: prepared.entry,
        encoder: prepared.encoder.as_deref(),
        device: catalog::device_for(prepared.entry, run.hardware, run.tools.backend),
        resources: &run.paths.resources,
    };
    let turns = engines::diarize(run.tools, &model, run.wav, run.work, run.cancel, on_tick)?;
    Ok(limit_speakers(&turns, run.settings.speaker_count))
}

/// What one step of the early part of a job leaves behind.
enum StepResult {
    Words(Vec<Word>),
    Turns(Vec<SpeakerTurn>),
    Faces(Vec<FaceSample>),
}

type StepRun<'a> =
    Box<dyn FnOnce(&CancelToken, &dyn Fn(Option<f64>)) -> AppResult<StepResult> + Send + 'a>;

struct Step<'a> {
    stage: Stage,
    run: StepRun<'a>,
}

enum StepEvent {
    Tick(Stage, Option<f64>),
    Done(Stage, AppResult<StepResult>),
}

/// Runs a group of steps at once. The bar follows the earliest step still running; a failed step stops
/// the others, and its failure is the one reported.
fn run_together(
    steps: Vec<Step<'_>>,
    cancel: &CancelToken,
    report: Report<'_>,
    note: &dyn Fn(Stage),
) -> AppResult<Vec<StepResult>> {
    let mut running: Vec<Stage> = steps.iter().map(|step| step.stage).collect();
    let group = CancelToken::default();
    let (sender, receiver) = mpsc::channel();
    let mut results = Vec::new();
    let mut failure = None;
    std::thread::scope(|scope| {
        for Step { stage, run } in steps {
            let (sender, group) = (sender.clone(), &group);
            scope.spawn(move || {
                let result = run(group, &|fraction| {
                    let _ = sender.send(StepEvent::Tick(stage, fraction));
                });
                let _ = sender.send(StepEvent::Done(stage, result));
            });
        }
        drop(sender);
        loop {
            match receiver.recv_timeout(STEP_POLL) {
                Ok(StepEvent::Tick(stage, fraction)) if running.first() == Some(&stage) => {
                    report(stage, fraction);
                }
                Ok(StepEvent::Tick(stage, _)) => note(stage),
                Ok(StepEvent::Done(stage, result)) => {
                    note(stage);
                    running.retain(|other| *other != stage);
                    match result {
                        Ok(found) => results.push(found),
                        Err(error) => {
                            group.cancel();
                            failure.get_or_insert(error);
                        }
                    }
                    if let (Some(next), None) = (running.first(), &failure) {
                        report(*next, None);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if cancel.is_cancelled() {
                group.cancel();
            }
        }
    });
    failure.map_or(Ok(results), Err)
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
    let rows = catalog::statuses(hardware, paths, &settings.model_dirs);
    // One speaker needs no separation at all: no model, no memory, no time.
    let speaker_plan = catalog::choose_speaker_plan(settings, hardware, &rows)?;
    let execution = schedule::execution_plan(settings, hardware, speaker_plan.as_ref());
    let speaker_model = speaker_plan
        .map(|plan| prepare_speaker_model(plan, paths, settings, hardware))
        .transpose()?;
    let faces_from = wants_faces(settings)
        .then(|| face_model(paths, settings))
        .transpose()?;
    let tools = Tools {
        whisper_without_gpu: catalog::whisper_runs_on(hardware) == RunsOn::Cpu,
        ..Tools::locate(paths, catalog::backend_for(hardware, paths))?
    };
    let info = engines::probe(&tools, video)?;
    let inputs = RateInputs {
        accelerated: hardware.is_mac || matches!(tools.backend, Backend::Vulkan | Backend::Cuda),
        speech_llm: speaker_model.is_some(),
    };
    let rates = StageRates::load(&paths.stage_rates_file());
    let needs_conversion = catalog::locate_whisper_ggml(paths, &settings.model_dirs).is_none();
    let plan = plan_job(
        &rates,
        inputs,
        settings,
        (speaker_plan.as_ref(), &execution),
        (info.duration, needs_conversion),
    );
    let tracker = RefCell::new(Tracker::new(plan, rates, info.duration));
    let mut report = |stage: Stage, fraction: Option<f64>| {
        let progress = tracker.borrow_mut().update(stage, fraction, Instant::now());
        sink(progress);
    };
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
    let voice = engines::voice_spans(&tools, &wav)?;
    let (mut words, mut turns, mut faces) = (Vec::new(), Vec::new(), Vec::new());
    for group in &execution.groups {
        let steps = group
            .iter()
            .filter_map(|stage| -> Option<Step<'_>> {
                let (tools, whisper, wav, work) = (&tools, &whisper, &wav, &work);
                let (speaker_model, faces_from, info) = (&speaker_model, &faces_from, &info);
                match stage {
                    Stage::Transcribe => Some(Step {
                        stage: *stage,
                        run: Box::new(move |cancel, tick| {
                            tick(Some(0.0));
                            let words = engines::transcribe(
                                tools,
                                whisper,
                                wav,
                                &settings.language,
                                &work.0,
                                cancel,
                                &mut |fraction| tick(Some(fraction)),
                            )?;
                            if words.is_empty() {
                                return Err(UserError::NoWords.into());
                            }
                            Ok(StepResult::Words(words))
                        }),
                    }),
                    Stage::Diarize => speaker_model.as_ref().map(|prepared| Step {
                        stage: *stage,
                        run: Box::new(move |cancel, tick| {
                            tick(None);
                            separate_speakers(
                                prepared,
                                &SpeakerRun {
                                    tools,
                                    hardware,
                                    wav,
                                    settings,
                                    cancel,
                                    paths,
                                    work: &work.0,
                                },
                                &mut || tick(None),
                            )
                            .map(StepResult::Turns)
                        }),
                    }),
                    Stage::Faces => faces_from.as_ref().map(|model| Step {
                        stage: *stage,
                        run: Box::new(move |cancel, tick| {
                            tick(Some(0.0));
                            locate_faces(tools, model, video, info, cancel, &mut |_, fraction| {
                                tick(fraction);
                            })
                            .map(StepResult::Faces)
                        }),
                    }),
                    _ => None,
                }
            })
            .collect();
        let note = |stage: Stage| tracker.borrow_mut().note(stage, Instant::now());
        for result in run_together(steps, cancel, &mut report, &note)? {
            match result {
                StepResult::Words(found) => words = found,
                StepResult::Turns(found) => turns = found,
                StepResult::Faces(found) => faces = found,
            }
        }
    }
    let speakers_missing = speaker_model.is_some() && turns.is_empty();
    if speaker_model.is_some() {
        // Only the colours: finding where voices overlap needs the model's turns as they came.
        assign_speakers(&mut words, &steadied_turns(&turns));
    } else {
        words.iter_mut().for_each(|word| word.speaker = Some(1));
    }
    let segmenter = Segmenter::for_app(paths);
    let mut lanes = LaneStats::default();
    if let Some((separator, deep)) = speaker_plan.map(|plan| (plan.separator, plan.deep)) {
        let ticks = RefCell::new(&mut report);
        let tick = |stage: Stage, fraction: f64| ticks.borrow_mut()(stage, Some(fraction));
        (words, lanes) = rows_for_overlaps(
            words,
            &turns,
            &RowRun {
                tools: &tools,
                paths,
                settings,
                whisper: &whisper,
                wav: &wav,
                work: &work.0,
                duration: info.duration,
                segmenter: &segmenter,
                cancel,
                separator,
                deep,
            },
            &tick,
        )?;
    }
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
        &segmenter,
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
    tracker.into_inner().finish(Instant::now())?;
    Ok(JobOutcome {
        outputs,
        base,
        speaker_count: transcript.speaker_count,
        requested_speakers: settings.speaker_count,
        cue_count: transcript.cues.len(),
        faceless,
        font_missing: glyphs.is_none(),
        speakers_missing,
        lanes,
        speaker_plan,
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
    // A timeline needs the audio's details, which transcripts of earlier versions do not hold.
    if settings.subtitle_format == SubtitleFormat::Drt && transcript.info.audio.is_none() {
        if let Some(info) = Tools::locate(paths, Backend::Default)
            .ok()
            .and_then(|tools| engines::probe(&tools, &transcript.source).ok())
        {
            transcript.info = info;
        }
    }
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
        speakers_missing: false,
        lanes: LaneStats::default(),
        speaker_plan: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_outside_the_catalog_is_named() {
        assert_eq!(
            catalogued("nope").unwrap_err().to_string(),
            "nope is not in the catalog"
        );
    }

    fn turn_step(stage: Stage, run: StepRun<'_>) -> Step<'_> {
        Step { stage, run }
    }

    fn no_turns() -> AppResult<StepResult> {
        Ok(StepResult::Turns(Vec::new()))
    }

    #[test]
    fn the_bar_follows_the_earliest_step_and_the_others_only_keep_time() {
        let (first_done, wait_first) = mpsc::channel();
        let reported = RefCell::new(Vec::new());
        let noted = RefCell::new(Vec::new());
        let results = run_together(
            vec![
                turn_step(
                    Stage::Transcribe,
                    Box::new(move |_, tick| {
                        tick(Some(0.5));
                        wait_first.recv().unwrap();
                        no_turns()
                    }),
                ),
                turn_step(
                    Stage::Faces,
                    Box::new(move |_, tick| {
                        tick(Some(0.25));
                        first_done.send(()).unwrap();
                        no_turns()
                    }),
                ),
            ],
            &CancelToken::default(),
            &mut |stage, fraction| reported.borrow_mut().push((stage, fraction)),
            &|stage| noted.borrow_mut().push(stage),
        )
        .unwrap();
        assert_eq!(results.len(), 2);
        // The step's poll may report before its first tick, so the order is not fixed.
        let reported = reported.into_inner();
        assert!(reported.contains(&(Stage::Transcribe, Some(0.5))));
        assert!(!reported.contains(&(Stage::Faces, Some(0.25))));
        assert!(noted.into_inner().contains(&Stage::Faces));
    }

    #[test]
    fn a_failed_step_stops_the_one_beside_it_and_is_the_failure_reported() {
        let error = run_together(
            vec![
                turn_step(
                    Stage::Transcribe,
                    Box::new(|cancel, _| {
                        while !cancel.is_cancelled() {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        Err(AppError::Cancelled)
                    }),
                ),
                turn_step(
                    Stage::Faces,
                    Box::new(|_, _| Err(AppError::Invalid("no faces".to_owned()))),
                ),
            ],
            &CancelToken::default(),
            &mut |_, _| {},
            &|_| {},
        )
        .err()
        .unwrap();
        assert_eq!(error.to_string(), "no faces");
    }

    #[test]
    fn cancelling_the_job_stops_every_step() {
        let cancel = CancelToken::default();
        cancel.cancel();
        let error = run_together(
            vec![turn_step(
                Stage::Transcribe,
                Box::new(|cancel, tick| {
                    while !cancel.is_cancelled() {
                        tick(None);
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(AppError::Cancelled)
                }),
            )],
            &cancel,
            &mut |_, _| {},
            &|_| {},
        )
        .err()
        .unwrap();
        assert!(matches!(error, AppError::Cancelled));
    }

    #[test]
    fn a_step_left_running_takes_over_the_bar() {
        let reported = RefCell::new(Vec::new());
        let (first_done, wait_first) = mpsc::channel();
        run_together(
            vec![
                turn_step(
                    Stage::Transcribe,
                    Box::new(move |_, _| {
                        let result = no_turns();
                        first_done.send(()).unwrap();
                        result
                    }),
                ),
                turn_step(
                    Stage::Faces,
                    Box::new(move |_, _| {
                        wait_first.recv().unwrap();
                        std::thread::sleep(Duration::from_millis(20));
                        no_turns()
                    }),
                ),
            ],
            &CancelToken::default(),
            &mut |stage, fraction| reported.borrow_mut().push((stage, fraction)),
            &|_| {},
        )
        .ok()
        .unwrap();
        assert_eq!(reported.into_inner(), vec![(Stage::Faces, None)]);
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
