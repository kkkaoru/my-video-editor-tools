use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

/// Which memory a model lacks: RAM and GPU memory are separate, except on a Mac, where one holds both.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Ram,
    GpuMemory,
    Unified,
}

impl std::fmt::Display for MemoryKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Ram => "RAM",
            Self::GpuMemory => "GPU memory",
            Self::Unified => "unified memory",
        })
    }
}

/// Failures the user can act on. The UI translates each code into the OS language.
#[derive(Clone, Debug, thiserror::Error, Serialize, PartialEq, Eq)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum UserError {
    #[error("the file has no video stream")]
    NoVideoStream,
    #[error("the file has no audio stream")]
    NoAudioStream,
    #[error("the video size could not be read")]
    UnreadableSize,
    #[error("the video duration could not be read")]
    UnreadableDuration,
    #[error("no words were detected in the audio")]
    NoWords,
    #[error("model {model} is not installed")]
    ModelMissing { model: String },
    #[error("no model is installed for {role}")]
    NoModelForRole { role: String },
    #[error("model {model} needs {required} GiB of {kind}, this machine has {available} GiB")]
    NeedsMemory {
        model: String,
        required: u64,
        available: u64,
        kind: MemoryKind,
    },
    #[error("no transcript was found beside {file}")]
    TranscriptMissing { file: String },
    #[error("profile {name} already exists")]
    ProfileExists { name: String },
    #[error("profile {name} does not exist")]
    ProfileMissing { name: String },
    #[error("{file} is not a valid profile file")]
    ProfileInvalid { file: String },
    #[error("models cannot be reset while a job or a download is running")]
    ModelsBusy,
    #[error("an update cannot be applied while a job, a download or another update is running")]
    UpdateBusy,
    #[error("separating speakers needs models that are missing: {missing:?}")]
    SpeakerModelsMissing {
        missing: Vec<crate::catalog::Role>,
        /// The model to fetch first, when one would do.
        suggested: Option<String>,
    },
}

/// Every failure the app reports; messages reach the UI, so they name the failed step.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{context}: {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },
    #[error(transparent)]
    User(#[from] UserError),
    #[error("{0}")]
    Invalid(String),
    #[error("{tool} failed: {detail}")]
    Tool { tool: String, detail: String },
    #[error("download failed: {0}")]
    Download(String),
    /// The connection was lost: what arrived is kept, so trying again resumes.
    #[error("download failed: {0}")]
    Interrupted(String),
    #[error("cancelled")]
    Cancelled,
}

pub type AppResult<T> = Result<T, AppError>;

/// Attaches the failed step to an I/O error.
pub fn io_context(context: impl Into<String>) -> impl FnOnce(std::io::Error) -> AppError {
    let context = context.into();
    move |source| AppError::Io { context, source }
}

/// User errors keep their code; everything else reaches the UI as untranslated technical detail.
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Self::User(error) = self {
            return error.serialize(serializer);
        }
        let code = if matches!(self, Self::Cancelled) {
            "cancelled"
        } else {
            "internal"
        };
        let mut object = serializer.serialize_struct("AppError", 2)?;
        object.serialize_field("code", code)?;
        object.serialize_field("detail", &self.to_string())?;
        object.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_context_names_the_failed_step() {
        let error = io_context("read model")(std::io::Error::other("boom"));
        assert_eq!(error.to_string(), "read model: boom");
    }

    #[test]
    fn cancelled_serializes_with_its_code() {
        assert_eq!(
            serde_json::to_string(&AppError::Cancelled).unwrap(),
            r#"{"code":"cancelled","detail":"cancelled"}"#
        );
    }

    #[test]
    fn user_errors_serialize_as_translatable_codes() {
        let error = AppError::from(UserError::NeedsMemory {
            model: "m".to_owned(),
            required: 24,
            available: 12,
            kind: MemoryKind::GpuMemory,
        });
        assert_eq!(
            serde_json::to_string(&error).unwrap(),
            r#"{"code":"needs_memory","model":"m","required":24,"available":12,"kind":"gpu_memory"}"#
        );
        assert_eq!(
            error.to_string(),
            "model m needs 24 GiB of GPU memory, this machine has 12 GiB"
        );
    }

    #[test]
    fn other_errors_serialize_as_internal_detail() {
        let error = AppError::Tool {
            tool: "ffmpeg".to_owned(),
            detail: "boom".to_owned(),
        };
        assert_eq!(
            serde_json::to_string(&error).unwrap(),
            r#"{"code":"internal","detail":"ffmpeg failed: boom"}"#
        );
    }
}
