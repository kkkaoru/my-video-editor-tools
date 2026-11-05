//! Replaces the distributed app with a newer release: a signed package from this distribution's own manifest.

use crate::error::{AppError, AppResult, UserError};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Runtime, Url};
use tauri_plugin_updater::{Update, UpdaterExt};

const MANIFESTS: &str = "https://github.com/kkkaoru/my-video-editor-tools/releases/latest/download";
/// Whole steps of a download reported to the UI, so a large package does not flood it with events.
const PROGRESS_STEPS: f64 = 100.0;

/// The GPU build a Windows installer holds; updating across them would swap the speech runtime.
pub const DISTRIBUTION: &str = distribution(
    cfg!(feature = "windows-geforce"),
    cfg!(feature = "windows-radeon"),
);

const fn distribution(geforce: bool, radeon: bool) -> &'static str {
    if geforce {
        "windows-geforce"
    } else if radeon {
        "windows-radeon"
    } else {
        "macos"
    }
}

#[must_use]
pub fn endpoint(distribution: &str) -> String {
    format!("{MANIFESTS}/latest-{distribution}.json")
}

/// What the UI shows about a newer release.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub version: String,
    pub notes: Option<String>,
}

pub struct Updates {
    endpoint: Url,
    installing: AtomicBool,
}

impl Updates {
    pub fn new(endpoint: &str) -> AppResult<Self> {
        Ok(Self {
            endpoint: endpoint
                .parse()
                .map_err(|error| AppError::Invalid(format!("update address: {error}")))?,
            installing: AtomicBool::new(false),
        })
    }

    #[cfg(test)]
    pub(crate) fn endpoint(&self) -> &Url {
        &self.endpoint
    }
}

/// Lets go of the running install whichever way it ends.
struct Installing<'a>(&'a AtomicBool);

impl Drop for Installing<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn failed(error: tauri_plugin_updater::Error) -> AppError {
    AppError::Invalid(format!("update failed: {error}"))
}

async fn find<R: Runtime>(app: &AppHandle<R>, updates: &Updates) -> AppResult<Option<Update>> {
    app.updater_builder()
        .endpoints(vec![updates.endpoint.clone()])
        .map_err(failed)?
        .build()
        .map_err(failed)?
        .check()
        .await
        .map_err(failed)
}

/// The newer release, if there is one.
pub async fn check<R: Runtime>(
    app: &AppHandle<R>,
    updates: &Updates,
) -> AppResult<Option<AvailableUpdate>> {
    Ok(find(app, updates).await?.map(|update| AvailableUpdate {
        version: update.version,
        notes: update.body,
    }))
}

/// Turns bytes received into fractions, only when the whole size is known and a step was passed.
fn progress_steps(mut report: impl FnMut(f64)) -> impl FnMut(usize, Option<u64>) {
    let (mut received, mut last_step) = (0u64, 0.0);
    move |chunk, total| {
        received += chunk as u64;
        if let Some(total) = total.filter(|total| *total > 0) {
            let step = (received as f64 / total as f64 * PROGRESS_STEPS).floor();
            if step > last_step {
                last_step = step;
                report((step / PROGRESS_STEPS).min(1.0));
            }
        }
    }
}

/// Downloads the verified package and hands it to `apply`. Refused while work is running, also when
/// work started during the download; any failure leaves the app as it was.
pub async fn install<R: Runtime>(
    app: &AppHandle<R>,
    updates: &Updates,
    is_working: impl Fn() -> AppResult<bool>,
    report: impl FnMut(f64),
    apply: impl FnOnce(&Update, &[u8]) -> AppResult<()>,
) -> AppResult<()> {
    if is_working()? || updates.installing.swap(true, Ordering::SeqCst) {
        return Err(UserError::UpdateBusy.into());
    }
    let _installing = Installing(&updates.installing);
    let update = find(app, updates)
        .await?
        .ok_or_else(|| AppError::Invalid("no newer version is available".to_owned()))?;
    let bytes = update
        .download(progress_steps(report), || {})
        .await
        .map_err(failed)?;
    if is_working()? {
        return Err(UserError::UpdateBusy.into());
    }
    apply(&update, &bytes)
}

/// Windows leaves inside `install` for the installer, which starts the new version itself.
pub fn apply_and_restart<R: Runtime>(
    app: &AppHandle<R>,
    update: &Update,
    bytes: &[u8],
) -> AppResult<()> {
    update.install(bytes).map_err(failed)?;
    app.restart()
}

#[cfg(test)]
pub(crate) mod test_server {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Answers one request per response, in order, from a port on this computer.
    pub(crate) fn answering(responses: impl FnOnce(&str) -> Vec<(u16, Vec<u8>)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let responses = responses(&base);
        std::thread::spawn(move || {
            responses.into_iter().for_each(|(status, body)| {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request);
                let head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            });
        });
        format!("{base}/latest.json")
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::test_server::answering;
    use super::*;
    use serde_json::{json, Value};
    use std::cell::{Cell, RefCell};
    use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
    use tauri::App;

    /// A key made for this test and thrown away; these two sign `PAYLOAD` and nothing else.
    pub(crate) const TEST_PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDYzNTIyMzY1NEVDRDc0RUYKUldUdmRNMU9aU05TWTBRV2ZSVjR4c2lPNytST0J3SVpyd3pwSWZockdqclAramFMOEpZWGxWY2YK";
    const TEST_SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUdmRNMU9aU05TWStJL1RTaGlndkNIMGtHZDJ3WnRFakRzczB1ZVM4ckZCZUFGQUc4K3J5Q0hkOVUwQUVMeWd5Q29yQ0RpYVlZTTZqWDgvUGR3NU8ydXJGSG5uMTBKT1FNPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkzOTE3MjU0CWZpbGU6cGF5bG9hZApIcVA5dnV3VEZWVGlSWjk4anpYdWVLaFhJVkhLYXNBbkVtbWRGaUxld2hNNjBWUkxtZ0RmeEdMeUMwRkFCRmRkeWVJRS9SeTBiVDdvYnBvenF4YzFCQT09Cg==";
    const PAYLOAD: &[u8] = b"transcribe update fixture";

    /// The plugin as the app has it, except for the key and the plain HTTP this test's server speaks.
    pub(crate) fn updater_context() -> tauri::Context<MockRuntime> {
        let mut context = mock_context(noop_assets());
        context.config_mut().plugins.0.insert(
            "updater".to_owned(),
            json!({
                "pubkey": TEST_PUBLIC_KEY,
                "dangerousInsecureTransportProtocol": true,
            }),
        );
        context
    }

    fn app() -> App<MockRuntime> {
        mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(updater_context())
            .unwrap()
    }

    pub(crate) fn manifest(version: &str, url: &str, signature: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "version": version,
            "notes": "新しい版です",
            "url": url,
            "signature": signature,
        }))
        .unwrap()
    }

    /// Runs `install` against `updates`; returns its outcome, the bytes applied and the progress.
    fn installed(
        updates: &Updates,
        is_working: impl Fn() -> AppResult<bool>,
    ) -> (AppResult<()>, Option<Vec<u8>>, Vec<f64>) {
        let app = app();
        let applied = RefCell::new(None);
        let progress = RefCell::new(Vec::new());
        let outcome = tauri::async_runtime::block_on(install(
            app.handle(),
            updates,
            is_working,
            |fraction| progress.borrow_mut().push(fraction),
            |_, bytes| {
                applied.replace(Some(bytes.to_vec()));
                Ok(())
            },
        ));
        (outcome, applied.into_inner(), progress.into_inner())
    }

    fn signed_update(body: &[u8], signature: &'static str) -> String {
        let body = body.to_vec();
        answering(move |base| {
            let package = format!("{base}/package");
            vec![(200, manifest("99.0.0", &package, signature)), (200, body)]
        })
    }

    fn idle() -> AppResult<bool> {
        Ok(false)
    }

    #[test]
    fn each_distribution_has_its_own_manifest() {
        assert_eq!(
            endpoint("macos"),
            "https://github.com/kkkaoru/my-video-editor-tools/releases/latest/download/latest-macos.json"
        );
        assert_eq!(
            endpoint("windows-geforce"),
            "https://github.com/kkkaoru/my-video-editor-tools/releases/latest/download/latest-windows-geforce.json"
        );
        assert_eq!(
            endpoint("windows-radeon"),
            "https://github.com/kkkaoru/my-video-editor-tools/releases/latest/download/latest-windows-radeon.json"
        );
    }

    #[test]
    fn the_gpu_features_decide_the_distribution() {
        assert_eq!(distribution(false, false), "macos");
        assert_eq!(distribution(true, false), "windows-geforce");
        assert_eq!(distribution(false, true), "windows-radeon");
    }

    #[cfg(feature = "windows-geforce")]
    #[test]
    fn a_geforce_build_only_looks_at_the_geforce_manifest() {
        assert_eq!(DISTRIBUTION, "windows-geforce");
    }

    #[cfg(feature = "windows-radeon")]
    #[test]
    fn a_radeon_build_only_looks_at_the_radeon_manifest() {
        assert_eq!(DISTRIBUTION, "windows-radeon");
    }

    #[cfg(not(any(feature = "windows-geforce", feature = "windows-radeon")))]
    #[test]
    fn a_build_without_gpu_features_looks_at_the_macos_manifest() {
        assert_eq!(DISTRIBUTION, "macos");
    }

    #[test]
    fn the_shipped_config_verifies_with_the_release_key_and_keeps_every_guard() {
        let config: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let updater = &config["plugins"]["updater"];
        // The base64 of minisign's "untrusted comment: minisign public key".
        assert!(updater["pubkey"]
            .as_str()
            .unwrap()
            .starts_with("dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6"));
        assert_ne!(updater["pubkey"], TEST_PUBLIC_KEY);
        assert_eq!(updater["windows"]["installMode"], "passive");
        assert_eq!(updater.get("dangerousInsecureTransportProtocol"), None);
        assert_eq!(updater.get("dangerousAcceptInvalidCerts"), None);
        assert_eq!(updater.get("endpoints"), None);
        // Local builds must work without the signing key.
        assert_eq!(config["bundle"].get("createUpdaterArtifacts"), None);
    }

    #[test]
    fn an_address_that_is_not_a_url_is_refused() {
        assert!(Updates::new("not a url").is_err());
    }

    #[test]
    fn a_newer_release_reports_its_version_and_notes() {
        let url = answering(|_| {
            vec![(
                200,
                manifest("99.0.0", "http://127.0.0.1:9/package", TEST_SIGNATURE),
            )]
        });
        let found =
            tauri::async_runtime::block_on(check(app().handle(), &Updates::new(&url).unwrap()));
        assert_eq!(
            found.unwrap(),
            Some(AvailableUpdate {
                version: "99.0.0".to_owned(),
                notes: Some("新しい版です".to_owned()),
            })
        );
    }

    #[test]
    fn the_same_version_is_no_update() {
        let url = answering(|_| {
            vec![(
                200,
                manifest("0.0.1", "http://127.0.0.1:9/package", TEST_SIGNATURE),
            )]
        });
        let found =
            tauri::async_runtime::block_on(check(app().handle(), &Updates::new(&url).unwrap()));
        assert_eq!(found.unwrap(), None);
    }

    #[test]
    fn a_failed_check_is_an_error_and_nothing_more() {
        let url = answering(|_| vec![(500, Vec::new())]);
        let found =
            tauri::async_runtime::block_on(check(app().handle(), &Updates::new(&url).unwrap()));
        assert!(found.unwrap_err().to_string().starts_with("update failed:"));
    }

    #[test]
    fn a_verified_package_is_applied_with_its_progress() {
        let updates = Updates::new(&signed_update(PAYLOAD, TEST_SIGNATURE)).unwrap();
        let (outcome, applied, progress) = installed(&updates, idle);
        outcome.unwrap();
        assert_eq!(applied.as_deref(), Some(PAYLOAD));
        assert_eq!(progress.last(), Some(&1.0));
        assert!(progress.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn a_package_that_fails_verification_is_never_applied() {
        let updates = Updates::new(&signed_update(b"tampered", TEST_SIGNATURE)).unwrap();
        let (outcome, applied, _) = installed(&updates, idle);
        assert!(outcome
            .unwrap_err()
            .to_string()
            .starts_with("update failed:"));
        assert_eq!(applied, None);
    }

    #[test]
    fn a_failed_download_is_never_applied_and_a_new_attempt_may_start() {
        let url = answering(|base| {
            let package = format!("{base}/package");
            vec![
                (200, manifest("99.0.0", &package, TEST_SIGNATURE)),
                (500, Vec::new()),
                (200, manifest("0.0.1", &package, TEST_SIGNATURE)),
            ]
        });
        let updates = Updates::new(&url).unwrap();
        let (outcome, applied, _) = installed(&updates, idle);
        assert!(outcome
            .unwrap_err()
            .to_string()
            .starts_with("update failed:"));
        assert_eq!(applied, None);
        let (retried, _, _) = installed(&updates, idle);
        assert_eq!(
            retried.unwrap_err().to_string(),
            "no newer version is available"
        );
    }

    #[test]
    fn nothing_starts_while_work_is_running() {
        // No server: a refusal must not reach the network.
        let updates = Updates::new("http://127.0.0.1:9/latest.json").unwrap();
        let (outcome, applied, _) = installed(&updates, || Ok(true));
        assert_eq!(
            outcome.unwrap_err().to_string(),
            UserError::UpdateBusy.to_string()
        );
        assert_eq!(applied, None);
    }

    #[test]
    fn work_started_during_the_download_stops_the_update_before_it_is_applied() {
        let updates = Updates::new(&signed_update(PAYLOAD, TEST_SIGNATURE)).unwrap();
        let asked = Cell::new(0);
        let (outcome, applied, _) = installed(&updates, || {
            asked.set(asked.get() + 1);
            Ok(asked.get() > 1)
        });
        assert_eq!(
            outcome.unwrap_err().to_string(),
            UserError::UpdateBusy.to_string()
        );
        assert_eq!(applied, None);
    }

    #[test]
    fn a_second_install_is_refused_while_one_runs() {
        let updates = Updates::new("http://127.0.0.1:9/latest.json").unwrap();
        updates.installing.store(true, Ordering::SeqCst);
        let (outcome, _, _) = installed(&updates, idle);
        assert_eq!(
            outcome.unwrap_err().to_string(),
            UserError::UpdateBusy.to_string()
        );
        assert!(updates.installing.load(Ordering::SeqCst));
    }

    #[test]
    fn a_failure_to_tell_whether_work_runs_refuses_the_update() {
        let updates = Updates::new("http://127.0.0.1:9/latest.json").unwrap();
        let (outcome, _, _) = installed(&updates, || Err(AppError::Invalid("lock".to_owned())));
        assert_eq!(outcome.unwrap_err().to_string(), "lock");
        assert!(!updates.installing.load(Ordering::SeqCst));
    }

    #[test]
    fn progress_moves_in_whole_steps_and_needs_the_size() {
        let reports = RefCell::new(Vec::new());
        let mut steps = progress_steps(|fraction| reports.borrow_mut().push(fraction));
        steps(1, Some(1000));
        steps(9, Some(1000));
        steps(1, Some(1000));
        steps(490, Some(1000));
        steps(499, Some(1000));
        steps(10, None);
        steps(10, Some(0));
        drop(steps);
        assert_eq!(reports.into_inner(), vec![0.01, 0.5, 1.0]);
    }
}
