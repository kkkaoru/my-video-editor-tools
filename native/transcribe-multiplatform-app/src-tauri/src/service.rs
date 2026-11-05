//! Application behaviour behind the UI commands: settings, the job queue and downloads.
//! Nothing here knows about Tauri; events leave through a callback.

use crate::catalog::{self, Backend, Hardware, ModelEntry, ModelStatus};
use crate::download::{self, DownloadProgress};
use crate::error::{io_context, AppError, AppResult, UserError};
use crate::eta::Progress;
use crate::export::{dropped_base, transcript_base, transcript_beside, unused_stem};
use crate::fonts;
use crate::paths::AppPaths;
use crate::pipeline::{self, JobOutcome};
use crate::process::CancelToken;
use crate::profiles::{self, CaptionLook, Profile};
use crate::settings::Settings;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

const FALLBACK_LOCALE: &str = "en";

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum JobEvent {
    Queued { id: u64, path: PathBuf },
    Running { id: u64, progress: Progress },
    Finished { id: u64, outcome: JobOutcome },
    Failed { id: u64, error: serde_json::Value },
    Cancelled { id: u64 },
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DownloadEvent {
    Running {
        id: String,
        done_bytes: u64,
        total_bytes: u64,
    },
    Finished {
        id: String,
    },
    Failed {
        id: String,
        error: serde_json::Value,
    },
}

#[derive(Clone, Debug)]
pub enum AppEvent {
    Job(JobEvent),
    Download(DownloadEvent),
}

pub type EventSink = Arc<dyn Fn(AppEvent) + Send + Sync>;

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub version: &'static str,
    pub locale: String,
    pub hardware: Hardware,
    /// The speech-tool build jobs use on this machine now.
    pub backend: Backend,
    pub settings: Settings,
    pub models: Vec<ModelStatus>,
    pub models_dir: PathBuf,
    pub profiles: Vec<Profile>,
}

enum JobRequest {
    Transcribe(PathBuf),
    /// A transcript or a dropped subtitle file: new files under an unused name, never over the existing ones.
    Reformat {
        dropped: PathBuf,
        look: Option<CaptionLook>,
    },
}

struct QueuedJob {
    id: u64,
    request: JobRequest,
    cancel: CancelToken,
}

struct Shared {
    paths: AppPaths,
    hardware: Hardware,
    settings: Mutex<Settings>,
    jobs: Mutex<HashMap<u64, CancelToken>>,
    downloads: Mutex<HashMap<String, CancelToken>>,
    fonts: OnceLock<Vec<String>>,
    sink: EventSink,
}

pub struct Service {
    shared: Arc<Shared>,
    queue: Sender<QueuedJob>,
    next_job: AtomicU64,
    locale: String,
}

fn locked<'a, Value>(mutex: &'a Mutex<Value>, name: &str) -> AppResult<MutexGuard<'a, Value>> {
    mutex
        .lock()
        .map_err(|_| AppError::Invalid(format!("{name} lock poisoned")))
}

/// The JSON shape the UI translates; serializing an `AppError` cannot fail.
fn error_json(error: &AppError) -> serde_json::Value {
    serde_json::to_value(error).unwrap_or(serde_json::Value::Null)
}

fn job_result_event(id: u64, result: AppResult<JobOutcome>) -> JobEvent {
    match result {
        Ok(outcome) => JobEvent::Finished { id, outcome },
        Err(AppError::Cancelled) => JobEvent::Cancelled { id },
        Err(error) => JobEvent::Failed {
            id,
            error: error_json(&error),
        },
    }
}

fn run_reformat(
    dropped: &Path,
    settings: &Settings,
    paths: &AppPaths,
    cancel: &CancelToken,
    report: &mut dyn FnMut(Progress),
) -> AppResult<JobOutcome> {
    let transcript = transcript_beside(dropped)?;
    let base = unused_stem(&transcript_base(&transcript));
    pipeline::relayout_to(&transcript, &base, paths, settings, cancel, report)
}

fn run_queued(shared: &Shared, job: &QueuedJob) -> AppResult<()> {
    let settings = locked(&shared.settings, "settings")?.clone();
    let id = job.id;
    let sink = Arc::clone(&shared.sink);
    let mut report = |progress: Progress| sink(AppEvent::Job(JobEvent::Running { id, progress }));
    let result = match &job.request {
        JobRequest::Transcribe(video) => pipeline::run_job(
            video,
            &shared.paths,
            &settings,
            &shared.hardware,
            &job.cancel,
            &mut report,
        ),
        JobRequest::Reformat { dropped, look } => run_reformat(
            dropped,
            &look
                .clone()
                .map_or_else(|| settings.clone(), |look| look.applied_to(&settings)),
            &shared.paths,
            &job.cancel,
            &mut report,
        ),
    };
    (shared.sink)(AppEvent::Job(job_result_event(id, result)));
    locked(&shared.jobs, "job")?.remove(&id);
    Ok(())
}

/// One job at a time: the speech models each want most of the memory.
fn work_through_queue(shared: &Shared, receiver: &Receiver<QueuedJob>) {
    for job in receiver {
        if let Err(error) = run_queued(shared, &job) {
            (shared.sink)(AppEvent::Job(JobEvent::Failed {
                id: job.id,
                error: error_json(&error),
            }));
        }
    }
}

fn run_download(entry: &'static ModelEntry, shared: &Shared, cancel: &CancelToken) {
    let id = entry.id.to_owned();
    let mut report = |progress: DownloadProgress| {
        (shared.sink)(AppEvent::Download(DownloadEvent::Running {
            id: id.clone(),
            done_bytes: progress.done_bytes,
            total_bytes: progress.total_bytes,
        }));
    };
    let result = download::download_entry(entry, &shared.paths, cancel, &mut report);
    let event = match result {
        Ok(()) => DownloadEvent::Finished { id: id.clone() },
        Err(error) => DownloadEvent::Failed {
            id: id.clone(),
            error: error_json(&error),
        },
    };
    // Forgotten before the event, so an action taken on it does not find this download still running.
    if let Ok(mut downloads) = shared.downloads.lock() {
        downloads.remove(&id);
    }
    (shared.sink)(AppEvent::Download(event));
}

fn known_entry(id: &str) -> AppResult<&'static ModelEntry> {
    catalog::find_entry(id).ok_or_else(|| AppError::Invalid(format!("unknown model {id}")))
}

fn own_folder(entry: &ModelEntry, paths: &AppPaths) -> PathBuf {
    if entry.id == catalog::GEFORCE_RUNTIME_ID {
        paths.downloaded_runtime_dir().join("geforce")
    } else {
        paths.models_dir().join(entry.id)
    }
}

/// Deletes only the app's own copy; files found in other folders belong to the user.
fn remove_own_copy(entry: &ModelEntry, paths: &AppPaths) -> AppResult<()> {
    let folder = own_folder(entry, paths);
    if !folder.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(&folder).map_err(io_context(format!("remove {}", folder.display())))
}

/// Compares where folders really are, so a link or a relative spelling cannot hide an overlap.
fn real_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// True when `target` holds, is, or sits inside a folder the user chose: those files stay theirs.
fn overlaps_user_folder(target: &Path, user_folders: &[PathBuf]) -> bool {
    let target = real_path(target);
    user_folders
        .iter()
        .map(|folder| real_path(folder))
        .any(|folder| folder.starts_with(&target) || target.starts_with(&folder))
}

impl Service {
    /// Starts the job worker; the service ends with the process.
    #[must_use]
    pub fn start(
        paths: AppPaths,
        hardware: Hardware,
        settings: Settings,
        locale: Option<String>,
        sink: EventSink,
    ) -> Self {
        let (queue, receiver) = mpsc::channel();
        let shared = Arc::new(Shared {
            paths,
            hardware,
            settings: Mutex::new(settings),
            jobs: Mutex::new(HashMap::new()),
            downloads: Mutex::new(HashMap::new()),
            fonts: OnceLock::new(),
            sink,
        });
        let worker = Arc::clone(&shared);
        std::thread::spawn(move || work_through_queue(&worker, &receiver));
        Self {
            shared,
            queue,
            next_job: AtomicU64::new(1),
            locale: locale.unwrap_or_else(|| FALLBACK_LOCALE.to_owned()),
        }
    }

    pub fn snapshot(&self) -> AppResult<Snapshot> {
        let settings = locked(&self.shared.settings, "settings")?.clone();
        let models = catalog::statuses(
            &self.shared.hardware,
            &self.shared.paths,
            &settings.model_dirs,
        );
        Ok(Snapshot {
            version: env!("CARGO_PKG_VERSION"),
            locale: self.locale.clone(),
            hardware: self.shared.hardware.clone(),
            backend: catalog::backend_for(&self.shared.hardware, &self.shared.paths),
            settings,
            models,
            models_dir: self.shared.paths.models_dir(),
            // A damaged file must not stop the app from opening; saving or deleting still reports it.
            profiles: self.saved_profiles().unwrap_or_default(),
        })
    }

    fn saved_profiles(&self) -> AppResult<Vec<Profile>> {
        profiles::load(&self.shared.paths.profiles_file())
    }

    fn write_profiles(&self, rows: Vec<Profile>) -> AppResult<()> {
        profiles::save(&self.shared.paths.profiles_file(), rows)
    }

    pub fn save_settings(&self, settings: Settings) -> AppResult<Snapshot> {
        settings.save(&self.shared.paths.settings_file())?;
        *locked(&self.shared.settings, "settings")? = settings;
        self.snapshot()
    }

    /// Saves `settings` under a new name and selects it; an existing name is refused.
    pub fn save_profile(&self, name: &str, settings: Settings) -> AppResult<Snapshot> {
        let name = profiles::clean_name(name)?;
        settings.validate()?;
        let profile = Profile {
            name: name.clone(),
            look: CaptionLook::from_settings(&settings),
        };
        self.write_profiles(profiles::added(self.saved_profiles()?, profile)?)?;
        self.save_settings(Settings {
            active_profile: name,
            ..settings
        })
    }

    /// Replaces the look of an existing profile with `settings` and selects it.
    pub fn overwrite_profile(&self, name: &str, settings: Settings) -> AppResult<Snapshot> {
        settings.validate()?;
        let profile = Profile {
            name: name.to_owned(),
            look: CaptionLook::from_settings(&settings),
        };
        self.write_profiles(profiles::replaced(self.saved_profiles()?, profile)?)?;
        self.save_settings(Settings {
            active_profile: name.to_owned(),
            ..settings
        })
    }

    /// Restores a profile's look over the current settings and selects it.
    pub fn load_profile(&self, name: &str) -> AppResult<Snapshot> {
        let look = profiles::look_named(&self.saved_profiles()?, name)?;
        let current = locked(&self.shared.settings, "settings")?.clone();
        self.save_settings(Settings {
            active_profile: name.to_owned(),
            ..look.applied_to(&current)
        })
    }

    /// Removes a profile; the settings on screen stay as they are, and no longer point at it.
    pub fn delete_profile(&self, name: &str) -> AppResult<Snapshot> {
        self.write_profiles(profiles::removed(self.saved_profiles()?, name)?)?;
        let current = locked(&self.shared.settings, "settings")?.clone();
        if current.active_profile != name {
            return self.snapshot();
        }
        self.save_settings(Settings {
            active_profile: String::new(),
            ..current
        })
    }

    /// The installed font families, scanned once on first request.
    pub fn font_families(&self) -> Vec<String> {
        self.shared
            .fonts
            .get_or_init(fonts::system_families)
            .clone()
    }

    fn enqueue(&self, path: &Path, request: JobRequest) -> AppResult<u64> {
        let id = self.next_job.fetch_add(1, Ordering::Relaxed);
        let cancel = CancelToken::default();
        locked(&self.shared.jobs, "job")?.insert(id, cancel.clone());
        (self.shared.sink)(AppEvent::Job(JobEvent::Queued {
            id,
            path: path.to_path_buf(),
        }));
        self.queue
            .send(QueuedJob {
                id,
                request,
                cancel,
            })
            .map_err(|_| AppError::Invalid("job worker stopped".into()))?;
        Ok(id)
    }

    pub fn add_videos(&self, videos: &[PathBuf]) -> AppResult<Vec<u64>> {
        videos
            .iter()
            .map(|video| self.enqueue(video, JobRequest::Transcribe(video.clone())))
            .collect()
    }

    pub fn relayout(&self, transcript: &Path) -> AppResult<u64> {
        self.enqueue(
            transcript,
            JobRequest::Reformat {
                dropped: transcript.to_path_buf(),
                look: None,
            },
        )
    }

    /// One job per transcript, however many of its files were dropped, with the selected profile's
    /// saved look; without a selection, the current settings.
    pub fn reformat_files(&self, dropped: &[PathBuf]) -> AppResult<Vec<u64>> {
        let selected = locked(&self.shared.settings, "settings")?
            .active_profile
            .clone();
        let look = profiles::look_named(&self.saved_profiles()?, &selected).ok();
        let mut seen = HashSet::new();
        dropped
            .iter()
            .filter(|file| seen.insert(dropped_base(file)))
            .map(|file| {
                self.enqueue(
                    file,
                    JobRequest::Reformat {
                        dropped: file.clone(),
                        look: look.clone(),
                    },
                )
            })
            .collect()
    }

    pub fn cancel_job(&self, id: u64) -> AppResult<()> {
        if let Some(token) = locked(&self.shared.jobs, "job")?.get(&id) {
            token.cancel();
        }
        Ok(())
    }

    pub fn start_download(&self, id: &str) -> AppResult<()> {
        let entry = known_entry(id)?;
        let cancel = CancelToken::default();
        {
            let mut downloads = locked(&self.shared.downloads, "download")?;
            if downloads.contains_key(id) {
                return Ok(());
            }
            downloads.insert(id.to_owned(), cancel.clone());
        }
        let shared = Arc::clone(&self.shared);
        std::thread::spawn(move || run_download(entry, &shared, &cancel));
        Ok(())
    }

    pub fn cancel_download(&self, id: &str) -> AppResult<()> {
        if let Some(token) = locked(&self.shared.downloads, "download")?.get(id) {
            token.cancel();
        }
        Ok(())
    }

    pub fn delete_model(&self, id: &str) -> AppResult<Snapshot> {
        remove_own_copy(known_entry(id)?, &self.shared.paths)?;
        self.snapshot()
    }

    /// Deletes every catalogued model's own folder and nothing else: files put there by hand, folders
    /// the user added, and anything overlapping them, stay. Refused while running work could still be using or writing a model.
    pub fn reset_models(&self) -> AppResult<Snapshot> {
        if !locked(&self.shared.jobs, "job")?.is_empty()
            || !locked(&self.shared.downloads, "download")?.is_empty()
        {
            return Err(UserError::ModelsBusy.into());
        }
        let user_folders = locked(&self.shared.settings, "settings")?
            .model_dirs
            .clone();
        // shortcut: only current catalog IDs are removed, so the folder of an ID since dropped from the
        // catalog stays on disk; walk the models folder if an ID is ever retired.
        catalog::CATALOG
            .iter()
            .filter(|entry| {
                !overlaps_user_folder(&own_folder(entry, &self.shared.paths), &user_folders)
            })
            .try_for_each(|entry| remove_own_copy(entry, &self.shared.paths))?;
        self.snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::Receiver;
    use std::time::Duration;

    const WAIT: Duration = Duration::from_secs(20);

    fn scratch(name: &str) -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target-test")
            .join(format!("service-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn started(root: &Path) -> (Service, Receiver<AppEvent>) {
        let paths = AppPaths {
            data: root.join("data"),
            resources: root.join("resources"),
            hf_hub: None,
        };
        let hardware = Hardware {
            is_mac: true,
            memory_gib: 16,
            gpu: None,
        };
        let (sender, receiver) = mpsc::channel();
        let sender = Mutex::new(sender);
        let sink: EventSink = Arc::new(move |event| sender.lock().unwrap().send(event).unwrap());
        (
            Service::start(paths, hardware, Settings::default(), None, sink),
            receiver,
        )
    }

    fn next_job_event(receiver: &Receiver<AppEvent>) -> JobEvent {
        loop {
            if let AppEvent::Job(event) = receiver.recv_timeout(WAIT).unwrap() {
                return event;
            }
        }
    }

    #[test]
    fn the_snapshot_carries_the_version_the_release_is_built_as() {
        let (service, _events) = started(&scratch("version"));
        let tauri: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(service.snapshot().unwrap().version, tauri["version"]);
    }

    #[test]
    fn a_missing_locale_falls_back_to_english() {
        let (service, _events) = started(&scratch("locale"));
        assert_eq!(service.snapshot().unwrap().locale, "en");
    }

    #[test]
    fn saved_settings_appear_in_the_snapshot_and_on_disk() {
        let root = scratch("settings");
        let (service, _events) = started(&root);
        let settings = Settings {
            font_ratio: 0.05,
            ..Settings::default()
        };
        assert_eq!(
            service.save_settings(settings).unwrap().settings.font_ratio,
            0.05
        );
        assert_eq!(
            Settings::load(&root.join("data/settings.json"))
                .unwrap()
                .font_ratio,
            0.05
        );
    }

    #[test]
    fn invalid_settings_are_refused() {
        let (service, _events) = started(&scratch("invalid"));
        let settings = Settings {
            palette: Vec::new(),
            ..Settings::default()
        };
        assert_eq!(
            service.save_settings(settings).unwrap_err().to_string(),
            "the speaker palette needs at least one colour"
        );
    }

    #[test]
    fn a_failing_job_reports_a_translatable_code_and_leaves_the_queue_working() {
        let (service, events) = started(&scratch("job"));
        let ids = service
            .add_videos(&[PathBuf::from("/nope.mp4"), PathBuf::from("/nope2.mp4")])
            .unwrap();
        assert_eq!(ids, vec![1, 2]);
        assert!(matches!(
            next_job_event(&events),
            JobEvent::Queued { id: 1, .. }
        ));
        assert!(matches!(
            next_job_event(&events),
            JobEvent::Queued { id: 2, .. }
        ));
        assert_eq!(
            serde_json::to_value(next_job_event(&events)).unwrap(),
            serde_json::json!({
                "state": "failed",
                "id": 1,
                "error": { "code": "no_model_for_role", "role": "diarize" }
            })
        );
    }

    #[test]
    fn relayout_of_a_missing_transcript_names_the_file() {
        let (service, events) = started(&scratch("relayout"));
        assert_eq!(
            service
                .relayout(Path::new("/nope.transcript.json"))
                .unwrap(),
            1
        );
        assert!(matches!(
            next_job_event(&events),
            JobEvent::Queued { id: 1, .. }
        ));
        assert_eq!(
            serde_json::to_value(next_job_event(&events)).unwrap()["error"],
            serde_json::json!({ "code": "transcript_missing", "file": "nope.transcript.json" })
        );
    }

    #[test]
    fn cancelling_unknown_work_is_harmless() {
        let (service, _events) = started(&scratch("cancel"));
        assert!(service.cancel_job(99).is_ok());
        assert!(service.cancel_download("nope").is_ok());
    }

    #[test]
    fn job_results_map_to_events() {
        assert!(matches!(
            job_result_event(4, Err(AppError::Cancelled)),
            JobEvent::Cancelled { id: 4 }
        ));
        let outcome = JobOutcome {
            outputs: Vec::new(),
            base: PathBuf::from("b"),
            speaker_count: 2,
            requested_speakers: 0,
            cue_count: 3,
            faceless: false,
        };
        assert!(matches!(
            job_result_event(5, Ok(outcome)),
            JobEvent::Finished { id: 5, .. }
        ));
    }

    #[test]
    fn unknown_models_cannot_be_downloaded_or_deleted() {
        let (service, _events) = started(&scratch("unknown"));
        assert_eq!(
            service.start_download("nope").unwrap_err().to_string(),
            "unknown model nope"
        );
        assert_eq!(
            service.delete_model("nope").unwrap_err().to_string(),
            "unknown model nope"
        );
    }

    #[test]
    fn a_cancelled_download_reports_failure_and_saves_nothing() {
        let root = scratch("download");
        let (service, events) = started(&root);
        let entry = catalog::find_entry(catalog::WHISPER_ID).unwrap();
        let cancel = CancelToken::default();
        cancel.cancel();
        run_download(entry, &service.shared, &cancel);
        assert!(matches!(
            events.recv_timeout(WAIT).unwrap(),
            AppEvent::Download(DownloadEvent::Failed { id, .. }) if id == "whisper-large-v3-turbo"
        ));
    }

    #[test]
    fn deleting_removes_only_the_apps_copy() {
        let root = scratch("delete");
        let (service, _events) = started(&root);
        let owned = root.join("data/models/nemotron-3-diarization/a.gguf");
        std::fs::create_dir_all(owned.parent().unwrap()).unwrap();
        std::fs::write(&owned, b"x").unwrap();
        let elsewhere = root.join("user-models/a.gguf");
        std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
        std::fs::write(&elsewhere, b"x").unwrap();
        service.delete_model("nemotron-3-diarization").unwrap();
        assert!(!owned.exists());
        assert!(elsewhere.exists());
        assert!(service.delete_model("nemotron-3-diarization").is_ok());
    }

    #[test]
    fn deleting_the_runtime_uses_the_runtime_folder() {
        let root = scratch("delete-runtime");
        let (service, _events) = started(&root);
        let owned = root.join("data/runtime/geforce/nemo/x.dll");
        std::fs::create_dir_all(owned.parent().unwrap()).unwrap();
        std::fs::write(&owned, b"x").unwrap();
        service.delete_model("geforce-runtime").unwrap();
        assert!(!owned.exists());
    }

    #[test]
    fn starting_a_download_runs_on_its_own_thread() {
        let (service, events) = started(&scratch("start-download"));
        // The YuNet entry has no files, so this finishes without touching the network.
        service.start_download(catalog::YUNET_ID).unwrap();
        assert!(matches!(
            events.recv_timeout(WAIT).unwrap(),
            AppEvent::Download(DownloadEvent::Finished { id }) if id == "yunet"
        ));
    }

    #[test]
    fn a_broken_lock_fails_the_job_instead_of_stopping_the_worker() {
        let (service, events) = started(&scratch("poisoned"));
        let shared = Arc::clone(&service.shared);
        let poison = std::thread::spawn(move || {
            let guard = shared.settings.lock().unwrap();
            std::panic::panic_any(guard.font_family.clone())
        });
        assert!(poison.join().is_err());
        assert_eq!(
            service.snapshot().unwrap_err().to_string(),
            "settings lock poisoned"
        );
        assert_eq!(
            service.add_videos(&[PathBuf::from("/nope.mp4")]).unwrap(),
            vec![1]
        );
        assert!(matches!(
            next_job_event(&events),
            JobEvent::Queued { id: 1, .. }
        ));
        assert_eq!(
            serde_json::to_value(next_job_event(&events)).unwrap(),
            serde_json::json!({
                "state": "failed",
                "id": 1,
                "error": { "code": "internal", "detail": "settings lock poisoned" }
            })
        );
    }

    #[test]
    fn a_download_reports_its_progress_as_events() {
        let root = scratch("download-progress");
        let (service, events) = started(&root);
        // A file already present at the right size finishes without any request.
        let entry: &'static ModelEntry = Box::leak(Box::new(ModelEntry {
            id: "test-model",
            role: catalog::Role::Diarize,
            label: "",
            source_url: "",
            files: Box::leak(Box::new([catalog::FileSpec {
                url_kind: catalog::UrlKind::Direct("http://127.0.0.1:9/unused"),
                path: "m.bin",
                size: 1,
                sha256: "00",
            }])),
            min_memory_gib: 0,
            diarize_kind: None,
            needs_cuda: false,
            extra_args: &[],
            max_audio_seconds: None,
        }));
        let file = root.join("data/models/test-model/m.bin");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"x").unwrap();
        run_download(entry, &service.shared, &CancelToken::default());
        assert!(matches!(
            events.recv_timeout(WAIT).unwrap(),
            AppEvent::Download(DownloadEvent::Running {
                done_bytes: 1,
                total_bytes: 1,
                ..
            })
        ));
        assert!(matches!(
            events.recv_timeout(WAIT).unwrap(),
            AppEvent::Download(DownloadEvent::Finished { id }) if id == "test-model"
        ));
    }

    fn write_transcript(base: &Path) {
        let transcript = crate::export::Transcript::new(
            PathBuf::from("/video.mp4"),
            crate::engines::VideoInfo {
                width: 1920,
                height: 1080,
                duration: 10.0,
                fps_num: 30,
                fps_den: 1,
            },
            crate::captions::metrics_for(
                &crate::engines::VideoInfo {
                    width: 1920,
                    height: 1080,
                    duration: 10.0,
                    fps_num: 30,
                    fps_den: 1,
                },
                &Settings::default(),
            ),
            vec![crate::captions::Word {
                start: 0.0,
                end: 1.0,
                text: "hello".to_owned(),
                speaker: Some(1),
            }],
            Vec::new(),
            Vec::new(),
        );
        std::fs::write(
            format!("{}.transcript.json", base.display()),
            serde_json::to_vec(&transcript).unwrap(),
        )
        .unwrap();
    }

    fn next_outcome(events: &Receiver<AppEvent>) -> JobOutcome {
        loop {
            match next_job_event(events) {
                JobEvent::Finished { outcome, .. } => return outcome,
                JobEvent::Failed { error, .. } => panic!("job failed: {error}"),
                _ => {}
            }
        }
    }

    #[test]
    fn a_saved_profile_is_listed_and_selected() {
        let (service, _events) = started(&scratch("profile-save"));
        let settings = Settings {
            font_ratio: 0.07,
            ..Settings::default()
        };
        let snapshot = service.save_profile("  配信用 ", settings).unwrap();
        assert_eq!(snapshot.profiles[0].name, "配信用");
        assert_eq!(snapshot.profiles[0].look.font_ratio, 0.07);
        assert_eq!(snapshot.settings.active_profile, "配信用");
    }

    #[test]
    fn saving_under_an_existing_name_is_refused_and_keeps_the_profile() {
        let (service, _events) = started(&scratch("profile-exists"));
        let first = Settings {
            font_ratio: 0.07,
            ..Settings::default()
        };
        service.save_profile("A", first).unwrap();
        assert_eq!(
            service
                .save_profile("A", Settings::default())
                .unwrap_err()
                .to_string(),
            "profile A already exists"
        );
        assert_eq!(
            service.snapshot().unwrap().profiles[0].look.font_ratio,
            0.07
        );
    }

    #[test]
    fn invalid_settings_cannot_become_a_profile() {
        let (service, _events) = started(&scratch("profile-invalid"));
        let invalid = Settings {
            palette: Vec::new(),
            ..Settings::default()
        };
        assert_eq!(
            service
                .save_profile("A", invalid.clone())
                .unwrap_err()
                .to_string(),
            "the speaker palette needs at least one colour"
        );
        assert_eq!(
            service
                .overwrite_profile("A", invalid)
                .unwrap_err()
                .to_string(),
            "the speaker palette needs at least one colour"
        );
        assert_eq!(service.snapshot().unwrap().profiles, Vec::<Profile>::new());
    }

    #[test]
    fn overwriting_replaces_the_look_and_needs_an_existing_name() {
        let (service, _events) = started(&scratch("profile-overwrite"));
        service.save_profile("A", Settings::default()).unwrap();
        let newer = Settings {
            font_ratio: 0.09,
            ..Settings::default()
        };
        let snapshot = service.overwrite_profile("A", newer.clone()).unwrap();
        assert_eq!(snapshot.profiles.len(), 1);
        assert_eq!(snapshot.profiles[0].look.font_ratio, 0.09);
        assert_eq!(
            service
                .overwrite_profile("B", newer)
                .unwrap_err()
                .to_string(),
            "profile B does not exist"
        );
    }

    #[test]
    fn loading_restores_the_look_and_keeps_the_speech_settings() {
        let (service, _events) = started(&scratch("profile-load"));
        let saved = Settings {
            font_ratio: 0.07,
            font_family: "Hiragino Sans".to_owned(),
            ..Settings::default()
        };
        service.save_profile("A", saved).unwrap();
        service
            .save_settings(Settings {
                font_ratio: 0.05,
                language: "ja".to_owned(),
                active_profile: String::new(),
                ..Settings::default()
            })
            .unwrap();
        let settings = service.load_profile("A").unwrap().settings;
        assert_eq!(settings.font_ratio, 0.07);
        assert_eq!(settings.font_family, "Hiragino Sans");
        assert_eq!(settings.language, "ja");
        assert_eq!(settings.active_profile, "A");
        assert_eq!(
            service.load_profile("B").unwrap_err().to_string(),
            "profile B does not exist"
        );
    }

    #[test]
    fn deleting_the_selected_profile_clears_the_selection_only() {
        let (service, _events) = started(&scratch("profile-delete"));
        let saved = Settings {
            font_ratio: 0.07,
            ..Settings::default()
        };
        service.save_profile("A", saved).unwrap();
        let snapshot = service.delete_profile("A").unwrap();
        assert_eq!(snapshot.profiles, Vec::<Profile>::new());
        assert_eq!(snapshot.settings.active_profile, "");
        assert_eq!(snapshot.settings.font_ratio, 0.07);
    }

    #[test]
    fn deleting_another_profile_keeps_the_selection() {
        let (service, _events) = started(&scratch("profile-delete-other"));
        service.save_profile("A", Settings::default()).unwrap();
        service.save_profile("B", Settings::default()).unwrap();
        let snapshot = service.delete_profile("A").unwrap();
        assert_eq!(snapshot.settings.active_profile, "B");
        assert_eq!(
            service.delete_profile("A").unwrap_err().to_string(),
            "profile A does not exist"
        );
    }

    #[test]
    fn dropped_subtitles_become_new_files_with_the_selected_profiles_look() {
        let root = scratch("reformat");
        let (service, events) = started(&root);
        let saved = Settings {
            font_family: "Hiragino Sans".to_owned(),
            placement: crate::settings::Placement {
                anchor: crate::settings::Anchor::VideoBottom,
                offset_x_percent: 0.0,
                offset_y_percent: 0.0,
            },
            ..Settings::default()
        };
        service.save_profile("A", saved).unwrap();
        // Changes on screen that were not saved to the profile must not reach the files.
        service
            .save_settings(Settings {
                font_family: "Unsaved".to_owned(),
                placement: crate::settings::Placement {
                    anchor: crate::settings::Anchor::VideoTop,
                    offset_x_percent: 0.0,
                    offset_y_percent: 0.0,
                },
                active_profile: "A".to_owned(),
                ..Settings::default()
            })
            .unwrap();
        write_transcript(&root.join("clip"));
        std::fs::write(root.join("clip.ttml"), b"keep me").unwrap();
        service.reformat_files(&[root.join("clip.ttml")]).unwrap();
        let outcome = next_outcome(&events);
        assert_eq!(outcome.base, root.join("clip-2"));
        assert_eq!(std::fs::read(root.join("clip.ttml")).unwrap(), b"keep me");
        let written = std::fs::read_to_string(root.join("clip-2.ttml")).unwrap();
        assert!(
            written.contains("tts:fontFamily=\"Hiragino Sans\""),
            "{written}"
        );
        let timeline = std::fs::read_to_string(root.join("clip-2.fcpxml")).unwrap();
        assert!(timeline.contains("font=\"Hiragino Sans\""), "{timeline}");
    }

    #[test]
    fn without_a_selection_dropped_subtitles_follow_the_current_settings() {
        let root = scratch("reformat-current");
        let (service, events) = started(&root);
        service
            .save_settings(Settings {
                font_family: "Current Font".to_owned(),
                placement: crate::settings::Placement {
                    anchor: crate::settings::Anchor::VideoTop,
                    offset_x_percent: 0.0,
                    offset_y_percent: 0.0,
                },
                ..Settings::default()
            })
            .unwrap();
        write_transcript(&root.join("clip"));
        service
            .reformat_files(&[root.join("clip.transcript.json")])
            .unwrap();
        assert_eq!(next_outcome(&events).base, root.join("clip-2"));
        let written = std::fs::read_to_string(root.join("clip-2.ttml")).unwrap();
        assert!(
            written.contains("tts:fontFamily=\"Current Font\""),
            "{written}"
        );
    }

    #[test]
    fn the_files_of_one_transcript_make_one_job() {
        let root = scratch("reformat-dedupe");
        let (service, _events) = started(&root);
        assert_eq!(
            service
                .reformat_files(&[
                    root.join("clip.ttml"),
                    root.join("clip.srt"),
                    root.join("clip.fcpxml"),
                    root.join("clip.transcript.json"),
                    root.join("other.srt"),
                ])
                .unwrap(),
            vec![1, 2]
        );
    }

    #[test]
    fn a_dropped_file_without_its_transcript_names_itself() {
        let root = scratch("reformat-missing");
        let (service, events) = started(&root);
        service.reformat_files(&[root.join("clip.srt")]).unwrap();
        assert!(matches!(
            next_job_event(&events),
            JobEvent::Queued { id: 1, .. }
        ));
        assert_eq!(
            serde_json::to_value(next_job_event(&events)).unwrap(),
            serde_json::json!({
                "state": "failed",
                "id": 1,
                "error": { "code": "transcript_missing", "file": "clip.srt" }
            })
        );
    }

    #[test]
    fn the_font_list_is_read_once() {
        let (service, _events) = started(&scratch("fonts"));
        service.shared.fonts.set(vec!["Only".to_owned()]).unwrap();
        assert_eq!(service.font_families(), vec!["Only".to_owned()]);
    }

    #[test]
    fn a_broken_profile_file_does_not_stop_the_app_but_is_not_overwritten() {
        let root = scratch("profile-broken");
        let (service, _events) = started(&root);
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(root.join("data/profiles.json"), b"{").unwrap();
        assert_eq!(service.snapshot().unwrap().profiles, Vec::<Profile>::new());
        assert_eq!(
            service
                .save_profile("A", Settings::default())
                .unwrap_err()
                .to_string(),
            "profiles file: EOF while parsing an object at line 1 column 1"
        );
        assert_eq!(
            service.delete_profile("A").unwrap_err().to_string(),
            "profiles file: EOF while parsing an object at line 1 column 1"
        );
        assert_eq!(
            std::fs::read(root.join("data/profiles.json")).unwrap(),
            b"{".to_vec()
        );
    }

    #[test]
    fn resetting_deletes_downloads_and_interrupted_files_but_no_user_folder() {
        let root = scratch("reset");
        let (service, _events) = started(&root);
        let model = root.join("data/models/nemotron-3-diarization/a.gguf");
        let interrupted = root.join("data/models/whisper-large-v3-turbo/model.safetensors.part");
        let runtime = root.join("data/runtime/geforce/nemo/x.dll");
        let external = root.join("user-models/a.gguf");
        std::fs::create_dir_all(model.parent().unwrap()).unwrap();
        std::fs::create_dir_all(interrupted.parent().unwrap()).unwrap();
        std::fs::create_dir_all(runtime.parent().unwrap()).unwrap();
        std::fs::create_dir_all(external.parent().unwrap()).unwrap();
        std::fs::write(&model, b"x").unwrap();
        std::fs::write(&interrupted, b"x").unwrap();
        std::fs::write(&runtime, b"x").unwrap();
        std::fs::write(&external, b"x").unwrap();
        service
            .save_settings(Settings {
                model_dirs: vec![root.join("user-models")],
                ..Settings::default()
            })
            .unwrap();
        service.reset_models().unwrap();
        assert!(!model.exists());
        assert!(!interrupted.exists());
        assert!(!runtime.exists());
        assert!(external.exists());
        assert!(root.join("data/settings.json").exists());
    }

    #[test]
    fn resetting_keeps_files_placed_by_hand_in_the_models_folder() {
        let root = scratch("reset-by-hand");
        let (service, _events) = started(&root);
        let loose = root.join("data/models/mine.gguf");
        let foreign = root.join("data/models/my-own/a.gguf");
        std::fs::create_dir_all(foreign.parent().unwrap()).unwrap();
        std::fs::write(&loose, b"x").unwrap();
        std::fs::write(&foreign, b"x").unwrap();
        service.reset_models().unwrap();
        assert!(loose.exists());
        assert!(foreign.exists());
    }

    #[cfg(unix)]
    #[test]
    fn resetting_never_follows_a_link_out_of_the_models_folder() {
        let root = scratch("reset-link");
        let (service, _events) = started(&root);
        let target = root.join("elsewhere/a.gguf");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"x").unwrap();
        std::fs::create_dir_all(root.join("data/models")).unwrap();
        std::os::unix::fs::symlink(
            root.join("elsewhere"),
            root.join("data/models/nemotron-3-diarization"),
        )
        .unwrap();
        service.reset_models().unwrap();
        assert!(target.exists());
    }

    #[test]
    fn resetting_keeps_a_catalog_folder_the_user_added_as_their_own() {
        let root = scratch("reset-overlap");
        let (service, _events) = started(&root);
        let inside = root.join("data/models/nemotron-3-diarization/a.gguf");
        let beside = root.join("data/models/whisper-large-v3-turbo/a.gguf");
        std::fs::create_dir_all(inside.parent().unwrap()).unwrap();
        std::fs::create_dir_all(beside.parent().unwrap()).unwrap();
        std::fs::write(&inside, b"x").unwrap();
        std::fs::write(&beside, b"x").unwrap();
        service
            .save_settings(Settings {
                model_dirs: vec![root.join("data/models/nemotron-3-diarization")],
                ..Settings::default()
            })
            .unwrap();
        service.reset_models().unwrap();
        assert!(inside.exists());
        assert!(!beside.exists());
    }

    #[test]
    fn resetting_keeps_everything_when_the_user_added_a_parent_of_the_app_folder() {
        let root = scratch("reset-parent");
        let (service, _events) = started(&root);
        let model = root.join("data/models/nemotron-3-diarization/a.gguf");
        std::fs::create_dir_all(model.parent().unwrap()).unwrap();
        std::fs::write(&model, b"x").unwrap();
        service
            .save_settings(Settings {
                model_dirs: vec![root.join("data")],
                ..Settings::default()
            })
            .unwrap();
        service.reset_models().unwrap();
        assert!(model.exists());
    }

    #[test]
    fn resetting_with_nothing_downloaded_is_fine() {
        let (service, _events) = started(&scratch("reset-empty"));
        assert!(service.reset_models().is_ok());
    }

    #[test]
    fn resetting_is_refused_while_a_job_runs() {
        let root = scratch("reset-busy-job");
        let (service, _events) = started(&root);
        let model = root.join("data/models/a/a.gguf");
        std::fs::create_dir_all(model.parent().unwrap()).unwrap();
        std::fs::write(&model, b"x").unwrap();
        service
            .shared
            .jobs
            .lock()
            .unwrap()
            .insert(7, CancelToken::default());
        assert_eq!(
            serde_json::to_value(service.reset_models().unwrap_err()).unwrap(),
            serde_json::json!({ "code": "models_busy" })
        );
        assert!(model.exists());
    }

    #[test]
    fn resetting_is_refused_while_a_download_runs() {
        let (service, _events) = started(&scratch("reset-busy-download"));
        service
            .shared
            .downloads
            .lock()
            .unwrap()
            .insert("m".to_owned(), CancelToken::default());
        assert_eq!(
            service.reset_models().unwrap_err().to_string(),
            "models cannot be reset while a job or a download is running"
        );
    }

    #[test]
    fn a_finished_download_is_forgotten_and_a_running_one_is_not_started_twice() {
        let (service, events) = started(&scratch("download-twice"));
        service
            .shared
            .downloads
            .lock()
            .unwrap()
            .insert(catalog::YUNET_ID.to_owned(), CancelToken::default());
        service.start_download(catalog::YUNET_ID).unwrap();
        assert!(events.recv_timeout(Duration::from_millis(200)).is_err());
        service.shared.downloads.lock().unwrap().clear();
        service.start_download(catalog::YUNET_ID).unwrap();
        assert!(matches!(
            events.recv_timeout(WAIT).unwrap(),
            AppEvent::Download(DownloadEvent::Finished { .. })
        ));
        assert_eq!(service.shared.downloads.lock().unwrap().len(), 0);
    }

    #[test]
    fn cancelling_known_work_sets_its_token() {
        let (service, _events) = started(&scratch("cancel-known"));
        let job = CancelToken::default();
        service.shared.jobs.lock().unwrap().insert(7, job.clone());
        let download = CancelToken::default();
        service
            .shared
            .downloads
            .lock()
            .unwrap()
            .insert("m".to_owned(), download.clone());
        service.cancel_job(7).unwrap();
        service.cancel_download("m").unwrap();
        assert!(job.is_cancelled());
        assert!(download.is_cancelled());
    }

    #[test]
    fn writing_a_transcript_again_never_replaces_the_earlier_files() {
        let root = scratch("relayout-new-name");
        let (service, events) = started(&root);
        let info = crate::engines::VideoInfo {
            width: 1280,
            height: 720,
            duration: 2.0,
            fps_num: 30,
            fps_den: 1,
        };
        let metrics = crate::captions::metrics_for(&info, &Settings::default());
        let transcript = crate::export::Transcript::new(
            root.join("clip.mp4"),
            info,
            metrics,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let written =
            crate::export::write_outputs(&root.join("clip"), &transcript, &Settings::default())
                .unwrap();
        assert_eq!(written.len(), 4);
        std::fs::write(root.join("clip.srt"), b"edited by hand").unwrap();
        let mut bottom = Settings::default();
        bottom.placement.anchor = crate::settings::Anchor::VideoBottom;
        service.save_settings(bottom).unwrap();
        service
            .relayout(&root.join("clip.transcript.json"))
            .unwrap();
        assert!(matches!(
            next_job_event(&events),
            JobEvent::Queued { id: 1, .. }
        ));
        let finished = std::iter::repeat_with(|| next_job_event(&events))
            .find(|event| !matches!(event, JobEvent::Running { .. }))
            .unwrap();
        let finished = serde_json::to_value(finished).unwrap();
        assert_eq!(
            finished["outcome"]["base"]
                .as_str()
                .and_then(|base| base.rsplit('/').next()),
            Some("clip-2"),
            "{finished}"
        );
        assert_eq!(
            std::fs::read(root.join("clip.srt")).unwrap(),
            b"edited by hand".to_vec()
        );
    }
}
