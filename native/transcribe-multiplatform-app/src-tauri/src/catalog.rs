//! Downloadable models and runtimes, per hardware, and detection of copies already on disk.

use crate::error::{AppError, AppResult, UserError};
use crate::paths::AppPaths;
use crate::process::{background_command, run_capture};
use serde::Serialize;
use std::path::{Path, PathBuf};

const HF_RESOLVE: &str = "https://huggingface.co";
const GIB: u64 = 1 << 30;
/// Below this an adapter is an integrated GPU sharing system memory: slower than the CPU path for speech.
const MIN_GPU_MEMORY_GIB: u64 = 4;
/// The speech-LLM takes a recording of up to an hour; Q2_0_M's `max_context` holds about 7 minutes.
const SPEECH_LLM_MAX_SECONDS: u64 = 3600;
const Q2M_MAX_SECONDS: u64 = 420;
const SECONDS_PER_MINUTE: u64 = 60;
#[cfg(all(feature = "windows-geforce", feature = "windows-radeon"))]
compile_error!("a build is either windows-geforce or windows-radeon, not both");
/// The GPU vendor this distribution was built for; other vendors' cards are never used.
const BUILD_GPU_VENDOR: Option<GpuVendor> = build_gpu_vendor(
    cfg!(feature = "windows-geforce"),
    cfg!(feature = "windows-radeon"),
);
const GPU_QUERY: Option<ToolQuery> = if cfg!(windows) {
    Some((
        "powershell",
        &[
            "-NoProfile",
            "-Command",
            r"Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\0*' -EA SilentlyContinue | ForEach-Object { $_.DriverDesc + '|' + $_.'HardwareInformation.qwMemorySize' }",
        ],
    ))
} else {
    None
};
const MEMORY_QUERY: ToolQuery = if cfg!(windows) {
    (
        "powershell",
        &[
            "-NoProfile",
            "-Command",
            "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory",
        ],
    )
} else {
    ("/usr/sbin/sysctl", &["-n", "hw.memsize"])
};
const SEARCH_DEPTH: usize = 4;
const WHISPER_REPO: &str = "openai/whisper-large-v3-turbo";
const WHISPER_REVISION: &str = "41f01f3fe87f28c78e2fbf8b568835947dd65ed9";
const NEMOTRON_REPO: &str = "AtomicChat/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-GGUF";
const NEMOTRON_REVISION: &str = "dbd06c5cad7a2f33c8e06be2fb3ef814dbd9aa59";
const NEMOTRON_SOURCE: &str =
    "https://huggingface.co/nvidia/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B";
const ENCODER: FileSpec = FileSpec {
    url_kind: UrlKind::HuggingFace {
        repo: NEMOTRON_REPO,
        revision: NEMOTRON_REVISION,
    },
    path: "NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Encoder-F16.gguf",
    size: 1_670_410_080,
    sha256: "4221029c8a565c4a80c507ffa760c716e595acf663f9f4273ce69910050e7ba2",
};
/// The converted ggml model whisper.cpp reads; made once from the checkpoint.
pub const WHISPER_GGML_NAME: &str = "ggml-large-v3-turbo.bin";
/// Ready ggml files from other tools are used as they are.
const WHISPER_GGML_ALIASES: [&str; 3] = [
    WHISPER_GGML_NAME,
    "ggml-large-v3-turbo-q8_0.bin",
    "ggml-large-v3-turbo-q5_0.bin",
];
/// The MLX checkpoint this repository's Python pipeline already downloads.
const WHISPER_MLX_NAME: &str = "weights.safetensors";
const WHISPER_MLX_SIZE: u64 = 1_613_977_612;
pub const WHISPER_ID: &str = "whisper-large-v3-turbo";
pub const YUNET_ID: &str = "yunet";
pub const AVATAR_YOLO_ID: &str = "avatar-face-yolo";
const AVATAR_YOLO_NAME: &str = "avatar-face-caption-v2.onnx";
pub const GEFORCE_RUNTIME_ID: &str = "geforce-runtime";
const IQ4_ID: &str = "nemotron-3.5-transcribe-iq4";
const NVFP4_ID: &str = "nemotron-3.5-transcribe-nvfp4";
const Q2M_ID: &str = "nemotron-3.5-transcribe-q2m";
const Q2L_ID: &str = "nemotron-3.5-transcribe-q2l";
const Q8_ID: &str = "nemotron-3.5-transcribe-q8";
const NEMO_CUDA_URL: &str =
    "https://github.com/NVIDIA/NeMo-Speech.cpp/releases/download/v0.3.0/nemo-speech-0.3.0-windows-x86_64-cuda.zip";
/// Settings for 16 GB GPUs, documented by the GGUF publisher; `max_context` holds about 7 minutes.
const Q2M_ARGS: [&str; 12] = [
    "--asr.salm.max_sequences",
    "2",
    "--asr.salm.n_ctx_per_sequence",
    "8192",
    "--asr.salm.max_context",
    "8192",
    "--asr.salm.mtp_draft",
    "0",
    "--asr.salm.n_batch",
    "512",
    "--asr.salm.encoder_max_batch",
    "2",
];
/// Blackwell GPUs are the ones with native NVFP4 kernels.
const BLACKWELL_MARKERS: [&str; 3] = ["RTX 50", "RTX PRO 6000 Blackwell", "DGX Spark"];

type ToolQuery = (&'static str, &'static [&'static str]);

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Transcribe,
    Diarize,
    Separate,
    Face,
    Runtime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UrlKind {
    HuggingFace {
        repo: &'static str,
        revision: &'static str,
    },
    Direct(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileSpec {
    pub(crate) url_kind: UrlKind,
    pub path: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

impl FileSpec {
    #[must_use]
    pub fn url(&self) -> String {
        match self.url_kind {
            UrlKind::HuggingFace { repo, revision } => {
                format!("{HF_RESOLVE}/{repo}/resolve/{revision}/{}", self.path)
            }
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
    pub needs_cuda: bool,
    pub extra_args: &'static [&'static str],
    /// The longest recording the model takes, if it has a limit.
    pub max_audio_seconds: Option<u64>,
}

// Everything about where MossFormer2 comes from: a new build replaces these and nothing else.
pub const MOSSFORMER2_ID: &str = "mossformer2-ss-16k";
const MOSSFORMER2_SOURCE: &str = "https://huggingface.co/kkk4oru/mossformer2-ss-16k-onnx";
/// Measured with ONNX Runtime on the CPU: 2.35 GB for an 8 s clip, 7.25 GB for the longest range, 30 s.
const MOSSFORMER2_MEMORY_GIB: u64 = 8;
const MOSSFORMER2_FILES: [FileSpec; 1] = [FileSpec {
    url_kind: UrlKind::HuggingFace {
        repo: "kkk4oru/mossformer2-ss-16k-onnx",
        revision: "fb27e5ff961cff11372473f9808ca00d83144f69",
    },
    path: "model.onnx",
    size: 230_156_008,
    sha256: "d658037076e27c76b347f98b0b18eb0f51ef5df9da09a359dbb058fec52db2de",
}];

const fn nemotron_file(path: &'static str, size: u64, sha256: &'static str) -> FileSpec {
    FileSpec {
        url_kind: UrlKind::HuggingFace {
            repo: NEMOTRON_REPO,
            revision: NEMOTRON_REVISION,
        },
        path,
        size,
        sha256,
    }
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
        needs_cuda: false,
        extra_args: &[],
        max_audio_seconds: Some(SPEECH_LLM_MAX_SECONDS),
    }
}

const NEMOTRON_IQ4_FILES: [FileSpec; 2] = [
    nemotron_file(
        "NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-AD-IQ4_NL.gguf",
        19_678_656_672,
        "0c47d4ab6c783e7606ec03714b354fab0b2052027a7ea1991e7e2d3009c345ad",
    ),
    ENCODER,
];
const NEMOTRON_NVFP4_FILES: [FileSpec; 2] = [
    nemotron_file(
        "NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-AD-NVFP4.gguf",
        19_678_656_416,
        "ff1ae7655e66efe1d161a83e71c24853cea0614261b041b961089a23c7dcf2d3",
    ),
    ENCODER,
];
const NEMOTRON_Q2M_FILES: [FileSpec; 2] = [
    nemotron_file(
        "NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-AD-Q2_0_M.gguf",
        13_572_208_800,
        "764dc3aedbc800a96346e188d74f56fc4d0932299e4771df1bc2e1c952ceed81",
    ),
    ENCODER,
];
const NEMOTRON_Q2L_FILES: [FileSpec; 2] = [
    nemotron_file(
        "NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-AD-Q2_0_L.gguf",
        15_547_824_288,
        "67eee66a7bb205867e3f9b7c0944e287fc4d2661db2280030e1c26e6ebdea7b4",
    ),
    ENCODER,
];
const NEMOTRON_Q8_FILES: [FileSpec; 2] = [
    nemotron_file(
        "NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Q8_0.gguf",
        35_004_643_232,
        "3d99075b58f15153d87e8922f54e12dda818bbb505c2e0d7d50cd4f30beac9f2",
    ),
    ENCODER,
];
const WHISPER_FILES: [FileSpec; 1] = [FileSpec {
    url_kind: UrlKind::HuggingFace {
        repo: WHISPER_REPO,
        revision: WHISPER_REVISION,
    },
    path: "model.safetensors",
    size: 1_617_824_864,
    sha256: "542566a422ae4f3fd23f1ba11add198fca01bbf82e66e6a2857b3f608b1eb9d1",
}];
const GEFORCE_FILES: [FileSpec; 1] = [FileSpec {
    url_kind: UrlKind::Direct(NEMO_CUDA_URL),
    path: "nemo/nemo-speech-cuda.zip",
    size: 508_625_181,
    sha256: "d0d32ae1bf6ee0534767a47b2aa0e9e2ff6fe39643465c0baae9301d04f6ed42",
}];

/// Every model and runtime the app knows.
pub const CATALOG: [ModelEntry; 10] = [
    ModelEntry {
        id: WHISPER_ID,
        role: Role::Transcribe,
        label: "Whisper large-v3-turbo",
        source_url: "https://huggingface.co/openai/whisper-large-v3-turbo",
        files: &WHISPER_FILES,
        min_memory_gib: 4,
        needs_cuda: false,
        extra_args: &[],
        max_audio_seconds: None,
    },
    nemotron(
        IQ4_ID,
        "Nemotron 3.5 Transcribe 30B-A4B (IQ4_NL)",
        &NEMOTRON_IQ4_FILES,
        24,
    ),
    ModelEntry {
        needs_cuda: true,
        ..nemotron(
            NVFP4_ID,
            "Nemotron 3.5 Transcribe 30B-A4B (NVFP4)",
            &NEMOTRON_NVFP4_FILES,
            24,
        )
    },
    ModelEntry {
        needs_cuda: true,
        extra_args: &Q2M_ARGS,
        max_audio_seconds: Some(Q2M_MAX_SECONDS),
        ..nemotron(
            Q2M_ID,
            "Nemotron 3.5 Transcribe 30B-A4B (Q2_0_M)",
            &NEMOTRON_Q2M_FILES,
            16,
        )
    },
    nemotron(
        Q2L_ID,
        "Nemotron 3.5 Transcribe 30B-A4B (Q2_0_L)",
        &NEMOTRON_Q2L_FILES,
        20,
    ),
    nemotron(
        Q8_ID,
        "Nemotron 3.5 Transcribe 30B-A4B (Q8_0)",
        &NEMOTRON_Q8_FILES,
        48,
    ),
    ModelEntry {
        id: MOSSFORMER2_ID,
        role: Role::Separate,
        label: "MossFormer2 SS 16K",
        source_url: MOSSFORMER2_SOURCE,
        files: &MOSSFORMER2_FILES,
        min_memory_gib: MOSSFORMER2_MEMORY_GIB,
        needs_cuda: false,
        extra_args: &[],
        max_audio_seconds: None,
    },
    ModelEntry {
        id: YUNET_ID,
        role: Role::Face,
        label: "YuNet",
        source_url: "https://huggingface.co/opencv/face_detection_yunet",
        files: &[],
        min_memory_gib: 1,
        needs_cuda: false,
        extra_args: &[],
        max_audio_seconds: None,
    },
    ModelEntry {
        id: AVATAR_YOLO_ID,
        role: Role::Face,
        label: "Avatar face YOLO",
        source_url: "",
        files: &[],
        min_memory_gib: 1,
        needs_cuda: false,
        extra_args: &[],
        max_audio_seconds: None,
    },
    ModelEntry {
        id: GEFORCE_RUNTIME_ID,
        role: Role::Runtime,
        label: "GeForce CUDA runtime",
        source_url: "https://github.com/NVIDIA/NeMo-Speech.cpp/releases/tag/v0.3.0",
        files: &GEFORCE_FILES,
        min_memory_gib: 0,
        needs_cuda: false,
        extra_args: &[],
        max_audio_seconds: None,
    },
];

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GpuVendor {
    Nvidia,
    Radeon,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Gpu {
    pub vendor: GpuVendor,
    pub name: String,
    pub memory_gib: u64,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct Hardware {
    pub is_mac: bool,
    pub memory_gib: u64,
    /// A discrete Windows GPU the speech tools can use; Macs use Metal without this.
    pub gpu: Option<Gpu>,
}

/// Which builds of the speech tools run. Only this distribution's vendor is ever considered.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// Metal on a Mac, the CPU elsewhere.
    Default,
    /// Radeon distribution: both tools on Vulkan.
    Vulkan,
    /// GeForce distribution with its runtime: speakers on CUDA, transcription on Vulkan.
    Cuda,
    /// GeForce distribution without its runtime: transcription on Vulkan, speakers on the CPU.
    SpeakersOnCpu,
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
}

const fn build_gpu_vendor(geforce: bool, radeon: bool) -> Option<GpuVendor> {
    match (geforce, radeon) {
        (true, _) => Some(GpuVendor::Nvidia),
        (_, true) => Some(GpuVendor::Radeon),
        _ => None,
    }
}

/// Parses `DriverDesc|bytes` lines from the display adapters' registry keys. The registry, unlike
/// `Win32_VideoController.AdapterRAM`, reports more than 4 GB. The largest GPU of `vendor` wins.
#[must_use]
pub fn parse_display_adapters(output: &str, vendor: GpuVendor) -> Option<Gpu> {
    output
        .lines()
        .filter_map(|line| parse_display_adapter(line, vendor))
        .max_by_key(|gpu| gpu.memory_gib)
}

fn parse_display_adapter(line: &str, vendor: GpuVendor) -> Option<Gpu> {
    let (name, bytes) = line.trim().rsplit_once('|')?;
    if !is_from_vendor(name, vendor) {
        return None;
    }
    let memory_gib = bytes.trim().parse::<u64>().ok()? / GIB;
    (memory_gib >= MIN_GPU_MEMORY_GIB).then(|| Gpu {
        vendor,
        name: name.trim().to_owned(),
        memory_gib,
    })
}

fn is_from_vendor(name: &str, vendor: GpuVendor) -> bool {
    let upper = name.to_uppercase();
    match vendor {
        GpuVendor::Nvidia => upper.contains("NVIDIA") || upper.contains("GEFORCE"),
        GpuVendor::Radeon => upper.contains("RADEON"),
    }
}

fn query_gpu(query: Option<ToolQuery>, vendor: Option<GpuVendor>) -> Option<Gpu> {
    let (vendor, (program, args)) = (vendor?, query?);
    let mut command = background_command(Path::new(program));
    command.args(args);
    let output = run_capture(command, "display adapter query").ok()?;
    parse_display_adapters(&String::from_utf8_lossy(&output), vendor)
}

fn parse_memory_bytes(output: &[u8]) -> AppResult<u64> {
    let text = String::from_utf8_lossy(output);
    text.trim()
        .parse()
        .map_err(|_| AppError::Invalid(format!("unexpected memory size: {text}")))
}

fn query_memory_bytes() -> AppResult<u64> {
    let mut command = background_command(Path::new(MEMORY_QUERY.0));
    command.args(MEMORY_QUERY.1);
    parse_memory_bytes(&run_capture(command, "memory query")?)
}

/// Queries this machine. Macs and PCs without a discrete GPU simply have no `gpu`.
pub fn detect_hardware() -> AppResult<Hardware> {
    Ok(Hardware {
        is_mac: cfg!(target_os = "macos"),
        memory_gib: query_memory_bytes()?.div_ceil(GIB),
        gpu: query_gpu(GPU_QUERY, BUILD_GPU_VENDOR),
    })
}

#[must_use]
pub fn find_entry(id: &str) -> Option<&'static ModelEntry> {
    CATALOG.iter().find(|entry| entry.id == id)
}

fn is_blackwell(name: &str) -> bool {
    BLACKWELL_MARKERS.iter().any(|marker| name.contains(marker))
}

const fn recommended_for_nvidia(memory_gib: u64, blackwell: bool) -> Option<&'static str> {
    match memory_gib {
        24.. if blackwell => Some(NVFP4_ID),
        24.. => Some(IQ4_ID),
        16.. => Some(Q2M_ID),
        _ => None,
    }
}

/// Vulkan runs upstream llama.cpp kernels, which the model's documentation pairs with IQ4_NL and Q8_0.
const fn recommended_for_radeon(memory_gib: u64) -> Option<&'static str> {
    match memory_gib {
        48.. => Some(Q8_ID),
        24.. => Some(IQ4_ID),
        _ => None,
    }
}

/// The Nemotron 3.5 build the model card lists for this hardware; `None` means none will fit.
#[must_use]
pub fn recommended_speech_llm(hardware: &Hardware) -> Option<&'static str> {
    if let Some(gpu) = &hardware.gpu {
        return match gpu.vendor {
            GpuVendor::Nvidia => recommended_for_nvidia(gpu.memory_gib, is_blackwell(&gpu.name)),
            GpuVendor::Radeon => recommended_for_radeon(gpu.memory_gib),
        };
    }
    match hardware.memory_gib {
        64.. => Some(Q8_ID),
        32.. => Some(IQ4_ID),
        24.. => Some(Q2L_ID),
        _ => None,
    }
}

const fn has_nvidia(hardware: &Hardware) -> bool {
    matches!(
        &hardware.gpu,
        Some(Gpu {
            vendor: GpuVendor::Nvidia,
            ..
        })
    )
}

/// What this machine runs on: `gpu` only ever holds this distribution's vendor.
#[must_use]
pub fn backend_for(hardware: &Hardware, paths: &AppPaths) -> Backend {
    match &hardware.gpu {
        Some(Gpu {
            vendor: GpuVendor::Nvidia,
            ..
        }) if locate_runtime(paths).is_some() => Backend::Cuda,
        Some(Gpu {
            vendor: GpuVendor::Nvidia,
            ..
        }) => Backend::SpeakersOnCpu,
        Some(Gpu {
            vendor: GpuVendor::Radeon,
            ..
        }) => Backend::Vulkan,
        None => Backend::Default,
    }
}

/// The `--device` for a model: the GPU when the model fits in its memory, else system memory on the CPU.
#[must_use]
pub fn device_for(entry: &ModelEntry, hardware: &Hardware, backend: Backend) -> &'static str {
    let fits_gpu = hardware
        .gpu
        .as_ref()
        .is_some_and(|gpu| gpu.memory_gib >= entry.min_memory_gib);
    match backend {
        Backend::Default => "auto",
        Backend::SpeakersOnCpu => "cpu",
        Backend::Cuda | Backend::Vulkan if !fits_gpu => "cpu",
        Backend::Cuda => "cuda",
        Backend::Vulkan => "vulkan",
    }
}

/// `TooLongForModel` when the video exceeds what the model takes.
#[must_use]
pub fn too_long(entry: &ModelEntry, duration_seconds: f64) -> Option<UserError> {
    let limit = entry.max_audio_seconds?;
    (duration_seconds > limit as f64).then(|| UserError::TooLongForModel {
        model: entry.id.to_owned(),
        limit_minutes: limit / SECONDS_PER_MINUTE,
    })
}

/// Why this hardware cannot run the model, if it cannot.
#[must_use]
pub fn blocked_reason(entry: &ModelEntry, hardware: &Hardware) -> Option<UserError> {
    if entry.needs_cuda && !has_nvidia(hardware) {
        return Some(UserError::NeedsNvidia {
            model: entry.id.to_owned(),
        });
    }
    // A model too big for the GPU still runs from system memory on the CPU (see `device_for`).
    let gpu_memory = hardware.gpu.as_ref().map_or(0, |gpu| gpu.memory_gib);
    let available = if entry.role == Role::Diarize {
        gpu_memory.max(hardware.memory_gib)
    } else {
        hardware.memory_gib
    };
    (available < entry.min_memory_gib).then(|| UserError::NeedsMemory {
        model: entry.id.to_owned(),
        required: entry.min_memory_gib,
        available,
    })
}

/// The runtime exists only for a GeForce; every other machine never sees it.
fn is_listed(entry: &ModelEntry, hardware: &Hardware) -> bool {
    entry.id != GEFORCE_RUNTIME_ID || has_nvidia(hardware)
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
    subfolders
        .iter()
        .find_map(|folder| find_named(folder, names, depth - 1))
}

fn hf_snapshots(hub: Option<&Path>, repo: &str) -> Vec<PathBuf> {
    let Some(hub) = hub else { return Vec::new() };
    let folder = hub
        .join(format!("models--{}", repo.replace('/', "--")))
        .join("snapshots");
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    entries.flatten().map(|entry| entry.path()).collect()
}

/// Finds a complete copy of one catalogued file: right name, right size. Hugging Face cache included.
pub fn locate_file(file: &FileSpec, roots: &[PathBuf], hub: Option<&Path>) -> Option<PathBuf> {
    let name = Path::new(file.path).file_name()?.to_str()?;
    let matches_size =
        |path: &PathBuf| std::fs::metadata(path).is_ok_and(|meta| meta.len() == file.size);
    let from_roots = roots
        .iter()
        .filter_map(|root| find_named(root, &[name], SEARCH_DEPTH));
    let from_cache = file
        .hf_repo()
        .into_iter()
        .flat_map(|repo| hf_snapshots(hub, repo))
        .map(|snapshot| snapshot.join(name));
    from_roots.chain(from_cache).find(matches_size)
}

fn search_roots(paths: &AppPaths, extra: &[PathBuf]) -> Vec<PathBuf> {
    std::iter::once(paths.models_dir())
        .chain(extra.iter().cloned())
        .collect()
}

/// A checkpoint that can become the ggml file, from a download or an earlier MLX install.
fn locate_whisper_checkpoint(roots: &[PathBuf], hub: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = locate_file(&WHISPER_FILES[0], roots, hub) {
        return Some(path);
    }
    roots.iter().find_map(|root| {
        let path = find_named(root, &[WHISPER_MLX_NAME], SEARCH_DEPTH)?;
        std::fs::metadata(&path)
            .ok()
            .filter(|meta| meta.len() == WHISPER_MLX_SIZE)
            .map(|_| path)
    })
}

/// The ggml file ready for whisper.cpp, if one exists.
#[must_use]
pub fn locate_whisper_ggml(paths: &AppPaths, extra: &[PathBuf]) -> Option<PathBuf> {
    search_roots(paths, extra)
        .iter()
        .find_map(|root| find_named(root, &WHISPER_GGML_ALIASES, SEARCH_DEPTH))
}

/// Source weights to convert when no ggml file exists yet.
#[must_use]
pub fn locate_whisper_source(paths: &AppPaths, extra: &[PathBuf]) -> Option<PathBuf> {
    locate_whisper_checkpoint(&search_roots(paths, extra), paths.hf_hub.as_deref())
}

/// Written beside an archive only after all of it is unpacked.
pub(crate) fn unpacked_marker(archive: &Path) -> PathBuf {
    let mut name = archive.as_os_str().to_owned();
    name.push(".unpacked");
    PathBuf::from(name)
}

/// A running, stopped or half-unpacked download has no marker, so it does not count as installed.
fn locate_runtime(paths: &AppPaths) -> Option<PathBuf> {
    let folder = paths.downloaded_runtime_dir().join("geforce");
    unpacked_marker(&folder.join(GEFORCE_FILES[0].path))
        .is_file()
        .then_some(folder)
}

/// Where a usable copy of the entry is: its main file, or the folder for runtimes.
#[must_use]
pub fn locate(entry: &ModelEntry, paths: &AppPaths, extra: &[PathBuf]) -> Option<PathBuf> {
    let roots = search_roots(paths, extra);
    match entry.id {
        WHISPER_ID => {
            locate_whisper_ggml(paths, extra).or_else(|| locate_whisper_source(paths, extra))
        }
        YUNET_ID => Some(paths.yunet_model()).filter(|path| path.is_file()),
        AVATAR_YOLO_ID => roots
            .iter()
            .find_map(|root| find_named(root, &[AVATAR_YOLO_NAME], SEARCH_DEPTH)),
        GEFORCE_RUNTIME_ID => locate_runtime(paths),
        _ => {
            let located: Option<Vec<PathBuf>> = entry
                .files
                .iter()
                .map(|file| locate_file(file, &roots, paths.hf_hub.as_deref()))
                .collect();
            located?.into_iter().next()
        }
    }
}

/// The encoder that must sit beside a speech-LLM decoder; decoders load it by file name.
#[must_use]
pub fn speech_llm_encoder(extra_roots: &[PathBuf], paths: &AppPaths) -> Option<PathBuf> {
    locate_file(
        &ENCODER,
        &search_roots(paths, extra_roots),
        paths.hf_hub.as_deref(),
    )
}

/// The state of every catalog entry for the model manager.
#[must_use]
pub fn statuses(hardware: &Hardware, paths: &AppPaths, extra: &[PathBuf]) -> Vec<ModelStatus> {
    let recommended = recommended_speech_llm(hardware);
    CATALOG
        .iter()
        .filter(|entry| is_listed(entry, hardware))
        .map(|entry| ModelStatus {
            id: entry.id,
            role: entry.role,
            label: entry.label,
            source_url: entry.source_url,
            download_bytes: entry.files.iter().map(|file| file.size).sum(),
            found_at: locate(entry, paths, extra),
            blocked: blocked_reason(entry, hardware),
            recommended: recommended == Some(entry.id)
                || entry.id == WHISPER_ID
                || entry.id == YUNET_ID
                || entry.id == MOSSFORMER2_ID
                || entry.id == GEFORCE_RUNTIME_ID,
        })
        .collect()
}

/// The roles a job of `speaker_count` people lacks a usable model for; one person needs neither.
#[must_use]
pub fn missing_speaker_models(speaker_count: u32, rows: &[ModelStatus]) -> Vec<Role> {
    if speaker_count == 1 {
        return Vec::new();
    }
    [Role::Diarize, Role::Separate]
        .into_iter()
        .filter(|role| {
            !rows
                .iter()
                .any(|row| row.role == *role && row.found_at.is_some() && row.blocked.is_none())
        })
        .collect()
}

/// The diarization model to use when none is chosen: an installed one, the recommended build first, so
/// nothing downloads silently.
#[must_use]
pub fn default_diarize_model(hardware: &Hardware, rows: &[ModelStatus]) -> Option<&'static str> {
    let usable = |row: &&ModelStatus| {
        row.role == Role::Diarize && row.found_at.is_some() && row.blocked.is_none()
    };
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
        Hardware {
            is_mac: true,
            memory_gib,
            gpu: None,
        }
    }

    fn geforce(name: &str, vram: u64) -> Hardware {
        Hardware {
            is_mac: false,
            memory_gib: 64,
            gpu: Some(Gpu {
                vendor: GpuVendor::Nvidia,
                name: name.to_owned(),
                memory_gib: vram,
            }),
        }
    }

    #[test]
    fn urls_pin_the_revision() {
        assert_eq!(
            WHISPER_FILES[0].url(),
            "https://huggingface.co/openai/whisper-large-v3-turbo/resolve/41f01f3fe87f28c78e2fbf8b568835947dd65ed9/model.safetensors"
        );
    }

    #[test]
    fn only_the_build_vendors_largest_gpu_is_read() {
        let adapters = "Intel(R) UHD Graphics|2147483648\r\nNVIDIA GeForce RTX 3060|12884901888\r\nNVIDIA GeForce RTX 4090|25769803776\r\nAMD Radeon RX 7900 XTX|17179869184\r\n";
        assert_eq!(
            parse_display_adapters(adapters, GpuVendor::Nvidia),
            Some(Gpu {
                vendor: GpuVendor::Nvidia,
                name: "NVIDIA GeForce RTX 4090".to_owned(),
                memory_gib: 24
            })
        );
        assert_eq!(
            parse_display_adapters(adapters, GpuVendor::Radeon),
            Some(Gpu {
                vendor: GpuVendor::Radeon,
                name: "AMD Radeon RX 7900 XTX".to_owned(),
                memory_gib: 16
            })
        );
    }

    #[test]
    fn another_vendors_gpu_counts_as_no_gpu() {
        assert_eq!(
            parse_display_adapters("AMD Radeon RX 7900 XTX|25769803776", GpuVendor::Nvidia),
            None
        );
        assert_eq!(
            parse_display_adapters("NVIDIA GeForce RTX 4090|25769803776", GpuVendor::Radeon),
            None
        );
    }

    #[test]
    fn unusable_adapter_lines_are_skipped() {
        assert_eq!(parse_display_adapters("", GpuVendor::Nvidia), None);
        assert_eq!(
            parse_display_adapters("no separator", GpuVendor::Nvidia),
            None
        );
        assert_eq!(
            parse_display_adapters("NVIDIA GeForce|many", GpuVendor::Nvidia),
            None
        );
        assert_eq!(
            parse_display_adapters("NVIDIA GeForce|1073741824", GpuVendor::Nvidia),
            None
        );
    }

    #[test]
    fn a_geforce_is_recognised_without_the_vendor_name() {
        assert_eq!(
            parse_display_adapters("GeForce GT|8589934592", GpuVendor::Nvidia),
            Some(Gpu {
                vendor: GpuVendor::Nvidia,
                name: "GeForce GT".to_owned(),
                memory_gib: 8
            })
        );
    }

    #[test]
    fn the_features_choose_the_build_vendor() {
        assert_eq!(build_gpu_vendor(false, false), None);
        assert_eq!(build_gpu_vendor(true, false), Some(GpuVendor::Nvidia));
        assert_eq!(build_gpu_vendor(false, true), Some(GpuVendor::Radeon));
    }

    #[cfg(unix)]
    #[test]
    fn the_gpu_query_runs_its_tool_and_tolerates_failure() {
        let geforce: ToolQuery = (
            "/bin/sh",
            &["-c", "echo 'NVIDIA GeForce RTX 4090|25769803776'"],
        );
        assert_eq!(query_gpu(None, Some(GpuVendor::Nvidia)), None);
        assert_eq!(query_gpu(Some(geforce), None), None);
        assert_eq!(query_gpu(Some(geforce), Some(GpuVendor::Radeon)), None);
        assert_eq!(
            query_gpu(Some(geforce), Some(GpuVendor::Nvidia)),
            Some(Gpu {
                vendor: GpuVendor::Nvidia,
                name: "NVIDIA GeForce RTX 4090".to_owned(),
                memory_gib: 24
            })
        );
        assert_eq!(
            query_gpu(
                Some(("/bin/sh", &["-c", "exit 1"])),
                Some(GpuVendor::Nvidia)
            ),
            None
        );
    }

    #[test]
    fn radeon_gets_the_builds_vulkan_can_run() {
        let radeon = |memory_gib: u64| Hardware {
            is_mac: false,
            memory_gib: 64,
            gpu: Some(Gpu {
                vendor: GpuVendor::Radeon,
                name: "AMD Radeon".to_owned(),
                memory_gib,
            }),
        };
        assert_eq!(
            recommended_speech_llm(&radeon(48)),
            Some("nemotron-3.5-transcribe-q8")
        );
        assert_eq!(
            recommended_speech_llm(&radeon(24)),
            Some("nemotron-3.5-transcribe-iq4")
        );
        assert_eq!(recommended_speech_llm(&radeon(16)), None);
    }

    #[test]
    fn the_backend_depends_on_the_gpu_and_the_downloaded_runtime() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/catalog-backend");
        let _ = std::fs::remove_dir_all(&root);
        let paths = AppPaths {
            data: root.join("data"),
            resources: root.join("resources"),
            hf_hub: None,
        };
        let radeon = Hardware {
            is_mac: false,
            memory_gib: 64,
            gpu: Some(Gpu {
                vendor: GpuVendor::Radeon,
                name: "AMD Radeon".to_owned(),
                memory_gib: 24,
            }),
        };
        assert_eq!(backend_for(&mac(64), &paths), Backend::Default);
        assert_eq!(backend_for(&radeon, &paths), Backend::Vulkan);
        assert_eq!(
            backend_for(&geforce("RTX 4090", 24), &paths),
            Backend::SpeakersOnCpu
        );
        let archive = root.join("data/runtime/geforce/nemo/nemo-speech-cuda.zip");
        std::fs::create_dir_all(archive.parent().unwrap()).unwrap();
        std::fs::write(&archive, b"downloaded but not unpacked yet").unwrap();
        assert_eq!(
            backend_for(&geforce("RTX 4090", 24), &paths),
            Backend::SpeakersOnCpu
        );
        std::fs::write(unpacked_marker(&archive), b"").unwrap();
        assert_eq!(backend_for(&geforce("RTX 4090", 24), &paths), Backend::Cuda);
        assert_eq!(backend_for(&radeon, &paths), Backend::Vulkan);
        assert_eq!(backend_for(&mac(64), &paths), Backend::Default);
    }

    #[test]
    fn the_device_is_the_gpu_only_when_the_model_fits() {
        let iq4 = find_entry("nemotron-3.5-transcribe-iq4").unwrap();
        let big = geforce("RTX 4090", 24);
        let small = geforce("RTX 3060", 12);
        assert_eq!(device_for(iq4, &mac(64), Backend::Default), "auto");
        assert_eq!(device_for(iq4, &big, Backend::SpeakersOnCpu), "cpu");
        assert_eq!(device_for(iq4, &big, Backend::Cuda), "cuda");
        assert_eq!(device_for(iq4, &big, Backend::Vulkan), "vulkan");
        assert_eq!(device_for(iq4, &small, Backend::Cuda), "cpu");
        assert_eq!(device_for(iq4, &small, Backend::Vulkan), "cpu");
        assert_eq!(device_for(iq4, &mac(64), Backend::Vulkan), "cpu");
    }

    #[test]
    fn only_a_model_with_a_limit_can_be_too_long() {
        let unlimited = find_entry("yunet").unwrap();
        assert_eq!(too_long(unlimited, 1e9), None);
        let limited = ModelEntry {
            max_audio_seconds: Some(600),
            ..*unlimited
        };
        assert_eq!(too_long(&limited, 600.0), None);
        assert_eq!(
            too_long(&limited, 601.0),
            Some(UserError::TooLongForModel {
                model: "yunet".to_owned(),
                limit_minutes: 10
            })
        );
    }

    #[test]
    fn macs_get_the_build_their_memory_fits() {
        assert_eq!(recommended_speech_llm(&mac(16)), None);
        assert_eq!(
            recommended_speech_llm(&mac(24)),
            Some("nemotron-3.5-transcribe-q2l")
        );
        assert_eq!(
            recommended_speech_llm(&mac(48)),
            Some("nemotron-3.5-transcribe-iq4")
        );
        assert_eq!(
            recommended_speech_llm(&mac(64)),
            Some("nemotron-3.5-transcribe-q8")
        );
    }

    #[test]
    fn geforce_gets_its_own_builds() {
        assert_eq!(
            recommended_speech_llm(&geforce("NVIDIA GeForce RTX 5090", 32)),
            Some("nemotron-3.5-transcribe-nvfp4")
        );
        assert_eq!(
            recommended_speech_llm(&geforce("NVIDIA GeForce RTX 4090", 24)),
            Some("nemotron-3.5-transcribe-iq4")
        );
        assert_eq!(
            recommended_speech_llm(&geforce("NVIDIA GeForce RTX 4080", 16)),
            Some("nemotron-3.5-transcribe-q2m")
        );
        assert_eq!(
            recommended_speech_llm(&geforce("NVIDIA GeForce RTX 3060", 12)),
            None
        );
    }

    #[test]
    fn a_12gb_vm_cannot_run_the_30b_model_but_can_run_the_small_one() {
        let vm = Hardware {
            is_mac: false,
            memory_gib: 12,
            gpu: None,
        };
        let large = find_entry("nemotron-3.5-transcribe-iq4").unwrap();
        let small = find_entry("whisper-large-v3-turbo").unwrap();
        assert_eq!(
            blocked_reason(large, &vm).unwrap(),
            UserError::NeedsMemory {
                model: "nemotron-3.5-transcribe-iq4".to_owned(),
                required: 24,
                available: 12
            }
        );
        assert_eq!(blocked_reason(small, &vm), None);
    }

    #[test]
    fn cuda_builds_need_a_nvidia_gpu() {
        let entry = find_entry("nemotron-3.5-transcribe-nvfp4").unwrap();
        assert_eq!(
            blocked_reason(entry, &mac(128)).unwrap(),
            UserError::NeedsNvidia {
                model: "nemotron-3.5-transcribe-nvfp4".to_owned()
            }
        );
    }

    #[test]
    fn the_geforce_runtime_is_listed_only_with_a_geforce() {
        let entry = find_entry("geforce-runtime").unwrap();
        let radeon = Hardware {
            is_mac: false,
            memory_gib: 64,
            gpu: Some(Gpu {
                vendor: GpuVendor::Radeon,
                name: "AMD Radeon".to_owned(),
                memory_gib: 24,
            }),
        };
        assert!(!is_listed(entry, &mac(48)));
        assert!(!is_listed(entry, &radeon));
        assert!(is_listed(entry, &geforce("NVIDIA GeForce RTX 4090", 24)));
        assert!(is_listed(find_entry("yunet").unwrap(), &radeon));
    }

    #[test]
    fn every_downloadable_hash_is_a_sha256() {
        let bad: Vec<&str> = CATALOG
            .iter()
            .flat_map(|entry| entry.files.iter())
            .filter(|file| {
                file.sha256.len() != 64 || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            })
            .map(|file| file.path)
            .collect();
        assert_eq!(bad, Vec::<&str>::new());
    }

    #[test]
    fn const_builders_also_work_at_run_time() {
        let file = nemotron_file("a.gguf", 1, "00");
        assert_eq!(
            file.url(),
            "https://huggingface.co/AtomicChat/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-GGUF/resolve/dbd06c5cad7a2f33c8e06be2fb3ef814dbd9aa59/a.gguf"
        );
        let entry = nemotron("id", "label", &[], 3);
        assert_eq!(entry.min_memory_gib, 3);
    }

    #[test]
    fn direct_files_have_no_repository_and_may_be_archives() {
        let file = GEFORCE_FILES[0];
        assert_eq!(file.url(), "https://github.com/NVIDIA/NeMo-Speech.cpp/releases/download/v0.3.0/nemo-speech-0.3.0-windows-x86_64-cuda.zip");
        assert_eq!(file.hf_repo(), None);
        assert!(file.is_archive());
        assert!(!WHISPER_FILES[0].is_archive());
        assert_eq!(
            WHISPER_FILES[0].hf_repo(),
            Some("openai/whisper-large-v3-turbo")
        );
    }

    #[test]
    fn memory_output_must_be_a_number() {
        assert_eq!(parse_memory_bytes(b" 1024\n").unwrap(), 1024);
        assert_eq!(
            parse_memory_bytes(b"lots").unwrap_err().to_string(),
            "unexpected memory size: lots"
        );
    }

    #[test]
    fn a_geforce_counts_its_larger_memory_for_speaker_models_only() {
        let hardware = Hardware {
            is_mac: false,
            memory_gib: 8,
            gpu: Some(Gpu {
                vendor: GpuVendor::Nvidia,
                name: "RTX 4090".to_owned(),
                memory_gib: 24,
            }),
        };
        assert_eq!(
            blocked_reason(
                find_entry("nemotron-3.5-transcribe-iq4").unwrap(),
                &hardware
            ),
            None
        );
        assert_eq!(
            blocked_reason(
                find_entry("whisper-large-v3-turbo").unwrap(),
                &Hardware {
                    memory_gib: 2,
                    ..hardware
                }
            ),
            Some(UserError::NeedsMemory {
                model: "whisper-large-v3-turbo".to_owned(),
                required: 4,
                available: 2
            })
        );
    }

    #[test]
    fn the_runtime_is_recommended_where_it_is_listed() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/catalog-recommended");
        let paths = AppPaths {
            data: root.join("data"),
            resources: root.join("resources"),
            hf_hub: None,
        };
        let recommended: Vec<&str> = statuses(&geforce("NVIDIA GeForce RTX 3060", 12), &paths, &[])
            .iter()
            .filter(|row| row.recommended)
            .map(|row| row.id)
            .collect();
        assert_eq!(
            recommended,
            vec![
                "whisper-large-v3-turbo",
                "mossformer2-ss-16k",
                "yunet",
                "geforce-runtime"
            ]
        );
    }

    #[test]
    fn mossformer2_is_downloaded_from_its_pinned_revision_and_never_chosen_for_speakers() {
        let entry = find_entry("mossformer2-ss-16k").unwrap();
        assert_eq!(entry.role, Role::Separate);
        assert_eq!(
            entry.files[0].url(),
            "https://huggingface.co/kkk4oru/mossformer2-ss-16k-onnx/resolve/fb27e5ff961cff11372473f9808ca00d83144f69/model.onnx"
        );
        let rows = [ModelStatus {
            id: entry.id,
            role: entry.role,
            label: entry.label,
            source_url: entry.source_url,
            download_bytes: 1,
            found_at: Some("/models/moss".into()),
            blocked: None,
            recommended: true,
        }];
        assert_eq!(
            default_diarize_model(&geforce("NVIDIA GeForce RTX 3060", 12), &rows),
            None
        );
    }

    fn row(id: &'static str, found: bool, blocked: bool) -> ModelStatus {
        ModelStatus {
            id,
            role: Role::Diarize,
            label: "",
            source_url: "",
            download_bytes: 0,
            found_at: found.then(|| PathBuf::from("/m")),
            blocked: blocked.then_some(UserError::NoWords),
            recommended: false,
        }
    }

    #[test]
    fn one_person_needs_no_speaker_models() {
        assert_eq!(missing_speaker_models(1, &[]), Vec::<Role>::new());
    }

    fn separator(found: bool, blocked: bool) -> ModelStatus {
        ModelStatus {
            role: Role::Separate,
            ..row("mossformer2-ss-16k", found, blocked)
        }
    }

    #[test]
    fn several_people_or_automatic_need_both_models() {
        let nemotron = || row("nemotron-3.5-transcribe-q2l", true, false);
        assert_eq!(
            missing_speaker_models(0, &[nemotron(), separator(true, false)]),
            Vec::<Role>::new()
        );
        assert_eq!(
            missing_speaker_models(2, &[nemotron(), separator(true, false)]),
            Vec::<Role>::new()
        );
        assert_eq!(
            missing_speaker_models(3, &[]),
            vec![Role::Diarize, Role::Separate]
        );
        assert_eq!(
            missing_speaker_models(0, &[nemotron()]),
            vec![Role::Separate]
        );
        assert_eq!(
            missing_speaker_models(2, &[separator(true, false)]),
            vec![Role::Diarize]
        );
    }

    #[test]
    fn a_model_not_downloaded_or_too_big_for_this_machine_counts_as_missing() {
        assert_eq!(
            missing_speaker_models(
                2,
                &[
                    row("nemotron-3.5-transcribe-q2l", false, false),
                    separator(true, true)
                ]
            ),
            vec![Role::Diarize, Role::Separate]
        );
        assert_eq!(
            missing_speaker_models(
                2,
                &[
                    ModelStatus {
                        role: Role::Transcribe,
                        ..row("whisper-large-v3-turbo", true, false)
                    },
                    separator(false, false)
                ]
            ),
            vec![Role::Diarize, Role::Separate]
        );
    }
}
