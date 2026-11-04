//! Where the app keeps data and finds its bundled files.

use crate::error::{io_context, AppError, AppResult};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const APP_IDENTIFIER: &str = "net.teadea.transcribe";
const DATA_DIR_OVERRIDE: &str = "TRANSCRIBE_DATA_DIR";
const RESOURCE_DIR_OVERRIDE: &str = "TRANSCRIBE_RESOURCE_DIR";
/// The per-user base folder: an environment variable and the path below it.
const DATA_BASE: (&str, &str) = if cfg!(windows) {
    ("APPDATA", "")
} else {
    ("HOME", "Library/Application Support")
};
const HOME_VARIABLE: &str = if cfg!(windows) { "USERPROFILE" } else { "HOME" };

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub data: PathBuf,
    pub resources: PathBuf,
    /// The Hugging Face cache, where earlier downloads from any tool may already sit.
    pub hf_hub: Option<PathBuf>,
}

impl AppPaths {
    pub fn detect() -> AppResult<Self> {
        let exe = std::env::current_exe().map_err(io_context("locate executable"))?;
        Self::resolve(&exe, |name| std::env::var_os(name))
    }

    fn resolve(exe: &Path, env: impl Fn(&str) -> Option<OsString>) -> AppResult<Self> {
        let exe_dir = exe
            .parent()
            .ok_or_else(|| AppError::Invalid("executable has no parent".into()))?;
        let data = match env(DATA_DIR_OVERRIDE) {
            Some(value) => PathBuf::from(value),
            None => default_data_dir(&env)?,
        };
        let resources =
            env(RESOURCE_DIR_OVERRIDE).map_or_else(|| bundled_resource_dir(exe_dir), PathBuf::from);
        Ok(Self {
            data,
            resources,
            hf_hub: huggingface_hub_dir(&env),
        })
    }

    #[must_use]
    pub fn settings_file(&self) -> PathBuf {
        self.data.join("settings.json")
    }

    #[must_use]
    pub fn stage_rates_file(&self) -> PathBuf {
        self.data.join("stage-rates.json")
    }

    #[must_use]
    pub fn models_dir(&self) -> PathBuf {
        self.data.join("models")
    }

    #[must_use]
    pub fn jobs_dir(&self) -> PathBuf {
        self.data.join("jobs")
    }

    #[must_use]
    pub fn downloaded_runtime_dir(&self) -> PathBuf {
        self.data.join("runtime")
    }

    #[must_use]
    pub fn bundled_runtime_dir(&self) -> PathBuf {
        self.resources.join("runtime")
    }

    #[must_use]
    pub fn yunet_model(&self) -> PathBuf {
        self.resources
            .join("resources")
            .join("face_detection_yunet_2023mar.onnx")
    }
}

fn default_data_dir(env: &impl Fn(&str) -> Option<OsString>) -> AppResult<PathBuf> {
    let base =
        env(DATA_BASE.0).ok_or_else(|| AppError::Invalid(format!("{} is not set", DATA_BASE.0)))?;
    Ok(PathBuf::from(base).join(DATA_BASE.1).join(APP_IDENTIFIER))
}

/// macOS bundles keep resources in `Contents/Resources`; Windows keeps them beside the executable.
fn bundled_resource_dir(exe_dir: &Path) -> PathBuf {
    let is_mac_bundle = exe_dir.ends_with("Contents/MacOS");
    match exe_dir.parent() {
        Some(contents) if is_mac_bundle => contents.join("Resources"),
        _ => exe_dir.to_path_buf(),
    }
}

fn huggingface_hub_dir(env: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    env("HF_HUB_CACHE")
        .map(PathBuf::from)
        .or_else(|| env("HF_HOME").map(|home| PathBuf::from(home).join("hub")))
        .or_else(|| {
            env(HOME_VARIABLE).map(|home| PathBuf::from(home).join(".cache/huggingface/hub"))
        })
}

/// Replaces `path` only after the new content is complete, so a crash cannot truncate it.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(io_context(format!("create {}", parent.display())))?;
    }
    let mut partial: OsString = path.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    std::fs::write(&partial, bytes).map_err(io_context(format!("write {}", partial.display())))?;
    std::fs::rename(&partial, path).map_err(io_context(format!("publish {}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_bundle_uses_contents_resources() {
        assert_eq!(
            bundled_resource_dir(Path::new("/Applications/T.app/Contents/MacOS")),
            PathBuf::from("/Applications/T.app/Contents/Resources")
        );
    }

    #[test]
    fn other_layouts_use_the_executable_folder() {
        assert_eq!(
            bundled_resource_dir(Path::new("/opt/t")),
            PathBuf::from("/opt/t")
        );
    }

    #[test]
    fn subfolders_hang_off_the_data_directory() {
        let paths = AppPaths {
            data: PathBuf::from("/d"),
            resources: PathBuf::from("/r"),
            hf_hub: None,
        };
        assert_eq!(paths.models_dir(), PathBuf::from("/d/models"));
        assert_eq!(paths.jobs_dir(), PathBuf::from("/d/jobs"));
        assert_eq!(paths.settings_file(), PathBuf::from("/d/settings.json"));
        assert_eq!(paths.downloaded_runtime_dir(), PathBuf::from("/d/runtime"));
        assert_eq!(paths.bundled_runtime_dir(), PathBuf::from("/r/runtime"));
        assert_eq!(
            paths.yunet_model(),
            PathBuf::from("/r/resources/face_detection_yunet_2023mar.onnx")
        );
    }

    fn lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|pair| pair.0 == name)
                .map(|pair| OsString::from(pair.1))
        }
    }

    #[test]
    fn overrides_win_over_the_defaults() {
        let env = lookup(&[
            ("TRANSCRIBE_DATA_DIR", "/data"),
            ("TRANSCRIBE_RESOURCE_DIR", "/res"),
            ("HF_HUB_CACHE", "/hub"),
            ("HF_HOME", "/hf"),
            ("HOME", "/home"),
        ]);
        let paths = AppPaths::resolve(Path::new("/opt/t/app"), env).unwrap();
        assert_eq!(paths.data, PathBuf::from("/data"));
        assert_eq!(paths.resources, PathBuf::from("/res"));
        assert_eq!(paths.hf_hub, Some(PathBuf::from("/hub")));
    }

    #[cfg(unix)]
    #[test]
    fn defaults_hang_off_home_and_the_executable() {
        let paths =
            AppPaths::resolve(Path::new("/opt/t/app"), lookup(&[("HOME", "/home/u")])).unwrap();
        assert_eq!(
            paths.data,
            PathBuf::from("/home/u/Library/Application Support/net.teadea.transcribe")
        );
        assert_eq!(paths.resources, PathBuf::from("/opt/t"));
        assert_eq!(
            paths.hf_hub,
            Some(PathBuf::from("/home/u/.cache/huggingface/hub"))
        );
    }

    #[test]
    fn hf_home_is_used_when_no_cache_is_named() {
        let paths = AppPaths::resolve(
            Path::new("/opt/t/app"),
            lookup(&[("TRANSCRIBE_DATA_DIR", "/data"), ("HF_HOME", "/hf")]),
        )
        .unwrap();
        assert_eq!(paths.hf_hub, Some(PathBuf::from("/hf/hub")));
    }

    #[test]
    fn no_home_means_no_cache_but_an_explicit_data_folder_still_works() {
        let paths = AppPaths::resolve(
            Path::new("/opt/t/app"),
            lookup(&[("TRANSCRIBE_DATA_DIR", "/data")]),
        )
        .unwrap();
        assert_eq!(paths.hf_hub, None);
    }

    #[cfg(unix)]
    #[test]
    fn no_home_and_no_data_folder_is_an_error() {
        let error = AppPaths::resolve(Path::new("/opt/t/app"), lookup(&[]))
            .unwrap_err()
            .to_string();
        assert_eq!(error, "HOME is not set");
    }

    #[test]
    fn an_executable_without_a_folder_is_an_error() {
        let error = AppPaths::resolve(Path::new("/"), lookup(&[]))
            .unwrap_err()
            .to_string();
        assert_eq!(error, "executable has no parent");
    }

    #[test]
    fn this_process_resolves_its_paths() {
        assert!(AppPaths::detect().is_ok());
    }

    #[test]
    fn atomic_writes_create_the_folder_and_leave_no_partial_file() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/paths-atomic");
        let _ = std::fs::remove_dir_all(&root);
        let file = root.join("a/b.txt");
        write_atomic(&file, b"one").unwrap();
        write_atomic(&file, b"two").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"two".to_vec());
        assert!(!root.join("a/b.txt.partial").exists());
    }

    #[test]
    fn an_atomic_write_under_a_file_fails_with_the_step() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/paths-under-file");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("file"), b"x").unwrap();
        let error = write_atomic(&root.join("file/b.txt"), b"one")
            .unwrap_err()
            .to_string();
        assert!(error.starts_with("create "), "{error}");
    }
}
