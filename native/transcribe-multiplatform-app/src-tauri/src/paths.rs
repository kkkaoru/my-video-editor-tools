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
}
