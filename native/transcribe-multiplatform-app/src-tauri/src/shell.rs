//! Tauri wiring: thin commands over `Service`, and events forwarded to the window.

use crate::catalog;
use crate::error::AppResult;
use crate::paths::AppPaths;
use crate::service::{AppEvent, EventSink, Service, Snapshot};
use crate::settings::Settings;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::ipc::Invoke;
use tauri::{Builder, Emitter, Manager, Runtime, State};

const JOB_EVENT: &str = "job";
const DOWNLOAD_EVENT: &str = "download";

#[tauri::command]
fn snapshot(service: State<'_, Service>) -> AppResult<Snapshot> {
    service.snapshot()
}

#[tauri::command]
fn save_settings(service: State<'_, Service>, settings: Settings) -> AppResult<Snapshot> {
    service.save_settings(settings)
}

#[tauri::command]
fn add_videos(service: State<'_, Service>, paths: Vec<PathBuf>) -> AppResult<Vec<u64>> {
    service.add_videos(&paths)
}

#[tauri::command]
fn reformat_files(service: State<'_, Service>, paths: Vec<PathBuf>) -> AppResult<Vec<u64>> {
    service.reformat_files(&paths)
}

#[tauri::command]
fn save_profile(
    service: State<'_, Service>,
    name: String,
    settings: Settings,
) -> AppResult<Snapshot> {
    service.save_profile(&name, settings)
}

#[tauri::command]
fn overwrite_profile(
    service: State<'_, Service>,
    name: String,
    settings: Settings,
) -> AppResult<Snapshot> {
    service.overwrite_profile(&name, settings)
}

#[tauri::command]
fn load_profile(service: State<'_, Service>, name: String) -> AppResult<Snapshot> {
    service.load_profile(&name)
}

#[tauri::command]
fn delete_profile(service: State<'_, Service>, name: String) -> AppResult<Snapshot> {
    service.delete_profile(&name)
}

/// Off the main thread: the first call reads the header of every installed font.
#[tauri::command(async)]
fn font_families(service: State<'_, Service>) -> Vec<String> {
    service.font_families()
}

/// Off the main thread: model folders can be large.
#[tauri::command(async)]
fn reset_models(service: State<'_, Service>) -> AppResult<Snapshot> {
    service.reset_models()
}

#[tauri::command]
fn relayout_transcript(service: State<'_, Service>, path: PathBuf) -> AppResult<u64> {
    service.relayout(&path)
}

#[tauri::command]
fn cancel_job(service: State<'_, Service>, id: u64) -> AppResult<()> {
    service.cancel_job(id)
}

#[tauri::command]
fn start_download(service: State<'_, Service>, id: String) -> AppResult<()> {
    service.start_download(&id)
}

#[tauri::command]
fn cancel_download(service: State<'_, Service>, id: String) -> AppResult<()> {
    service.cancel_download(&id)
}

#[tauri::command]
fn delete_model(service: State<'_, Service>, id: String) -> AppResult<Snapshot> {
    service.delete_model(&id)
}

fn handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        snapshot,
        save_settings,
        add_videos,
        relayout_transcript,
        reformat_files,
        save_profile,
        overwrite_profile,
        load_profile,
        delete_profile,
        font_families,
        reset_models,
        cancel_job,
        start_download,
        cancel_download,
        delete_model
    ]
}

fn event_sink<R: Runtime>(app: tauri::AppHandle<R>) -> EventSink {
    Arc::new(move |event| {
        let delivered = match event {
            AppEvent::Job(payload) => app.emit(JOB_EVENT, payload),
            AppEvent::Download(payload) => app.emit(DOWNLOAD_EVENT, payload),
        };
        if let Err(error) = delivered {
            eprintln!("event delivery failed: {error}");
        }
    })
}

fn start_service<R: Runtime>(app: &tauri::AppHandle<R>, paths: AppPaths) -> AppResult<Service> {
    let settings = Settings::load(&paths.settings_file())?;
    Ok(Service::start(
        paths,
        catalog::detect_hardware()?,
        settings,
        sys_locale::get_locale(),
        event_sink(app.clone()),
    ))
}

fn manage_service<R: Runtime>(app: &tauri::AppHandle<R>, paths: AppPaths) -> AppResult<()> {
    app.manage(start_service(app, paths)?);
    Ok(())
}

/// Everything except the real window runtime, so the mock runtime can exercise it.
fn configure<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(handler())
}

fn launch<R: Runtime>(
    builder: Builder<R>,
    context: tauri::Context<R>,
    paths: AppPaths,
) -> AppResult<()> {
    configure(builder)
        .setup(move |app| manage_service(app.handle(), paths).map_err(Into::into))
        .run(context)
        .map_err(|error| crate::error::AppError::Invalid(format!("window runtime: {error}")))
}

/// Starts the desktop app.
pub fn run() -> AppResult<()> {
    launch(
        Builder::default(),
        tauri::generate_context!(),
        AppPaths::detect()?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
    use tauri::webview::InvokeRequest;
    use tauri::{Listener, WebviewWindow, WebviewWindowBuilder};

    fn app_window(
        name: &str,
    ) -> (
        tauri::App<tauri::test::MockRuntime>,
        WebviewWindow<tauri::test::MockRuntime>,
    ) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target-test")
            .join(format!("shell-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        let paths = AppPaths {
            data: root.join("data"),
            resources: root.join("resources"),
            hf_hub: None,
        };
        let app = configure(mock_builder())
            .build(mock_context(noop_assets()))
            .unwrap();
        manage_service(app.handle(), paths).unwrap();
        let window = WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        (app, window)
    }

    fn call(
        window: &WebviewWindow<tauri::test::MockRuntime>,
        command: &str,
        body: Value,
    ) -> Result<Value, Value> {
        let request = InvokeRequest {
            cmd: command.to_owned(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body: InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_owned(),
        };
        get_ipc_response(window, request).map(|response| response.deserialize::<Value>().unwrap())
    }

    #[test]
    fn snapshot_reaches_the_ui_with_a_locale_and_ten_colours() {
        let (_app, window) = app_window("snapshot");
        let snapshot = call(&window, "snapshot", json!({})).unwrap();
        assert_eq!(
            snapshot["settings"]["palette"].as_array().unwrap().len(),
            10
        );
        assert!(snapshot["locale"].is_string());
    }

    #[test]
    fn settings_commands_validate() {
        let (_app, window) = app_window("settings");
        let mut settings = call(&window, "snapshot", json!({})).unwrap()["settings"].clone();
        settings["font_ratio"] = json!(0.05);
        let saved = call(&window, "save_settings", json!({ "settings": settings })).unwrap();
        assert_eq!(saved["settings"]["font_ratio"], json!(0.05));
    }

    #[test]
    fn job_commands_queue_and_emit() {
        let (app, window) = app_window("jobs");
        let (sender, receiver) = std::sync::mpsc::channel();
        app.listen(JOB_EVENT, move |event| {
            let _ = sender.send(event.payload().to_owned());
        });
        assert_eq!(
            call(&window, "add_videos", json!({ "paths": ["/nope.mp4"] })).unwrap(),
            json!([1])
        );
        assert_eq!(
            call(
                &window,
                "relayout_transcript",
                json!({ "path": "/nope.transcript.json" })
            )
            .unwrap(),
            json!(2)
        );
        assert_eq!(
            call(&window, "cancel_job", json!({ "id": 2 })).unwrap(),
            Value::Null
        );
        let first = receiver
            .recv_timeout(std::time::Duration::from_secs(20))
            .unwrap();
        assert!(first.starts_with(r#"{"state":"queued""#), "{first}");
    }

    #[test]
    fn model_commands_report_errors_as_codes() {
        let (app, window) = app_window("models");
        let (sender, receiver) = std::sync::mpsc::channel();
        app.listen(DOWNLOAD_EVENT, move |event| {
            let _ = sender.send(event.payload().to_owned());
        });
        let error = call(&window, "start_download", json!({ "id": "nope" })).unwrap_err();
        assert_eq!(
            error,
            json!({ "code": "internal", "detail": "unknown model nope" })
        );
        assert_eq!(
            call(&window, "start_download", json!({ "id": "yunet" })).unwrap(),
            Value::Null
        );
        let finished = receiver
            .recv_timeout(std::time::Duration::from_secs(20))
            .unwrap();
        assert_eq!(finished, r#"{"state":"finished","id":"yunet"}"#);
        assert_eq!(
            call(&window, "cancel_download", json!({ "id": "yunet" })).unwrap(),
            Value::Null
        );
        assert!(
            call(&window, "delete_model", json!({ "id": "yunet" })).unwrap()["models"].is_array()
        );
    }

    #[test]
    fn profile_commands_save_load_overwrite_and_delete() {
        let (_app, window) = app_window("profiles");
        let settings = call(&window, "snapshot", json!({})).unwrap()["settings"].clone();
        let saved = call(
            &window,
            "save_profile",
            json!({ "name": "A", "settings": settings }),
        )
        .unwrap();
        assert_eq!(saved["profiles"][0]["name"], json!("A"));
        assert_eq!(saved["settings"]["active_profile"], json!("A"));
        let refused = call(
            &window,
            "save_profile",
            json!({ "name": "A", "settings": settings }),
        )
        .unwrap_err();
        assert_eq!(refused, json!({ "code": "profile_exists", "name": "A" }));
        let mut newer = settings.clone();
        newer["font_ratio"] = json!(0.07);
        let overwritten = call(
            &window,
            "overwrite_profile",
            json!({ "name": "A", "settings": newer }),
        )
        .unwrap();
        assert_eq!(
            overwritten["profiles"][0]["look"]["font_ratio"],
            json!(0.07)
        );
        let loaded = call(&window, "load_profile", json!({ "name": "A" })).unwrap();
        assert_eq!(loaded["settings"]["font_ratio"], json!(0.07));
        let deleted = call(&window, "delete_profile", json!({ "name": "A" })).unwrap();
        assert_eq!(deleted["profiles"], json!([]));
    }

    #[test]
    fn reformatting_queues_one_job_per_file_and_reports_what_is_missing() {
        let (app, window) = app_window("reformat");
        let (sender, receiver) = std::sync::mpsc::channel();
        app.listen(JOB_EVENT, move |event| {
            let _ = sender.send(event.payload().to_owned());
        });
        assert_eq!(
            call(&window, "reformat_files", json!({ "paths": ["/nope.srt"] })).unwrap(),
            json!([1])
        );
        let queued = receiver
            .recv_timeout(std::time::Duration::from_secs(20))
            .unwrap();
        assert_eq!(queued, r#"{"state":"queued","id":1,"path":"/nope.srt"}"#);
        let failed = receiver
            .recv_timeout(std::time::Duration::from_secs(20))
            .unwrap();
        assert_eq!(
            failed,
            r#"{"state":"failed","id":1,"error":{"code":"transcript_missing","file":"nope.srt"}}"#
        );
    }

    #[test]
    fn the_reset_and_font_commands_answer() {
        let (_app, window) = app_window("reset");
        assert!(call(&window, "reset_models", json!({})).unwrap()["models"].is_array());
        assert!(call(&window, "font_families", json!({}))
            .unwrap()
            .is_array());
    }

    #[test]
    fn launching_manages_the_service_before_the_loop_runs() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target-test/shell-launch");
        let _ = std::fs::remove_dir_all(&root);
        let paths = AppPaths {
            data: root.join("data"),
            resources: root.join("resources"),
            hf_hub: None,
        };
        let managed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let seen = std::sync::Arc::clone(&managed);
        // Closing the only window on Ready is what ends the mock event loop.
        let closer = tauri::plugin::Builder::<tauri::test::MockRuntime, ()>::new("closer")
            .on_event(move |app, event| {
                if matches!(event, tauri::RunEvent::Ready) {
                    seen.store(
                        app.try_state::<Service>().is_some(),
                        std::sync::atomic::Ordering::SeqCst,
                    );
                    // `on_event` holds the plugin lock that creating a window needs.
                    let handle = app.clone();
                    std::thread::spawn(move || {
                        let window = WebviewWindowBuilder::new(&handle, "main", Default::default())
                            .build()
                            .unwrap();
                        window.destroy().unwrap();
                    });
                }
            })
            .build();
        launch(
            mock_builder().plugin(closer),
            mock_context(noop_assets()),
            paths,
        )
        .unwrap();
        assert!(managed.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn an_unreadable_settings_file_stops_the_service_from_starting() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target-test/shell-bad-settings");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(root.join("data/settings.json"), b"{").unwrap();
        let paths = AppPaths {
            data: root.join("data"),
            resources: root.join("resources"),
            hf_hub: None,
        };
        let app = configure(mock_builder())
            .build(mock_context(noop_assets()))
            .unwrap();
        assert_eq!(
            manage_service(app.handle(), paths).unwrap_err().to_string(),
            "settings file: EOF while parsing an object at line 1 column 1"
        );
    }
}
