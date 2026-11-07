//! Downloadable models and runtimes, per hardware, and detection of copies already on disk.

use crate::error::{AppError, AppResult, MemoryKind, UserError};
use crate::paths::AppPaths;
use crate::process::{background_command, run_capture};
use crate::settings::{SeparationBackend, Settings};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const HF_RESOLVE: &str = "https://huggingface.co";
const GIB: u64 = 1 << 30;
/// Below this an adapter is an integrated GPU sharing system memory: slower than the CPU path for speech.
const MIN_GPU_MEMORY_GIB: u64 = 4;
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
const NEMOTRON_SOURCE: &str =
    "https://huggingface.co/nvidia/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B";
const ENCODER: FileSpec = FileSpec {
    url_kind: UrlKind::HuggingFace {
        repo: "kkk4oru/nemotron-3.5-speaker-encoder-gguf",
        revision: "a0054b68c46cbe33ade39746bd5be4bae3792524",
    },
    path: "NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Speaker-Encoder-F16.gguf",
    size: 200_779_392,
    sha256: "657afaeac267f6f8f38501927a354eeac1da19b34b87f10c71d320eb5bb757c5",
};
/// An encoder an earlier download left in a model folder: found by name and size, never fetched.
const EARLIER_ENCODER_NAME: &str = "NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Encoder-F16.gguf";
const EARLIER_ENCODER_BYTES: u64 = 1_670_410_080;
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
const NEMO_CUDA_URL: &str =
    "https://github.com/NVIDIA/NeMo-Speech.cpp/releases/download/v0.3.0/nemo-speech-0.3.0-windows-x86_64-cuda.zip";
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

/// Where a model runs on this machine.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunsOn {
    Gpu,
    Cpu,
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
    /// RAM to run on the CPU.
    pub min_ram_gib: u64,
    /// GPU memory to run on the GPU; 0 never runs there.
    pub min_gpu_memory_gib: u64,
    /// RAM beside the GPU memory while it runs on the GPU.
    pub min_ram_with_gpu_gib: u64,
}

/// Nemotron 3.5's speaker branch alone: its encoder and an empty decoder the app makes (`engines`).
pub const NEMOTRON35_SPEAKER_ID: &str = "nemotron-3.5-speaker";
/// An hour of audio on the CPU peaks at 2.85 GB; an earlier, full encoder takes 5.0 GB.
const NEMOTRON35_SPEAKER_RAM_GIB: u64 = 3;
/// An hour of audio on Metal raises the machine's GPU use by 2.98 to 3.17 GB against an idle base that
/// moves about 0.3 GB (three runs). A GPU that still fails gives way to the CPU.
/// shortcut: no discrete GPU has been measured; replace when Vulkan and CUDA have.
const NEMOTRON35_SPEAKER_GPU_MEMORY_GIB: u64 = 4;
/// Where the encoder comes from; the one thing to change when another copy is published.
const NEMOTRON35_SPEAKER_SOURCE: &str = NEMOTRON_SOURCE;
const NEMOTRON35_SPEAKER_FILES: [FileSpec; 1] = [ENCODER];
/// Whisper large-v3-turbo, never another: 2.4 GB of RAM on the CPU, about 2 GB of GPU memory.
const WHISPER_RAM_GIB: u64 = 3;
const WHISPER_GPU_MEMORY_GIB: u64 = 2;
/// Several people need this much RAM: the largest step peaks at 2.9 GB (a 3 GiB budget), on top of what
/// `schedule` keeps for the system and the app. With the second, MossFormer2 also runs its second pass.
const SEVERAL_SPEAKERS_RAM_GIB: u64 = 8;
const DEEP_SEPARATE_RAM_GIB: u64 = 16;

// Everything about where MossFormer2 comes from: a new build replaces these and nothing else.
pub const MOSSFORMER2_ID: &str = "mossformer2-ss-16k";
const MOSSFORMER2_SOURCE: &str = "https://huggingface.co/kkk4oru/mossformer2-ss-16k-onnx";
/// Measured with ONNX Runtime on the CPU, its memory pattern off: 1.65 GB for the longest range, 8 s.
const MOSSFORMER2_MEMORY_GIB: u64 = 2;
pub const MOSSFORMER2_COREML_ID: &str = "mossformer2-ss-16k-coreml";
const MOSSFORMER2_COREML_SOURCE: &str = "https://huggingface.co/kkk4oru/mossformer2-ss-16k-coreml";
const MOSSFORMER2_COREML_PACKAGE: &str = "model.mlpackage";
/// About 1.1 GB of unified memory in all, partly estimated.
const MOSSFORMER2_COREML_MEMORY_GIB: u64 = 2;
const fn coreml_file(path: &'static str, size: u64, sha256: &'static str) -> FileSpec {
    FileSpec {
        url_kind: UrlKind::HuggingFace {
            repo: "kkk4oru/mossformer2-ss-16k-coreml",
            revision: "00ad477c4ee88956c4852161f0bcb21d0fd40b98",
        },
        path,
        size,
        sha256,
    }
}
const MOSSFORMER2_COREML_FILES: [FileSpec; 3] = [
    coreml_file(
        "model.mlpackage/Manifest.json",
        617,
        "ecc977b873a7c7b454d54cc909b55b72a5d362a2dc7b594fdcb4e706bf777c1b",
    ),
    coreml_file(
        "model.mlpackage/Data/com.apple.CoreML/model.mlmodel",
        1_811_303,
        "76d8219d0e23750f9b14900b58f211aeeb18e6b1acfff2a893c6ecb0bc307bce",
    ),
    coreml_file(
        "model.mlpackage/Data/com.apple.CoreML/weights/weight.bin",
        129_964_736,
        "3c64c07066c55bb840da968804f75976638fe3fbb96ec26494fd27ee0ec7bb35",
    ),
];
const MOSSFORMER2_FILES: [FileSpec; 1] = [FileSpec {
    url_kind: UrlKind::HuggingFace {
        repo: "kkk4oru/mossformer2-ss-16k-onnx",
        revision: "fb27e5ff961cff11372473f9808ca00d83144f69",
    },
    path: "model.onnx",
    size: 230_156_008,
    sha256: "d658037076e27c76b347f98b0b18eb0f51ef5df9da09a359dbb058fec52db2de",
}];

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
pub const CATALOG: [ModelEntry; 7] = [
    ModelEntry {
        id: WHISPER_ID,
        role: Role::Transcribe,
        label: "Whisper large-v3-turbo",
        source_url: "https://huggingface.co/openai/whisper-large-v3-turbo",
        files: &WHISPER_FILES,
        min_ram_gib: WHISPER_RAM_GIB,
        min_gpu_memory_gib: WHISPER_GPU_MEMORY_GIB,
        min_ram_with_gpu_gib: WHISPER_GPU_MEMORY_GIB,
    },
    ModelEntry {
        id: NEMOTRON35_SPEAKER_ID,
        role: Role::Diarize,
        label: "Nemotron 3.5 speakers (encoder only)",
        source_url: NEMOTRON35_SPEAKER_SOURCE,
        files: &NEMOTRON35_SPEAKER_FILES,
        min_ram_gib: NEMOTRON35_SPEAKER_RAM_GIB,
        min_gpu_memory_gib: NEMOTRON35_SPEAKER_GPU_MEMORY_GIB,
        min_ram_with_gpu_gib: 1,
    },
    ModelEntry {
        id: MOSSFORMER2_ID,
        role: Role::Separate,
        label: "MossFormer2 SS 16K",
        source_url: MOSSFORMER2_SOURCE,
        files: &MOSSFORMER2_FILES,
        min_ram_gib: MOSSFORMER2_MEMORY_GIB,
        min_gpu_memory_gib: 0,
        min_ram_with_gpu_gib: 0,
    },
    ModelEntry {
        id: MOSSFORMER2_COREML_ID,
        role: Role::Separate,
        label: "MossFormer2 on Core ML (GPU)",
        source_url: MOSSFORMER2_COREML_SOURCE,
        files: &MOSSFORMER2_COREML_FILES,
        min_ram_gib: MOSSFORMER2_COREML_MEMORY_GIB,
        min_gpu_memory_gib: MOSSFORMER2_COREML_MEMORY_GIB,
        min_ram_with_gpu_gib: 1,
    },
    ModelEntry {
        id: YUNET_ID,
        role: Role::Face,
        label: "YuNet",
        source_url: "https://huggingface.co/opencv/face_detection_yunet",
        files: &[],
        min_ram_gib: 1,
        min_gpu_memory_gib: 0,
        min_ram_with_gpu_gib: 0,
    },
    ModelEntry {
        id: AVATAR_YOLO_ID,
        role: Role::Face,
        label: "Avatar face YOLO",
        source_url: "",
        files: &[],
        min_ram_gib: 1,
        min_gpu_memory_gib: 0,
        min_ram_with_gpu_gib: 0,
    },
    ModelEntry {
        id: GEFORCE_RUNTIME_ID,
        role: Role::Runtime,
        label: "GeForce CUDA runtime for speakers (optional)",
        source_url: "https://github.com/NVIDIA/NeMo-Speech.cpp/releases/tag/v0.3.0",
        files: &GEFORCE_FILES,
        min_ram_gib: 0,
        min_gpu_memory_gib: 0,
        min_ram_with_gpu_gib: 0,
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

impl Hardware {
    /// GPU memory to run models in. A Mac's memory is unified, so it counts as this and as RAM.
    #[must_use]
    pub fn gpu_memory_gib(&self) -> Option<u64> {
        if self.is_mac {
            return Some(self.memory_gib);
        }
        self.gpu.as_ref().map(|gpu| gpu.memory_gib)
    }
}

/// Where a model runs: the GPU while its memory holds it, else the CPU from RAM; `None` when neither
/// does. RAM and GPU memory are judged apart, as each way of running needs different amounts.
#[must_use]
pub fn runs_on(entry: &ModelEntry, hardware: &Hardware) -> Option<RunsOn> {
    let fits_gpu = entry.min_gpu_memory_gib > 0
        && hardware
            .gpu_memory_gib()
            .is_some_and(|memory| memory >= entry.min_gpu_memory_gib)
        && hardware.memory_gib >= entry.min_ram_with_gpu_gib;
    if fits_gpu {
        Some(RunsOn::Gpu)
    } else if hardware.memory_gib >= entry.min_ram_gib {
        Some(RunsOn::Cpu)
    } else {
        None
    }
}

/// The `--device` for a model: the GPU when the model fits in its memory, else the CPU.
#[must_use]
pub fn device_for(entry: &ModelEntry, hardware: &Hardware, backend: Backend) -> &'static str {
    let on_gpu = runs_on(entry, hardware) == Some(RunsOn::Gpu);
    match backend {
        Backend::Default if on_gpu || !hardware.is_mac => "auto",
        Backend::Default | Backend::SpeakersOnCpu => "cpu",
        Backend::Cuda | Backend::Vulkan if !on_gpu => "cpu",
        Backend::Cuda => "cuda",
        Backend::Vulkan => "vulkan",
    }
}

/// Why this hardware cannot run the model, if it cannot.
#[must_use]
pub fn blocked_reason(entry: &ModelEntry, hardware: &Hardware) -> Option<UserError> {
    if runs_on(entry, hardware).is_some() {
        return None;
    }
    let (kind, required, available) = if hardware.is_mac {
        (MemoryKind::Unified, entry.min_ram_gib, hardware.memory_gib)
    } else {
        (MemoryKind::Ram, entry.min_ram_gib, hardware.memory_gib)
    };
    Some(UserError::NeedsMemory {
        model: entry.id.to_owned(),
        required,
        available,
        kind,
    })
}

/// The runtime exists only for a GeForce; every other machine never sees it.
fn is_listed(entry: &ModelEntry, hardware: &Hardware) -> bool {
    match entry.id {
        GEFORCE_RUNTIME_ID => has_nvidia(hardware),
        MOSSFORMER2_COREML_ID => hardware.is_mac,
        _ => true,
    }
}

fn find_named(root: &Path, names: &[&str], depth: usize) -> Option<PathBuf> {
    find_accepted(root, names, depth, &|_| true)
}

/// `accepts` tells apart models that share a file name, like two `model.onnx`.
fn find_accepted(
    root: &Path,
    names: &[&str],
    depth: usize,
    accepts: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    let mut subfolders = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let path = entry.path();
        if names.iter().any(|wanted| name.to_string_lossy() == *wanted)
            && path.is_file()
            && accepts(&path)
        {
            return Some(path);
        }
        let hidden = name.to_string_lossy().starts_with('.');
        if depth > 0 && !hidden && path.is_dir() {
            subfolders.push(path);
        }
    }
    subfolders
        .iter()
        .find_map(|folder| find_accepted(folder, names, depth - 1, accepts))
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
        |path: &Path| std::fs::metadata(path).is_ok_and(|meta| meta.len() == file.size);
    let from_roots = roots
        .iter()
        .filter_map(|root| find_accepted(root, &[name], SEARCH_DEPTH, &matches_size));
    let from_cache = file
        .hf_repo()
        .into_iter()
        .flat_map(|repo| hf_snapshots(hub, repo))
        .map(|snapshot| snapshot.join(name));
    from_roots.chain(from_cache).find(|path| matches_size(path))
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
        NEMOTRON35_SPEAKER_ID => {
            locate_file(&ENCODER, &roots, paths.hf_hub.as_deref()).or_else(|| {
                let right_size = |path: &Path| {
                    std::fs::metadata(path).is_ok_and(|meta| meta.len() == EARLIER_ENCODER_BYTES)
                };
                roots.iter().find_map(|root| {
                    find_accepted(root, &[EARLIER_ENCODER_NAME], SEARCH_DEPTH, &right_size)
                })
            })
        }
        // A package reads its files by their places inside it, so only the copy the app downloaded does.
        MOSSFORMER2_COREML_ID => {
            let folder = paths.models_dir().join(entry.id);
            entry
                .files
                .iter()
                .all(|file| {
                    std::fs::metadata(folder.join(file.path))
                        .is_ok_and(|meta| meta.len() == file.size)
                })
                .then(|| folder.join(MOSSFORMER2_COREML_PACKAGE))
        }
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
    locate(find_entry(NEMOTRON35_SPEAKER_ID)?, paths, extra_roots)
}

/// The state of every catalog entry for the model manager.
#[must_use]
pub fn statuses(hardware: &Hardware, paths: &AppPaths, extra: &[PathBuf]) -> Vec<ModelStatus> {
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
            recommended: entry.id == WHISPER_ID
                || entry.id == YUNET_ID
                || entry.id == NEMOTRON35_SPEAKER_ID
                || entry.id == MOSSFORMER2_ID,
        })
        .collect()
}

/// What a job for several people runs and where, within this machine's RAM and GPU memory. Whisper is
/// always the same model: only where it runs changes. Steps run one after the other in processes of their
/// own, so what must fit is the largest of them.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct SpeakerPlan {
    pub diarize: &'static str,
    pub whisper_on: RunsOn,
    pub diarize_on: RunsOn,
    /// `None` skips MossFormer2: one row at a time.
    pub separate_on: RunsOn,
    /// Which build of MossFormer2.
    pub separator: &'static str,
    /// Three people at once go through the model twice, for up to four rows.
    pub deep: bool,
}

/// Where Whisper runs on this machine, for any number of speakers: the same model on either.
#[must_use]
pub fn whisper_runs_on(hardware: &Hardware) -> RunsOn {
    find_entry(WHISPER_ID)
        .and_then(|entry| runs_on(entry, hardware))
        .unwrap_or(RunsOn::Cpu)
}

fn is_usable(rows: &[ModelStatus], id: &str) -> bool {
    rows.iter()
        .any(|row| row.id == id && row.found_at.is_some() && row.blocked.is_none())
}

/// The separator to run: the one the settings prefer, or the other if only that is here. Core ML exists
/// only on a Mac.
fn usable_separator(
    backend: SeparationBackend,
    hardware: &Hardware,
    rows: &[ModelStatus],
) -> Option<&'static str> {
    let coreml = (hardware.is_mac && is_usable(rows, MOSSFORMER2_COREML_ID))
        .then_some(MOSSFORMER2_COREML_ID);
    let onnx = is_usable(rows, MOSSFORMER2_ID).then_some(MOSSFORMER2_ID);
    match backend {
        SeparationBackend::Coreml => coreml.or(onnx),
        SeparationBackend::Onnx => onnx.or(coreml),
    }
}

fn separator_to_fetch(settings: &Settings, hardware: &Hardware) -> &'static str {
    if hardware.is_mac && settings.separation_backend == SeparationBackend::Coreml {
        MOSSFORMER2_COREML_ID
    } else {
        MOSSFORMER2_ID
    }
}

/// What a job for these settings runs; `None` for one person, who needs no speaker model at all.
/// Automatic takes what is here and refuses only with no speaker model; a quality asked for by name
/// refuses without its models.
pub fn choose_speaker_plan(
    settings: &Settings,
    hardware: &Hardware,
    rows: &[ModelStatus],
) -> AppResult<Option<SpeakerPlan>> {
    if settings.speaker_count == 1 {
        return Ok(None);
    }
    // Before the models: fetching them would not help.
    if hardware.memory_gib < SEVERAL_SPEAKERS_RAM_GIB {
        return Err(UserError::NeedsMemory {
            model: MOSSFORMER2_ID.to_owned(),
            required: SEVERAL_SPEAKERS_RAM_GIB,
            available: hardware.memory_gib,
            kind: if hardware.is_mac {
                MemoryKind::Unified
            } else {
                MemoryKind::Ram
            },
        }
        .into());
    }
    let separator = usable_separator(settings.separation_backend, hardware, rows);
    let lacks_speaker_model = !is_usable(rows, NEMOTRON35_SPEAKER_ID);
    let Some(separator) = separator.filter(|_| !lacks_speaker_model) else {
        return Err(UserError::SpeakerModelsMissing {
            missing: lacks_speaker_model
                .then_some(Role::Diarize)
                .into_iter()
                .chain(separator.is_none().then_some(Role::Separate))
                .collect(),
            suggested: Some(
                if lacks_speaker_model {
                    NEMOTRON35_SPEAKER_ID
                } else {
                    separator_to_fetch(settings, hardware)
                }
                .to_owned(),
            ),
        }
        .into());
    };
    let place = |id: &str| {
        find_entry(id)
            .and_then(|entry| runs_on(entry, hardware))
            .unwrap_or(RunsOn::Cpu)
    };
    Ok(Some(SpeakerPlan {
        diarize: NEMOTRON35_SPEAKER_ID,
        whisper_on: whisper_runs_on(hardware),
        diarize_on: place(NEMOTRON35_SPEAKER_ID),
        separator,
        separate_on: place(separator),
        deep: hardware.memory_gib >= DEEP_SEPARATE_RAM_GIB,
    }))
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
        let iq4 = &ModelEntry {
            min_ram_gib: 24,
            min_gpu_memory_gib: 24,
            ..*find_entry("nemotron-3.5-speaker").unwrap()
        };
        let big = geforce("RTX 4090", 24);
        let small = geforce("RTX 3060", 12);
        assert_eq!(device_for(iq4, &mac(64), Backend::Default), "auto");
        assert_eq!(device_for(iq4, &big, Backend::SpeakersOnCpu), "cpu");
        assert_eq!(device_for(iq4, &big, Backend::Cuda), "cuda");
        assert_eq!(device_for(iq4, &big, Backend::Vulkan), "vulkan");
        assert_eq!(device_for(iq4, &small, Backend::Cuda), "cpu");
        assert_eq!(device_for(iq4, &small, Backend::Vulkan), "cpu");
        assert_eq!(device_for(iq4, &mac(16), Backend::Default), "cpu");
    }

    #[test]
    fn a_machine_too_small_for_the_speaker_branch_says_how_much_it_needs() {
        let vm = Hardware {
            is_mac: false,
            memory_gib: 2,
            gpu: None,
        };
        assert_eq!(
            blocked_reason(find_entry("nemotron-3.5-speaker").unwrap(), &vm).unwrap(),
            UserError::NeedsMemory {
                model: "nemotron-3.5-speaker".to_owned(),
                required: 3,
                available: 2,
                kind: MemoryKind::Ram,
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
    fn a_geforce_keeps_a_speaker_model_in_its_memory_with_little_ram() {
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
            blocked_reason(find_entry("nemotron-3.5-speaker").unwrap(), &hardware),
            None
        );
        assert_eq!(
            blocked_reason(
                find_entry("whisper-large-v3-turbo").unwrap(),
                &Hardware {
                    memory_gib: 1,
                    ..hardware
                }
            ),
            Some(UserError::NeedsMemory {
                model: "whisper-large-v3-turbo".to_owned(),
                required: 3,
                available: 1,
                kind: MemoryKind::Ram,
            })
        );
    }

    #[test]
    fn the_optional_runtime_is_listed_but_not_recommended() {
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
                "nemotron-3.5-speaker",
                "mossformer2-ss-16k",
                "yunet"
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

    fn moss(found: bool) -> ModelStatus {
        ModelStatus {
            role: Role::Separate,
            ..row("mossformer2-ss-16k", found, false)
        }
    }

    fn speaker_branch() -> ModelStatus {
        row("nemotron-3.5-speaker", true, false)
    }

    fn asked() -> Settings {
        Settings {
            speaker_count: 2,
            ..Settings::default()
        }
    }

    fn windows(ram: u64, gram: Option<u64>) -> Hardware {
        Hardware {
            is_mac: false,
            memory_gib: ram,
            gpu: gram.map(|memory_gib| Gpu {
                vendor: GpuVendor::Radeon,
                name: "R".to_owned(),
                memory_gib,
            }),
        }
    }

    #[test]
    fn one_person_has_no_plan_and_needs_no_speaker_models() {
        let settings = Settings {
            speaker_count: 1,
            ..Settings::default()
        };
        assert_eq!(choose_speaker_plan(&settings, &mac(64), &[]).unwrap(), None);
    }

    #[test]
    fn several_people_always_run_all_three_and_the_memory_decides_where() {
        let rows = [speaker_branch(), moss(true)];
        let table = |hardware: Hardware| {
            let plan = choose_speaker_plan(&asked(), &hardware, &rows)
                .unwrap()
                .unwrap();
            assert_eq!(plan.diarize, "nemotron-3.5-speaker");
            assert_eq!(plan.separator, "mossformer2-ss-16k");
            (
                plan.whisper_on,
                plan.diarize_on,
                plan.separate_on,
                plan.deep,
            )
        };
        assert_eq!(
            table(windows(8, None)),
            (RunsOn::Cpu, RunsOn::Cpu, RunsOn::Cpu, false)
        );
        assert_eq!(
            table(windows(8, Some(3))),
            (RunsOn::Gpu, RunsOn::Cpu, RunsOn::Cpu, false)
        );
        assert_eq!(
            table(windows(8, Some(4))),
            (RunsOn::Gpu, RunsOn::Gpu, RunsOn::Cpu, false)
        );
        assert_eq!(
            table(windows(16, None)),
            (RunsOn::Cpu, RunsOn::Cpu, RunsOn::Cpu, true)
        );
        assert_eq!(
            table(windows(16, Some(12))),
            (RunsOn::Gpu, RunsOn::Gpu, RunsOn::Cpu, true)
        );
        assert_eq!(
            table(mac(8)),
            (RunsOn::Gpu, RunsOn::Gpu, RunsOn::Cpu, false)
        );
    }

    #[test]
    fn too_little_memory_says_how_much_it_takes_whatever_is_here() {
        let refused = |hardware: Hardware, rows: &[ModelStatus]| {
            serde_json::to_value(choose_speaker_plan(&asked(), &hardware, rows).unwrap_err())
                .unwrap()
        };
        let low = refused(windows(7, Some(12)), &[speaker_branch(), moss(true)]);
        assert_eq!(low["required"], 8);
        assert_eq!(low["available"], 7);
        assert_eq!(low["kind"], "ram");
        let mac_low = refused(mac(4), &[]);
        assert_eq!(mac_low["required"], 8);
        assert_eq!(mac_low["available"], 4);
        assert_eq!(mac_low["kind"], "unified");
    }

    #[test]
    fn missing_models_stop_the_job_and_name_what_to_fetch() {
        let refused = |rows: &[ModelStatus]| {
            serde_json::to_value(choose_speaker_plan(&asked(), &mac(64), rows).unwrap_err())
                .unwrap()
        };
        let both = refused(&[]);
        assert_eq!(both["missing"], serde_json::json!(["diarize", "separate"]));
        assert_eq!(both["suggested"], "nemotron-3.5-speaker");
        let speaker = refused(&[moss(true)]);
        assert_eq!(speaker["missing"], serde_json::json!(["diarize"]));
        assert_eq!(speaker["suggested"], "nemotron-3.5-speaker");
        let separator = refused(&[speaker_branch()]);
        assert_eq!(separator["missing"], serde_json::json!(["separate"]));
        assert_eq!(separator["suggested"], "mossformer2-ss-16k");
        let unable = refused(&[row("nemotron-3.5-speaker", true, true), moss(true)]);
        assert_eq!(unable["missing"], serde_json::json!(["diarize"]));
    }

    fn coreml(found: bool) -> ModelStatus {
        ModelStatus {
            role: Role::Separate,
            ..row("mossformer2-ss-16k-coreml", found, false)
        }
    }

    #[test]
    fn a_mac_runs_the_separator_the_settings_prefer_or_the_one_that_is_here() {
        let chosen = |backend: SeparationBackend, hardware: Hardware, rows: &[ModelStatus]| {
            let plan = choose_speaker_plan(
                &Settings {
                    separation_backend: backend,
                    ..asked()
                },
                &hardware,
                rows,
            )
            .unwrap()
            .unwrap();
            (plan.separator, plan.separate_on)
        };
        let both = [speaker_branch(), moss(true), coreml(true)];
        assert_eq!(
            chosen(SeparationBackend::Coreml, mac(16), &both),
            ("mossformer2-ss-16k-coreml", RunsOn::Gpu)
        );
        assert_eq!(
            chosen(SeparationBackend::Onnx, mac(16), &both),
            ("mossformer2-ss-16k", RunsOn::Cpu)
        );
        assert_eq!(
            chosen(
                SeparationBackend::Coreml,
                mac(16),
                &[speaker_branch(), moss(true)]
            ),
            ("mossformer2-ss-16k", RunsOn::Cpu)
        );
        assert_eq!(
            chosen(
                SeparationBackend::Onnx,
                mac(16),
                &[speaker_branch(), coreml(true)]
            ),
            ("mossformer2-ss-16k-coreml", RunsOn::Gpu)
        );
        assert_eq!(
            chosen(SeparationBackend::Coreml, windows(16, None), &both),
            ("mossformer2-ss-16k", RunsOn::Cpu)
        );
    }

    #[test]
    fn a_mac_set_to_core_ml_is_told_to_fetch_core_ml() {
        let refused = serde_json::to_value(
            choose_speaker_plan(
                &Settings {
                    separation_backend: SeparationBackend::Coreml,
                    ..asked()
                },
                &mac(16),
                &[speaker_branch()],
            )
            .unwrap_err(),
        )
        .unwrap();
        assert_eq!(refused["suggested"], "mossformer2-ss-16k-coreml");
    }

    #[test]
    fn whisper_is_one_model_that_only_moves_between_gpu_and_cpu() {
        assert_eq!(whisper_runs_on(&windows(8, Some(4))), RunsOn::Gpu);
        assert_eq!(whisper_runs_on(&windows(8, Some(1))), RunsOn::Cpu);
        assert_eq!(whisper_runs_on(&windows(8, None)), RunsOn::Cpu);
        assert_eq!(whisper_runs_on(&windows(1, None)), RunsOn::Cpu);
        assert_eq!(whisper_runs_on(&mac(8)), RunsOn::Gpu);
    }

    #[test]
    fn ram_and_gpu_memory_are_judged_apart() {
        let entry = &ModelEntry {
            min_ram_gib: 20,
            min_gpu_memory_gib: 20,
            min_ram_with_gpu_gib: 4,
            ..*find_entry("nemotron-3.5-speaker").unwrap()
        };
        let windows = |ram: u64, gram: u64| windows(ram, Some(gram));
        // GPU memory enough: little RAM is needed beside it.
        assert_eq!(runs_on(entry, &windows(8, 24)), Some(RunsOn::Gpu));
        assert_eq!(
            device_for(entry, &windows(8, 24), Backend::Vulkan),
            "vulkan"
        );
        // Too little GPU memory: the CPU from RAM.
        assert_eq!(runs_on(entry, &windows(24, 8)), Some(RunsOn::Cpu));
        assert_eq!(device_for(entry, &windows(24, 8), Backend::Vulkan), "cpu");
        // Neither, and the RAM it asks for is what is reported.
        assert_eq!(runs_on(entry, &windows(8, 8)), None);
        assert_eq!(
            blocked_reason(entry, &windows(8, 8)),
            Some(UserError::NeedsMemory {
                model: "nemotron-3.5-speaker".to_owned(),
                required: 20,
                available: 8,
                kind: MemoryKind::Ram,
            })
        );
        // GPU memory enough but no RAM beside it.
        assert_eq!(runs_on(entry, &windows(2, 24)), None);
    }

    #[test]
    fn a_mac_has_one_memory_for_both() {
        let entry = &ModelEntry {
            min_ram_gib: 20,
            min_gpu_memory_gib: 20,
            min_ram_with_gpu_gib: 4,
            ..*find_entry("nemotron-3.5-speaker").unwrap()
        };
        assert_eq!(mac(32).gpu_memory_gib(), Some(32));
        assert_eq!(runs_on(entry, &mac(32)), Some(RunsOn::Gpu));
        assert_eq!(device_for(entry, &mac(32), Backend::Default), "auto");
        assert_eq!(runs_on(entry, &mac(16)), None);
        assert_eq!(
            blocked_reason(entry, &mac(16)),
            Some(UserError::NeedsMemory {
                model: "nemotron-3.5-speaker".to_owned(),
                required: 20,
                available: 16,
                kind: MemoryKind::Unified,
            })
        );
        let cpu_only = find_entry("mossformer2-ss-16k").unwrap();
        assert_eq!(runs_on(cpu_only, &mac(32)), Some(RunsOn::Cpu));
        assert_eq!(device_for(cpu_only, &mac(32), Backend::Default), "cpu");
    }

    #[test]
    fn every_model_a_job_for_several_people_needs_is_recommended() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/catalog-low-memory");
        let paths = AppPaths {
            data: root.join("data"),
            resources: root.join("resources"),
            hf_hub: None,
        };
        let recommended: Vec<&str> = statuses(
            &Hardware {
                is_mac: false,
                memory_gib: 6,
                gpu: None,
            },
            &paths,
            &[],
        )
        .iter()
        .filter(|row| row.recommended)
        .map(|row| row.id)
        .collect();
        assert_eq!(
            recommended,
            vec![
                "whisper-large-v3-turbo",
                "nemotron-3.5-speaker",
                "mossformer2-ss-16k",
                "yunet"
            ]
        );
    }
}
