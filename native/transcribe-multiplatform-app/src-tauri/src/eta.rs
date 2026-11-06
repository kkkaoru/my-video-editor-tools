//! The steps of a job, where the bar stands, and how long is left.
//!
//! Each step's expected time is a speed in seconds of work per second of video. Speeds start from
//! measured defaults and are replaced by what this machine actually did, so estimates improve with use.

use crate::error::{io_context, AppError, AppResult};
use crate::paths::write_atomic;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Measured on an M5 Pro with Whisper turbo and YOLO faces: 85 min video in 18 min.
const FAST_TRANSCRIBE_RATE: f64 = 0.09;
const FAST_DIARIZE_RATE: f64 = 0.02;
const FAST_SPEECH_LLM_RATE: f64 = 0.01;
/// Measured under x64 emulation on a 4-core ARM VM: 20 s of video in 84 s; plain x64 CPUs do better.
const CPU_TRANSCRIBE_RATE: f64 = 3.0;
const CPU_DIARIZE_RATE: f64 = 0.5;
const CPU_SPEECH_LLM_RATE: f64 = 0.1;
/// Measured on an M5 Pro, two hours of three people: 23% of it goes through the model, on the CPU.
const SEPARATE_RATE: f64 = 0.22;
const FAST_RETRANSCRIBE_RATE: f64 = 0.035;
/// Two tracks of that 23%, at the CPU's speed of transcribing.
const CPU_RETRANSCRIBE_RATE: f64 = 1.4;
const EXTRACT_RATE: f64 = 0.01;
const FACES_RATE: f64 = 0.1;
const CONVERT_SECONDS: f64 = 60.0;
const EXPORT_SECONDS: f64 = 1.0;
/// New measurements count as much as everything before them, so a changed machine is learned quickly.
const LEARNING_WEIGHT: f64 = 0.5;
/// Below this a tool's own percentage is too noisy to extrapolate from.
const MIN_FRACTION_FOR_EXTRAPOLATION: f64 = 0.02;
/// An estimated step never claims to be finished: only the tool's exit can say that.
const ESTIMATED_FRACTION_LIMIT: f64 = 0.95;
const MIN_LEARNED_SECONDS: f64 = 1.0;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    ConvertModel,
    ExtractAudio,
    Transcribe,
    Diarize,
    Separate,
    Retranscribe,
    Faces,
    Export,
}

impl Stage {
    /// The learned speeds of the steps for voices talking at once, per second of the whole video.
    pub const SEPARATE_KEY: &'static str = "separate";
    pub const RETRANSCRIBE_KEY: &'static str = "retranscribe";
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Progress {
    pub stage: Stage,
    /// Whole job, 0 to 1.
    pub overall: f64,
    /// Inside this step, 0 to 1; an estimate when `determinate` is false.
    pub stage_fraction: f64,
    /// `false` while a tool reports no percentage.
    pub determinate: bool,
    /// Every step of this job, in order.
    pub plan: Vec<Stage>,
    pub remaining_seconds: Option<f64>,
}

/// One planned step. `key` names its learned speed; `None` means a fixed duration.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedStage {
    pub stage: Stage,
    pub key: Option<String>,
    pub expected_seconds: f64,
}

/// Learned seconds of work per second of video, kept in the app data folder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StageRates {
    rates: HashMap<String, f64>,
    path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug)]
pub struct RateInputs {
    pub accelerated: bool,
    pub speech_llm: bool,
}

#[derive(Debug)]
pub struct Tracker {
    plan: Vec<PlannedStage>,
    rates: StageRates,
    duration: f64,
    current: Option<(usize, Instant)>,
}

fn default_rate(stage: Stage, inputs: RateInputs) -> f64 {
    let RateInputs {
        accelerated,
        speech_llm,
    } = inputs;
    match (stage, accelerated, speech_llm) {
        (Stage::Transcribe, true, _) => FAST_TRANSCRIBE_RATE,
        (Stage::Transcribe, false, _) => CPU_TRANSCRIBE_RATE,
        (Stage::Diarize, true, false) => FAST_DIARIZE_RATE,
        (Stage::Diarize, false, false) => CPU_DIARIZE_RATE,
        (Stage::Diarize, true, true) => FAST_SPEECH_LLM_RATE,
        (Stage::Diarize, false, true) => CPU_SPEECH_LLM_RATE,
        (Stage::Separate, _, _) => SEPARATE_RATE,
        (Stage::Retranscribe, true, _) => FAST_RETRANSCRIBE_RATE,
        (Stage::Retranscribe, false, _) => CPU_RETRANSCRIBE_RATE,
        (Stage::Faces, _, _) => FACES_RATE,
        _ => EXTRACT_RATE,
    }
}

impl StageRates {
    /// A missing or unreadable file only means no history yet; estimates fall back to the defaults.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        let rates = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            rates,
            path: Some(path.to_path_buf()),
        }
    }

    fn save(&self) -> AppResult<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let text = serde_json::to_vec_pretty(&self.rates)
            .map_err(|error| AppError::Invalid(error.to_string()))?;
        write_atomic(path, &text)
    }

    fn rate(&self, key: &str, fallback: f64) -> f64 {
        self.rates.get(key).copied().unwrap_or(fallback)
    }

    fn learn(&mut self, key: &str, measured: f64) {
        let next = self.rates.get(key).map_or(measured, |old| {
            old * (1.0 - LEARNING_WEIGHT) + measured * LEARNING_WEIGHT
        });
        self.rates.insert(key.to_owned(), next);
    }

    /// A step that scales with the video's length.
    #[must_use]
    pub fn scaled(
        &self,
        stage: Stage,
        key: &str,
        inputs: RateInputs,
        duration: f64,
    ) -> PlannedStage {
        PlannedStage {
            stage,
            key: Some(key.to_owned()),
            expected_seconds: self.rate(key, default_rate(stage, inputs)) * duration,
        }
    }
}

/// The model conversion, which depends on the model and not on the video.
#[must_use]
pub fn convert_step() -> PlannedStage {
    PlannedStage {
        stage: Stage::ConvertModel,
        key: None,
        expected_seconds: CONVERT_SECONDS,
    }
}

/// Writing files is near instant.
#[must_use]
pub fn export_step() -> PlannedStage {
    PlannedStage {
        stage: Stage::Export,
        key: None,
        expected_seconds: EXPORT_SECONDS,
    }
}

impl Tracker {
    #[must_use]
    pub fn new(plan: Vec<PlannedStage>, rates: StageRates, duration: f64) -> Self {
        Self {
            plan,
            rates,
            duration,
            current: None,
        }
    }

    fn learn_finished(&mut self, now: Instant) {
        let Some((index, started)) = self.current else {
            return;
        };
        let elapsed = now.duration_since(started).as_secs_f64();
        let Some(key) = self.plan.get(index).and_then(|step| step.key.clone()) else {
            return;
        };
        // Sub-second steps carry only timer noise.
        if self.duration > 0.0 && elapsed >= MIN_LEARNED_SECONDS {
            self.rates.learn(&key, elapsed / self.duration);
        }
    }

    fn enter(&mut self, index: usize, now: Instant) -> Instant {
        match self.current {
            Some((current, started)) if current == index => started,
            _ => {
                self.learn_finished(now);
                self.current = Some((index, now));
                now
            }
        }
    }

    /// `fraction` is the tool's own progress; `None` when it reports none.
    pub fn update(&mut self, stage: Stage, fraction: Option<f64>, now: Instant) -> Progress {
        let index = self
            .plan
            .iter()
            .position(|step| step.stage == stage)
            .unwrap_or(0);
        let started = self.enter(index, now);
        let elapsed = now.duration_since(started).as_secs_f64();
        let expected = self
            .plan
            .get(index)
            .map_or(0.0, |step| step.expected_seconds);
        let stage_fraction = fraction.map_or_else(
            || estimated_fraction(elapsed, expected),
            |reported| reported.clamp(0.0, 1.0),
        );
        Progress {
            stage,
            overall: self.overall(index, expected, stage_fraction),
            stage_fraction,
            determinate: fraction.is_some(),
            plan: self.plan.iter().map(|step| step.stage).collect(),
            remaining_seconds: Some(self.remaining(index, elapsed, expected, fraction)),
        }
    }

    fn overall(&self, index: usize, expected: f64, fraction: f64) -> f64 {
        let total: f64 = self.plan.iter().map(|step| step.expected_seconds).sum();
        if total <= 0.0 {
            return 0.0;
        }
        let done: f64 = self.plan[..index]
            .iter()
            .map(|step| step.expected_seconds)
            .sum();
        ((done + expected * fraction) / total).clamp(0.0, 1.0)
    }

    fn remaining(&self, index: usize, elapsed: f64, expected: f64, fraction: Option<f64>) -> f64 {
        let later: f64 = self
            .plan
            .get(index + 1..)
            .unwrap_or_default()
            .iter()
            .map(|step| step.expected_seconds)
            .sum();
        let here = fraction
            .filter(|reported| *reported >= MIN_FRACTION_FOR_EXTRAPOLATION)
            .map_or((expected - elapsed).max(0.0), |reported| {
                elapsed * (1.0 - reported.min(1.0)) / reported
            });
        here + later
    }

    /// Records the last step and keeps what this run taught.
    pub fn finish(&mut self, now: Instant) -> AppResult<()> {
        self.learn_finished(now);
        self.current = None;
        self.rates.save().map_err(|error| match error {
            AppError::Io { context, source } => {
                io_context(format!("save speeds: {context}"))(source)
            }
            other => other,
        })
    }
}

fn estimated_fraction(elapsed: f64, expected: f64) -> f64 {
    if expected <= 0.0 {
        return 0.0;
    }
    (elapsed / expected).min(ESTIMATED_FRACTION_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn step(stage: Stage, expected_seconds: f64) -> PlannedStage {
        PlannedStage {
            stage,
            key: Some(format!("{stage:?}")),
            expected_seconds,
        }
    }

    #[test]
    fn the_bar_follows_expected_time_not_step_count() {
        let start = Instant::now();
        let mut tracker = Tracker::new(
            vec![
                step(Stage::ExtractAudio, 10.0),
                step(Stage::Transcribe, 90.0),
            ],
            StageRates::default(),
            100.0,
        );
        tracker.update(Stage::ExtractAudio, Some(1.0), start);
        let progress = tracker.update(Stage::Transcribe, Some(0.5), start);
        assert_eq!(progress.overall, 0.55);
        assert_eq!(progress.plan, vec![Stage::ExtractAudio, Stage::Transcribe]);
        assert_eq!(progress.stage_fraction, 0.5);
        assert!(progress.determinate);
    }

    #[test]
    fn time_left_extrapolates_from_the_tool_percentage() {
        let start = Instant::now();
        let mut tracker = Tracker::new(
            vec![step(Stage::Transcribe, 100.0), step(Stage::Diarize, 30.0)],
            StageRates::default(),
            100.0,
        );
        tracker.update(Stage::Transcribe, Some(0.0), start);
        let progress = tracker.update(
            Stage::Transcribe,
            Some(0.25),
            start + Duration::from_secs(10),
        );
        // 10 s for 25 % leaves 30 s here, plus the 30 s expected for the next step.
        assert_eq!(progress.remaining_seconds, Some(60.0));
    }

    #[test]
    fn a_tiny_percentage_falls_back_to_the_expected_time() {
        let start = Instant::now();
        let mut tracker = Tracker::new(
            vec![step(Stage::Transcribe, 100.0)],
            StageRates::default(),
            100.0,
        );
        tracker.update(Stage::Transcribe, Some(0.0), start);
        let progress = tracker.update(
            Stage::Transcribe,
            Some(0.001),
            start + Duration::from_secs(10),
        );
        assert_eq!(progress.remaining_seconds, Some(90.0));
    }

    #[test]
    fn a_step_without_a_percentage_estimates_but_never_claims_done() {
        let start = Instant::now();
        let mut tracker = Tracker::new(
            vec![step(Stage::Diarize, 10.0)],
            StageRates::default(),
            100.0,
        );
        tracker.update(Stage::Diarize, None, start);
        let halfway = tracker.update(Stage::Diarize, None, start + Duration::from_secs(5));
        assert_eq!(halfway.stage_fraction, 0.5);
        assert!(!halfway.determinate);
        let overdue = tracker.update(Stage::Diarize, None, start + Duration::from_secs(50));
        assert_eq!(overdue.stage_fraction, 0.95);
        assert_eq!(overdue.remaining_seconds, Some(0.0));
    }

    #[test]
    fn finished_steps_teach_the_next_estimate() {
        let start = Instant::now();
        let mut tracker = Tracker::new(
            vec![step(Stage::Transcribe, 1.0), step(Stage::Diarize, 1.0)],
            StageRates::default(),
            100.0,
        );
        tracker.update(Stage::Transcribe, Some(0.0), start);
        tracker.update(Stage::Diarize, None, start + Duration::from_secs(50));
        tracker.finish(start + Duration::from_secs(60)).unwrap();
        let inputs = RateInputs {
            accelerated: true,
            speech_llm: false,
        };
        let learned = tracker
            .rates
            .scaled(Stage::Transcribe, "Transcribe", inputs, 200.0);
        assert_eq!(learned.expected_seconds, 100.0);
    }

    #[test]
    fn learning_averages_with_what_was_known() {
        let mut rates = StageRates::default();
        rates.learn("k", 1.0);
        rates.learn("k", 3.0);
        assert_eq!(rates.rate("k", 9.0), 2.0);
    }

    #[test]
    fn steps_too_short_to_measure_teach_nothing() {
        let start = Instant::now();
        let mut tracker = Tracker::new(
            vec![step(Stage::ExtractAudio, 1.0)],
            StageRates::default(),
            100.0,
        );
        tracker.update(Stage::ExtractAudio, Some(0.0), start);
        tracker.finish(start + Duration::from_millis(100)).unwrap();
        assert_eq!(tracker.rates.rate("ExtractAudio", 7.0), 7.0);
    }

    #[test]
    fn fixed_steps_do_not_learn() {
        let start = Instant::now();
        let mut tracker = Tracker::new(
            vec![convert_step(), export_step()],
            StageRates::default(),
            100.0,
        );
        tracker.update(Stage::ConvertModel, Some(1.0), start);
        tracker.update(Stage::Export, Some(1.0), start + Duration::from_secs(70));
        tracker.finish(start + Duration::from_secs(71)).unwrap();
        assert_eq!(tracker.rates, StageRates::default());
    }

    #[test]
    fn the_steps_for_voices_at_once_have_their_measured_speeds() {
        let fast = RateInputs {
            accelerated: true,
            speech_llm: true,
        };
        let slow = RateInputs {
            accelerated: false,
            speech_llm: true,
        };
        assert_eq!(default_rate(Stage::Separate, fast), 0.22);
        assert_eq!(default_rate(Stage::Separate, slow), 0.22);
        assert_eq!(default_rate(Stage::Retranscribe, fast), 0.035);
        assert_eq!(default_rate(Stage::Retranscribe, slow), 1.4);
    }

    #[test]
    fn defaults_differ_by_hardware_and_model_kind() {
        let fast = RateInputs {
            accelerated: true,
            speech_llm: false,
        };
        let slow = RateInputs {
            accelerated: false,
            speech_llm: false,
        };
        let llm = RateInputs {
            accelerated: true,
            speech_llm: true,
        };
        assert_eq!(default_rate(Stage::Transcribe, fast), 0.09);
        assert_eq!(default_rate(Stage::Transcribe, slow), 3.0);
        assert_eq!(default_rate(Stage::Diarize, fast), 0.02);
        assert_eq!(default_rate(Stage::Diarize, slow), 0.5);
        assert_eq!(default_rate(Stage::Diarize, llm), 0.01);
        assert_eq!(default_rate(Stage::Faces, slow), 0.1);
        assert_eq!(default_rate(Stage::ExtractAudio, slow), 0.01);
    }

    #[test]
    fn an_empty_plan_stays_at_zero() {
        let mut tracker = Tracker::new(Vec::new(), StageRates::default(), 0.0);
        let progress = tracker.overall(0, 0.0, 0.5);
        assert_eq!(progress, 0.0);
        assert_eq!(
            tracker.update(Stage::Export, None, Instant::now()).overall,
            0.0
        );
    }

    #[test]
    fn stage_kinds_serialize_in_snake_case() {
        assert_eq!(
            serde_json::to_string(&Stage::ConvertModel).unwrap(),
            "\"convert_model\""
        );
    }

    #[test]
    fn finishing_an_empty_plan_after_an_update_is_harmless() {
        let start = Instant::now();
        let mut tracker = Tracker::new(Vec::new(), StageRates::default(), 10.0);
        tracker.update(Stage::Export, None, start);
        assert!(tracker.finish(start + Duration::from_secs(5)).is_ok());
    }

    #[test]
    fn a_step_outside_the_plan_counts_as_the_first_step() {
        let mut tracker =
            Tracker::new(vec![step(Stage::Export, 10.0)], StageRates::default(), 10.0);
        let progress = tracker.update(Stage::Faces, Some(0.5), Instant::now());
        assert_eq!(progress.overall, 0.5);
        assert_eq!(progress.stage, Stage::Faces);
    }

    #[test]
    fn a_step_with_no_expected_time_estimates_nothing() {
        assert_eq!(estimated_fraction(5.0, 0.0), 0.0);
    }

    #[test]
    fn a_video_of_no_length_teaches_nothing() {
        let start = Instant::now();
        let mut tracker = Tracker::new(
            vec![step(Stage::Transcribe, 1.0)],
            StageRates::default(),
            0.0,
        );
        tracker.update(Stage::Transcribe, None, start);
        tracker.finish(start + Duration::from_secs(5)).unwrap();
        assert_eq!(tracker.rates, StageRates::default());
    }

    #[test]
    fn the_remaining_defaults_cover_every_mix() {
        let slow_llm = RateInputs {
            accelerated: false,
            speech_llm: true,
        };
        assert_eq!(default_rate(Stage::Diarize, slow_llm), 0.1);
    }

    #[test]
    fn learned_speeds_survive_a_restart() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/eta-round-trip");
        let _ = std::fs::remove_dir_all(&root);
        let file = root.join("rates.json");
        let start = Instant::now();
        let mut tracker = Tracker::new(
            vec![step(Stage::Transcribe, 1.0)],
            StageRates::load(&file),
            100.0,
        );
        tracker.update(Stage::Transcribe, Some(0.0), start);
        tracker.finish(start + Duration::from_secs(50)).unwrap();
        let inputs = RateInputs {
            accelerated: true,
            speech_llm: false,
        };
        assert_eq!(
            StageRates::load(&file).scaled(Stage::Transcribe, "Transcribe", inputs, 10.0),
            PlannedStage {
                stage: Stage::Transcribe,
                key: Some("Transcribe".to_owned()),
                expected_seconds: 5.0
            }
        );
    }

    #[test]
    fn an_unreadable_speed_file_means_no_history() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/eta-unreadable");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("rates.json");
        std::fs::write(&file, b"{").unwrap();
        assert_eq!(StageRates::load(&file).rate("k", 9.0), 9.0);
    }

    #[test]
    fn a_speed_file_that_cannot_be_written_names_the_step() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/eta-unwritable");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("file"), b"x").unwrap();
        let rates = StageRates::load(&root.join("file/rates.json"));
        let mut tracker = Tracker::new(Vec::new(), rates, 1.0);
        let error = tracker.finish(Instant::now()).unwrap_err().to_string();
        assert!(error.starts_with("save speeds: create "), "{error}");
    }
}
