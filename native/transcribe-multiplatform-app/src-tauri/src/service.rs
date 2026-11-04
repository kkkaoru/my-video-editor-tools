//! Application behaviour behind the UI commands: settings, the job queue and downloads.
//! Nothing here knows about Tauri; events leave through a callback.

use crate::catalog::{self, Backend, Hardware, ModelEntry, ModelStatus};
use crate::download::{self, DownloadProgress};
use crate::error::{io_context, AppError, AppResult};
use crate::eta::Progress;
use crate::paths::AppPaths;
use crate::pipeline::{self, JobOutcome};
use crate::process::CancelToken;
use crate::settings::Settings;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};

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
    pub locale: String,
    pub hardware: Hardware,
    /// The speech-tool build jobs use on this machine now.
    pub backend: Backend,
    pub settings: Settings,
    pub models: Vec<ModelStatus>,
    pub models_dir: PathBuf,
}

enum JobRequest {
    Transcribe(PathBuf),
    Relayout(PathBuf),
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
        JobRequest::Relayout(file) => {
            pipeline::relayout(file, &shared.paths, &settings, &job.cancel, &mut report)
        }
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
    (shared.sink)(AppEvent::Download(event));
}

fn known_entry(id: &str) -> AppResult<&'static ModelEntry> {
    catalog::find_entry(id).ok_or_else(|| AppError::Invalid(format!("unknown model {id}")))
}

/// Deletes only the app's own copy; files found in other folders belong to the user.
fn remove_own_copy(entry: &ModelEntry, paths: &AppPaths) -> AppResult<()> {
    let folder = if entry.id == catalog::GEFORCE_RUNTIME_ID {
        paths.downloaded_runtime_dir().join("geforce")
    } else {
        paths.models_dir().join(entry.id)
    };
    if !folder.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(&folder).map_err(io_context(format!("remove {}", folder.display())))
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
            locale: self.locale.clone(),
            hardware: self.shared.hardware.clone(),
            backend: catalog::backend_for(&self.shared.hardware, &self.shared.paths),
            settings,
            models,
            models_dir: self.shared.paths.models_dir(),
        })
    }

    pub fn save_settings(&self, settings: Settings) -> AppResult<Snapshot> {
        settings.save(&self.shared.paths.settings_file())?;
        *locked(&self.shared.settings, "settings")? = settings;
        self.snapshot()
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
        self.enqueue(transcript, JobRequest::Relayout(transcript.to_path_buf()))
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
        locked(&self.shared.downloads, "download")?.insert(id.to_owned(), cancel.clone());
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
    fn relayout_of_a_missing_transcript_fails_with_internal_detail() {
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
            serde_json::to_value(next_job_event(&events)).unwrap()["error"]["code"],
            "internal"
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
}
