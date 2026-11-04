//! Downloadable models and runtimes, per hardware, and detection of copies already on disk.

use crate::error::{AppError, AppResult, UserError};
use crate::paths::{huggingface_hub_dir, AppPaths};
use crate::process::{background_command, run_capture};
use serde::Serialize;
use std::path::{Path, PathBuf};

const HF_RESOLVE: &str = "https://huggingface.co";
const GIB: u64 = 1 << 30;
const MEMORY_QUERY: (&str, &[&str]) = if cfg!(windows) {
    ("powershell", &["-NoProfile", "-Command", "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory"])
} else {
    ("/usr/sbin/sysctl", &["-n", "hw.memsize"])
};
const SEARCH_DEPTH: usize = 4;
const WHISPER_REPO: &str = "openai/whisper-large-v3-turbo";
const WHISPER_REVISION: &str = "41f01f3fe87f28c78e2fbf8b568835947dd65ed9";
const NEMOTRON_REPO: &str = "AtomicChat/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-GGUF";
const NEMOTRON_REVISION: &str = "dbd06c5cad7a2f33c8e06be2fb3ef814dbd9aa59";
const NEMOTRON_SOURCE: &str = "https://huggingface.co/nvidia/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B";
const DIARIZATION_REPO: &str = "nvidia/Nemotron-3-Diarization";
const DIARIZATION_REVISION: &str = "f667ed73aee57d40cc39428eb768b4fd87a0a29e";
const ENCODER: FileSpec = FileSpec {
    url_kind: UrlKind::HuggingFace { repo: NEMOTRON_REPO, revision: NEMOTRON_REVISION },
    path: "NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Encoder-F16.gguf",
    size: 1_670_410_080,
    sha256: "4221029c8a565c4a80c507ffa760c716e595acf663f9f4273ce69910050e7ba2",
};
/// The converted ggml model whisper.cpp reads; made once from the checkpoint.
pub const WHISPER_GGML_NAME: &str = "ggml-large-v3-turbo.bin";
/// Ready ggml files from other tools are used as they are.
const WHISPER_GGML_ALIASES: [&str; 3] = [WHISPER_GGML_NAME, "ggml-large-v3-turbo-q8_0.bin", "ggml-large-v3-turbo-q5_0.bin"];
/// The MLX checkpoint this repository's Python pipeline already downloads.
const WHISPER_MLX_NAME: &str = "weights.safetensors";
const WHISPER_MLX_SIZE: u64 = 1_613_977_612;
pub const WHISPER_ID: &str = "whisper-large-v3-turbo";
pub const YUNET_ID: &str = "yunet";
pub const AVATAR_YOLO_ID: &str = "avatar-face-yolo";
const AVATAR_YOLO_NAME: &str = "avatar-face-caption-v2.onnx";
pub const GEFORCE_RUNTIME_ID: &str = "geforce-runtime";
const NEMO_DIARIZATION_ID: &str = "nemotron-3-diarization";
const IQ4_ID: &str = "nemotron-3.5-transcribe-iq4";
const NVFP4_ID: &str = "nemotron-3.5-transcribe-nvfp4";
const Q2M_ID: &str = "nemotron-3.5-transcribe-q2m";
const Q2L_ID: &str = "nemotron-3.5-transcribe-q2l";
const Q8_ID: &str = "nemotron-3.5-transcribe-q8";
const NEMO_CUDA_URL: &str =
    "https://github.com/NVIDIA/NeMo-Speech.cpp/releases/download/v0.3.0/nemo-speech-0.3.0-windows-x86_64-cuda.zip";
const WHISPER_CUDA_URL: &str =
    "https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-bin-win-cuda-12.4.0-x64.zip";
/// Settings for 16 GB GPUs, documented by the GGUF publisher; `max_context` holds about 7 minutes.
const Q2M_ARGS: [&str; 12] = [
    "--asr.salm.max_sequences", "2",
    "--asr.salm.n_ctx_per_sequence", "8192",
    "--asr.salm.max_context", "8192",
    "--asr.salm.mtp_draft", "0",
    "--asr.salm.n_batch", "512",
    "--asr.salm.encoder_max_batch", "2",
];
/// Blackwell GPUs are the ones with native NVFP4 kernels.
const BLACKWELL_MARKERS: [&str; 3] = ["RTX 50", "RTX PRO 6000 Blackwell", "DGX Spark"];

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Transcribe,
    Diarize,
    Face,
    Runtime,
}

/// How `nemo-speech` must be driven for a diarization model.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiarizeKind {
    /// Standalone Sortformer-family model: `nemo-speech diarize`.
    Sortformer,
    /// Speech-LLM with a speaker branch: `nemo-speech transcribe --diarize`.
    SpeechLlm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UrlKind {
    HuggingFace { repo: &'static str, revision: &'static str },
    Direct(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileSpec {
    url_kind: UrlKind,
    pub path: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

impl FileSpec {
    #[must_use]
    pub fn url(&self) -> String {
        match self.url_kind {
            UrlKind::HuggingFace { repo, revision } => format!("{HF_RESOLVE}/{repo}/resolve/{revision}/{}", self.path),
            UrlKind::Direct(url) => url.to_owned(),
        }
    }

    /// The Hugging Face cache snapshot root for this file, when it comes from a repository.
    fn hf_repo(&self) -> Option<&'static str> {
        match self.url_kind {
            UrlKind::HuggingFace { repo, .. } => Some(repo),
            UrlKind::Direct(_) => None,
        }
    }

    /// Archives unpack into the runtime folder instead of landing as one file.
    #[must_use]
    pub fn is_archive(&self) -> bool {
        self.path.ends_with(".zip")
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ModelEntry {
    pub id: &'static str,
    pub role: Role,
    pub label: &'static str,
    pub source_url: &'static str,
    pub files: &'static [FileSpec],
    /// Memory the model needs: GPU memory under CUDA, otherwise system memory.
    pub min_memory_gib: u64,
    pub diarize_kind: Option<DiarizeKind>,
    pub needs_cuda: bool,
    pub extra_args: &'static [&'static str],
}

const fn nemotron_file(path: &'static str, size: u64, sha256: &'static str) -> FileSpec {
    FileSpec { url_kind: UrlKind::HuggingFace { repo: NEMOTRON_REPO, revision: NEMOTRON_REVISION }, path, size, sha256 }
}

const fn nemotron(
    id: &'static str,
    label: &'static str,
    files: &'static [FileSpec],
    min_memory_gib: u64,
) -> ModelEntry {
    ModelEntry {
        id,
        role: Role::Diarize,
        label,
        source_url: NEMOTRON_SOURCE,
        files,
        min_memory_gib,
        diarize_kind: Some(DiarizeKind::SpeechLlm),
        needs_cuda: false,
        extra_args: &[],
    }
}

const NEMOTRON_IQ4_FILES: [FileSpec; 2] = [
    nemotron_file("NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-AD-IQ4_NL.gguf", 19_678_656_672, "0c47d4ab6c783e7606ec03714b354fab0b2052027a7ea1991e7e2d3009c345ad"),
    ENCODER,
];
const NEMOTRON_NVFP4_FILES: [FileSpec; 2] = [
    nemotron_file("NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-AD-NVFP4.gguf", 19_678_656_416, "ff1ae7655e66efe1d161a83e71c24853cea0614261b041b961089a23c7dcf2d3"),
    ENCODER,
];
const NEMOTRON_Q2M_FILES: [FileSpec; 2] = [
    nemotron_file("NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-AD-Q2_0_M.gguf", 13_572_208_800, "764dc3aedbc800a96346e188d74f56fc4d0932299e4771df1bc2e1c952ceed81"),
    ENCODER,
];
const NEMOTRON_Q2L_FILES: [FileSpec; 2] = [
    nemotron_file("NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-AD-Q2_0_L.gguf", 15_547_824_288, "67eee66a7bb205867e3f9b7c0944e287fc4d2661db2280030e1c26e6ebdea7b4"),
    ENCODER,
];
const NEMOTRON_Q8_FILES: [FileSpec; 2] = [
    nemotron_file("NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Q8_0.gguf", 35_004_643_232, "3d99075b58f15153d87e8922f54e12dda818bbb505c2e0d7d50cd4f30beac9f2"),
    ENCODER,
];
const WHISPER_FILES: [FileSpec; 1] = [FileSpec {
    url_kind: UrlKind::HuggingFace { repo: WHISPER_REPO, revision: WHISPER_REVISION },
    path: "model.safetensors",
    size: 1_617_824_864,
    sha256: "542566a422ae4f3fd23f1ba11add198fca01bbf82e66e6a2857b3f608b1eb9d1",
}];
const DIARIZATION_FILES: [FileSpec; 1] = [FileSpec {
    url_kind: UrlKind::HuggingFace { repo: DIARIZATION_REPO, revision: DIARIZATION_REVISION },
    path: "Nemotron-3-Diarization.q8_0.gguf",
    size: 107_012_128,
    sha256: "08456d9e22cd9a323c0364d98375f3746d6e68507ebb705cd46438c534c7a3a1",
}];
const GEFORCE_FILES: [FileSpec; 2] = [
    FileSpec {
        url_kind: UrlKind::Direct(NEMO_CUDA_URL),
        path: "nemo/nemo-speech-cuda.zip",
        size: 508_625_181,
        sha256: "d0d32ae1bf6ee0534767a47b2aa0e9e2ff6fe39643465c0baae9301d04f6ed42",
    },
    FileSpec {
        url_kind: UrlKind::Direct(WHISPER_CUDA_URL),
        path: "whisper/whisper-cuda.zip",
        size: 684_913_404,
        sha256: "afef0b881c500958921c3f5523b50e59ee2ec9b6f5cbd25b324c51ed308a957a",
    },
];

/// Every model and runtime the app knows.
pub const CATALOG: [ModelEntry; 10] = [
    ModelEntry {
        id: WHISPER_ID,
        role: Role::Transcribe,
        label: "Whisper large-v3-turbo",
        source_url: "https://huggingface.co/openai/whisper-large-v3-turbo",
        files: &WHISPER_FILES,
        min_memory_gib: 4,
        diarize_kind: None,
        needs_cuda: false,
        extra_args: &[],
    },
    nemotron(IQ4_ID, "Nemotron 3.5 Transcribe 30B-A4B (IQ4_NL)", &NEMOTRON_IQ4_FILES, 24),
    ModelEntry {
        needs_cuda: true,
        ..nemotron(NVFP4_ID, "Nemotron 3.5 Transcribe 30B-A4B (NVFP4)", &NEMOTRON_NVFP4_FILES, 24)
    },
    ModelEntry {
        needs_cuda: true,
        extra_args: &Q2M_ARGS,
        ..nemotron(Q2M_ID, "Nemotron 3.5 Transcribe 30B-A4B (Q2_0_M)", &NEMOTRON_Q2M_FILES, 16)
    },
    nemotron(Q2L_ID, "Nemotron 3.5 Transcribe 30B-A4B (Q2_0_L)", &NEMOTRON_Q2L_FILES, 20),
    nemotron(Q8_ID, "Nemotron 3.5 Transcribe 30B-A4B (Q8_0)", &NEMOTRON_Q8_FILES, 48),
    ModelEntry {
        id: NEMO_DIARIZATION_ID,
        role: Role::Diarize,
        label: "Nemotron 3 Diarization (Q8)",
        source_url: "https://huggingface.co/nvidia/Nemotron-3-Diarization",
        files: &DIARIZATION_FILES,
        min_memory_gib: 2,
        diarize_kind: Some(DiarizeKind::Sortformer),
        needs_cuda: false,
        extra_args: &[],
    },
    ModelEntry {
        id: YUNET_ID,
        role: Role::Face,
        label: "YuNet",
        source_url: "https://huggingface.co/opencv/face_detection_yunet",
        files: &[],
        min_memory_gib: 1,
        diarize_kind: None,
        needs_cuda: false,
        extra_args: &[],
    },
    ModelEntry {
        id: AVATAR_YOLO_ID,
        role: Role::Face,
        label: "Avatar face YOLO",
        source_url: "",
        files: &[],
        min_memory_gib: 1,
        diarize_kind: None,
        needs_cuda: false,
        extra_args: &[],
    },
    ModelEntry {
        id: GEFORCE_RUNTIME_ID,
        role: Role::Runtime,
        label: "GeForce CUDA runtime",
        source_url: "https://github.com/NVIDIA/NeMo-Speech.cpp/releases/tag/v0.3.0",
        files: &GEFORCE_FILES,
        min_memory_gib: 0,
        diarize_kind: None,
        needs_cuda: false,
        extra_args: &[],
    },
];

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct Hardware {
    pub is_mac: bool,
    pub memory_gib: u64,
    pub nvidia_name: Option<String>,
    pub nvidia_memory_gib: u64,
}

/// One row of the model manager.
#[derive(Clone, Debug, Serialize)]
pub struct ModelStatus {
    pub id: &'static str,
    pub role: Role,
    pub label: &'static str,
    pub source_url: &'static str,
    pub download_bytes: u64,
    /// Where the usable copy is, if any.
    pub found_at: Option<PathBuf>,
    /// The hardware's reason this model cannot run, if any.
    pub blocked: Option<UserError>,
    pub recommended: bool,
    pub hidden: bool,
}

/// Parses `nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits`.
#[must_use]
pub fn parse_nvidia_smi(output: &str) -> Option<(String, u64)> {
    let line = output.lines().next()?;
    let (name, mebibytes) = line.rsplit_once(',')?;
    let mebibytes: u64 = mebibytes.trim().parse().ok()?;
    Some((name.trim().to_owned(), mebibytes / 1024))
}

fn query_nvidia() -> Option<(String, u64)> {
    let mut command = background_command(Path::new("nvidia-smi"));
    command.args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"]);
    let output = run_capture(command, "nvidia-smi").ok()?;
    parse_nvidia_smi(&String::from_utf8_lossy(&output))
}

fn parse_memory_bytes(output: &[u8]) -> AppResult<u64> {
    let text = String::from_utf8_lossy(output);
    text.trim().parse().map_err(|_| AppError::Invalid(format!("unexpected memory size: {text}")))
}

fn query_memory_bytes() -> AppResult<u64> {
    let mut command = background_command(Path::new(MEMORY_QUERY.0));
    command.args(MEMORY_QUERY.1);
    parse_memory_bytes(&run_capture(command, "memory query")?)
}

/// Queries this machine; a missing `nvidia-smi` simply means no GeForce.
pub fn detect_hardware() -> AppResult<Hardware> {
    let nvidia = query_nvidia();
    Ok(Hardware {
        is_mac: cfg!(target_os = "macos"),
        memory_gib: query_memory_bytes()?.div_ceil(GIB),
        nvidia_memory_gib: nvidia.as_ref().map_or(0, |gpu| gpu.1),
        nvidia_name: nvidia.map(|gpu| gpu.0),
    })
}

#[must_use]
pub fn find_entry(id: &str) -> Option<&'static ModelEntry> {
    CATALOG.iter().find(|entry| entry.id == id)
}

fn is_blackwell(name: &str) -> bool {
    BLACKWELL_MARKERS.iter().any(|marker| name.contains(marker))
}

/// The Nemotron 3.5 build the model card lists for this hardware; `None` means none will fit.
#[must_use]
pub fn recommended_speech_llm(hardware: &Hardware) -> Option<&'static str> {
    if let Some(name) = &hardware.nvidia_name {
        return match hardware.nvidia_memory_gib {
            24.. if is_blackwell(name) => Some(NVFP4_ID),
            24.. => Some(IQ4_ID),
            16.. => Some(Q2M_ID),
            _ => None,
        };
    }
    match hardware.memory_gib {
        64.. => Some(Q8_ID),
        32.. => Some(IQ4_ID),
        24.. => Some(Q2L_ID),
        _ => None,
    }
}

/// Why this hardware cannot run the model, if it cannot.
#[must_use]
pub fn blocked_reason(entry: &ModelEntry, hardware: &Hardware) -> Option<UserError> {
    if entry.needs_cuda && hardware.nvidia_name.is_none() {
        return Some(UserError::NeedsNvidia { model: entry.id.to_owned() });
    }
    let available = if hardware.nvidia_name.is_some() && entry.role == Role::Diarize {
        hardware.nvidia_memory_gib.max(hardware.memory_gib)
    } else {
        hardware.memory_gib
    };
    (available < entry.min_memory_gib).then(|| UserError::NeedsMemory {
        model: entry.id.to_owned(),
        required: entry.min_memory_gib,
        available,
    })
}

fn is_hidden(entry: &ModelEntry, hardware: &Hardware) -> bool {
    entry.id == GEFORCE_RUNTIME_ID && hardware.nvidia_name.is_none()
}

fn find_named(root: &Path, names: &[&str], depth: usize) -> Option<PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    let mut subfolders = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let path = entry.path();
        if names.iter().any(|wanted| name.to_string_lossy() == *wanted) && path.is_file() {
            return Some(path);
        }
        let hidden = name.to_string_lossy().starts_with('.');
        if depth > 0 && !hidden && path.is_dir() {
            subfolders.push(path);
        }
    }
    subfolders.iter().find_map(|folder| find_named(folder, names, depth - 1))
}

fn hf_snapshots(repo: &str) -> Vec<PathBuf> {
    let Ok(hub) = huggingface_hub_dir() else { return Vec::new() };
    let folder = hub.join(format!("models--{}", repo.replace('/', "--"))).join("snapshots");
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    entries.flatten().map(|entry| entry.path()).collect()
}

/// Finds a complete copy of one catalogued file: right name, right size. Hugging Face cache included.
fn locate_file(file: &FileSpec, roots: &[PathBuf]) -> Option<PathBuf> {
    let name = Path::new(file.path).file_name()?.to_str()?;
    let matches_size = |path: &PathBuf| std::fs::metadata(path).is_ok_and(|meta| meta.len() == file.size);
    let from_roots = roots.iter().filter_map(|root| find_named(root, &[name], SEARCH_DEPTH));
    let from_cache = file.hf_repo().into_iter().flat_map(hf_snapshots).map(|snapshot| snapshot.join(name));
    from_roots.chain(from_cache).find(matches_size)
}

fn search_roots(paths: &AppPaths, extra: &[PathBuf]) -> Vec<PathBuf> {
    std::iter::once(paths.models_dir()).chain(extra.iter().cloned()).collect()
}

/// A checkpoint that can become the ggml file, from a download or an earlier MLX install.
fn locate_whisper_checkpoint(roots: &[PathBuf]) -> Option<PathBuf> {
    if let Some(path) = locate_file(&WHISPER_FILES[0], roots) {
        return Some(path);
    }
    roots.iter().find_map(|root| {
        let path = find_named(root, &[WHISPER_MLX_NAME], SEARCH_DEPTH)?;
        std::fs::metadata(&path).ok().filter(|meta| meta.len() == WHISPER_MLX_SIZE).map(|_| path)
    })
}

/// The ggml file ready for whisper.cpp, if one exists.
#[must_use]
pub fn locate_whisper_ggml(paths: &AppPaths, extra: &[PathBuf]) -> Option<PathBuf> {
    search_roots(paths, extra).iter().find_map(|root| find_named(root, &WHISPER_GGML_ALIASES, SEARCH_DEPTH))
}

/// Source weights to convert when no ggml file exists yet.
#[must_use]
pub fn locate_whisper_source(paths: &AppPaths, extra: &[PathBuf]) -> Option<PathBuf> {
    locate_whisper_checkpoint(&search_roots(paths, extra))
}

fn locate_runtime(paths: &AppPaths) -> Option<PathBuf> {
    let folder = paths.downloaded_runtime_dir().join("geforce");
    folder.join("nemo").is_dir().then_some(folder.clone()).filter(|_| folder.join("whisper").is_dir())
}

/// Where a usable copy of the entry is: its main file, or the folder for runtimes.
#[must_use]
pub fn locate(entry: &ModelEntry, paths: &AppPaths, extra: &[PathBuf]) -> Option<PathBuf> {
    let roots = search_roots(paths, extra);
    match entry.id {
        WHISPER_ID => locate_whisper_ggml(paths, extra).or_else(|| locate_whisper_source(paths, extra)),
        YUNET_ID => Some(paths.yunet_model()).filter(|path| path.is_file()),
        AVATAR_YOLO_ID => roots.iter().find_map(|root| find_named(root, &[AVATAR_YOLO_NAME], SEARCH_DEPTH)),
        GEFORCE_RUNTIME_ID => locate_runtime(paths),
        _ => {
            let located: Option<Vec<PathBuf>> = entry.files.iter().map(|file| locate_file(file, &roots)).collect();
            located?.into_iter().next()
        }
    }
}

/// The encoder that must sit beside a speech-LLM decoder; decoders load it by file name.
#[must_use]
pub fn speech_llm_encoder(extra_roots: &[PathBuf], paths: &AppPaths) -> Option<PathBuf> {
    locate_file(&ENCODER, &search_roots(paths, extra_roots))
}

/// The state of every catalog entry for the model manager.
#[must_use]
pub fn statuses(hardware: &Hardware, paths: &AppPaths, extra: &[PathBuf]) -> Vec<ModelStatus> {
    let recommended = recommended_speech_llm(hardware);
    CATALOG
        .iter()
        .map(|entry| ModelStatus {
            id: entry.id,
            role: entry.role,
            label: entry.label,
            source_url: entry.source_url,
            download_bytes: entry.files.iter().map(|file| file.size).sum(),
            found_at: locate(entry, paths, extra),
            blocked: blocked_reason(entry, hardware),
            recommended: recommended == Some(entry.id) || entry.id == WHISPER_ID || entry.id == YUNET_ID,
            hidden: is_hidden(entry, hardware),
        })
        .collect()
}

/// The diarization model to use when none is chosen: an installed one first, so nothing downloads silently.
#[must_use]
pub fn default_diarize_model(hardware: &Hardware, rows: &[ModelStatus]) -> Option<&'static str> {
    let usable = |row: &&ModelStatus| row.role == Role::Diarize && row.found_at.is_some() && row.blocked.is_none();
    let recommended = recommended_speech_llm(hardware);
    rows.iter()
        .filter(usable)
        .find(|row| Some(row.id) == recommended)
        .or_else(|| rows.iter().find(usable))
        .map(|row| row.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mac(memory_gib: u64) -> Hardware {
        Hardware { is_mac: true, memory_gib, nvidia_name: None, nvidia_memory_gib: 0 }
    }

    fn geforce(name: &str, vram: u64) -> Hardware {
        Hardware { is_mac: false, memory_gib: 64, nvidia_name: Some(name.to_owned()), nvidia_memory_gib: vram }
    }

    #[test]
    fn urls_pin_the_revision() {
        assert_eq!(
            WHISPER_FILES[0].url(),
            "https://huggingface.co/openai/whisper-large-v3-turbo/resolve/41f01f3fe87f28c78e2fbf8b568835947dd65ed9/model.safetensors"
        );
    }

    #[test]
    fn nvidia_smi_output_parses() {
        assert_eq!(parse_nvidia_smi("NVIDIA GeForce RTX 5090, 32760\n"), Some(("NVIDIA GeForce RTX 5090".to_owned(), 31)));
        assert_eq!(parse_nvidia_smi(""), None);
        assert_eq!(parse_nvidia_smi("no comma"), None);
    }

    #[test]
    fn macs_get_the_build_their_memory_fits() {
        assert_eq!(recommended_speech_llm(&mac(16)), None);
        assert_eq!(recommended_speech_llm(&mac(24)), Some("nemotron-3.5-transcribe-q2l"));
        assert_eq!(recommended_speech_llm(&mac(48)), Some("nemotron-3.5-transcribe-iq4"));
        assert_eq!(recommended_speech_llm(&mac(64)), Some("nemotron-3.5-transcribe-q8"));
    }

    #[test]
    fn geforce_gets_its_own_builds() {
        assert_eq!(recommended_speech_llm(&geforce("NVIDIA GeForce RTX 5090", 32)), Some("nemotron-3.5-transcribe-nvfp4"));
        assert_eq!(recommended_speech_llm(&geforce("NVIDIA GeForce RTX 4090", 24)), Some("nemotron-3.5-transcribe-iq4"));
        assert_eq!(recommended_speech_llm(&geforce("NVIDIA GeForce RTX 4080", 16)), Some("nemotron-3.5-transcribe-q2m"));
        assert_eq!(recommended_speech_llm(&geforce("NVIDIA GeForce RTX 3060", 12)), None);
    }

    #[test]
    fn a_12gb_vm_cannot_run_the_30b_model_but_can_run_the_small_one() {
        let vm = Hardware { is_mac: false, memory_gib: 12, nvidia_name: None, nvidia_memory_gib: 0 };
        let large = find_entry("nemotron-3.5-transcribe-iq4").unwrap();
        let small = find_entry("nemotron-3-diarization").unwrap();
        assert_eq!(
            blocked_reason(large, &vm).unwrap(),
            UserError::NeedsMemory { model: "nemotron-3.5-transcribe-iq4".to_owned(), required: 24, available: 12 }
        );
        assert_eq!(blocked_reason(small, &vm), None);
    }

    #[test]
    fn cuda_builds_need_a_nvidia_gpu() {
        let entry = find_entry("nemotron-3.5-transcribe-nvfp4").unwrap();
        assert_eq!(blocked_reason(entry, &mac(128)).unwrap(), UserError::NeedsNvidia { model: "nemotron-3.5-transcribe-nvfp4".to_owned() });
    }

    #[test]
    fn the_geforce_runtime_shows_only_with_a_geforce() {
        let entry = find_entry("geforce-runtime").unwrap();
        assert!(is_hidden(entry, &mac(48)));
        assert!(!is_hidden(entry, &geforce("NVIDIA GeForce RTX 4090", 24)));
    }

    #[test]
    fn every_downloadable_hash_is_a_sha256() {
        let bad: Vec<&str> = CATALOG
            .iter()
            .flat_map(|entry| entry.files.iter())
            .filter(|file| file.sha256.len() != 64 || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(|file| file.path)
            .collect();
        assert_eq!(bad, Vec::<&str>::new());
    }
}
