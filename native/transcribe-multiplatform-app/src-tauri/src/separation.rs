//! MossFormer2 through ONNX Runtime, in a process of its own: the runtime is loaded from the bundle at
//! run time, a crash or the model's gigabytes stay out of the app, and cancelling kills the process.

use crate::catalog::RunsOn;
use crate::error::{io_context, AppError, AppResult};
use crate::paths::{write_atomic, AppPaths};
use crate::process::{background_command, run_with_progress, CancelToken};
use ort::environment::Environment;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The app run with this flag and a request file separates the clips listed there.
pub const SEPARATE_FLAG: &str = "--separate-voices";
const TELEMETRY_OFF_VARIABLE: &str = "ORT_DISABLE_TELEMETRY";
const TOOL_NAME: &str = "voice separation";
const LIBRARY_FOLDER: &str = "onnxruntime";
const LIBRARY_FILE: &str = if cfg!(windows) {
    "onnxruntime.dll"
} else {
    "libonnxruntime.dylib"
};
const INPUT_NAME: &str = "wave";
const OUTPUT_NAME: &str = "tracks";
const TRACK_COUNT: usize = 2;
const SAMPLE_RATE: u32 = 16_000;
const FULL_SCALE: f32 = 32_768.0;
const SILENCE_FLOOR: f32 = 1e-12;
const WAV_FORMAT_PCM: u16 = 1;
const WAV_FORMAT_FLOAT: u16 = 3;
const WAV_HEADER_BYTES: u32 = 44;
const RIFF_HEADER_BYTES: usize = 12;
const CHUNK_HEADER_BYTES: usize = 8;
const FORMAT_CHUNK_BYTES: usize = 16;
const REQUEST_FILE: &str = "separation-request.json";
/// How the child process reports clips done on its standard error.
const PROGRESS_PREFIX: &str = "separated ";

/// Cuts `[start, end)` seconds of a 16 kHz mono WAV into files of their own, in the same form of samples.
pub fn cut_clips(wav: &Path, clips: &[(f64, f64, PathBuf)]) -> AppResult<()> {
    let context = || wav.display().to_string();
    let mut source = File::open(wav).map_err(io_context(format!("read {}", wav.display())))?;
    let layout = wav_layout(&mut source).map_err(|error| invalid(&context(), error))?;
    clips.iter().try_for_each(|(start, end, file)| {
        let (first, bytes) = layout.slice(*start, *end);
        let mut raw = vec![0; bytes as usize];
        source
            .seek(SeekFrom::Start(first))
            .and_then(|_| source.read_exact(&mut raw))
            .map_err(io_context(format!("read {}", wav.display())))?;
        write_atomic(file, &write_float_wav(&decode_samples(&raw, layout.float)))
    })
}

/// Copies `[start, end)` seconds into a file of its own as they are, without holding them in memory.
pub fn copy_span(wav: &Path, span: (f64, f64), target: &Path) -> AppResult<()> {
    let mut source = File::open(wav).map_err(io_context(format!("read {}", wav.display())))?;
    let layout =
        wav_layout(&mut source).map_err(|error| invalid(&wav.display().to_string(), error))?;
    let (first, bytes) = layout.slice(span.0, span.1);
    let mut out =
        File::create(target).map_err(io_context(format!("write {}", target.display())))?;
    let header = u32::try_from(bytes).map_err(tool_failure)?;
    out.write_all(&wav_head(header, layout.float))
        .and_then(|()| source.seek(SeekFrom::Start(first)))
        .and_then(|_| std::io::copy(&mut Read::take(&mut source, bytes), &mut out))
        .map(|_| ())
        .map_err(io_context(format!("write {}", target.display())))
}

/// One 16 kHz mono clip and where its two tracks go; the order of the tracks means nothing.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SeparationClip {
    pub input: PathBuf,
    pub outputs: [PathBuf; TRACK_COUNT],
}

#[derive(Debug, Serialize, Deserialize)]
struct SeparationRequest {
    library: PathBuf,
    model: PathBuf,
    runs_on: RunsOn,
    clips: Vec<SeparationClip>,
}

pub(crate) fn invalid(context: &str, error: impl std::fmt::Display) -> AppError {
    AppError::Invalid(format!("{context}: {error}"))
}

fn tool_failure(error: impl std::fmt::Display) -> AppError {
    invalid(TOOL_NAME, error)
}

fn setup_failure(error: impl std::fmt::Display) -> AppError {
    invalid("set up MossFormer2", error)
}

fn message(error: impl ToString) -> String {
    error.to_string()
}

/// The ONNX Runtime library in the bundle.
pub fn runtime_library(paths: &AppPaths) -> AppResult<PathBuf> {
    let library = paths
        .bundled_runtime_dir()
        .join(LIBRARY_FOLDER)
        .join(LIBRARY_FILE);
    if library.is_file() {
        return Ok(library);
    }
    Err(AppError::Invalid(format!(
        "{} is missing",
        library.display()
    )))
}

/// How MossFormer2 runs: the child process, its runtime and model, and on what.
pub struct SeparationRun<'a> {
    /// Marks the app re-invoked as its own child; a runner of its own takes only the request.
    pub flag: Option<&'a str>,
    pub exe: &'a Path,
    pub library: &'a Path,
    pub model: &'a Path,
    pub runs_on: RunsOn,
    pub work: &'a Path,
    pub cancel: &'a CancelToken,
}

/// Separates `clips` in one child process, so the model is loaded once and its memory goes with it.
pub fn separate_clips(
    run: &SeparationRun<'_>,
    clips: &[SeparationClip],
    progress: &mut dyn FnMut(f64),
) -> AppResult<()> {
    let request = SeparationRequest {
        library: run.library.to_path_buf(),
        model: run.model.to_path_buf(),
        runs_on: run.runs_on,
        clips: clips.to_vec(),
    };
    let file = run.work.join(REQUEST_FILE);
    let text = serde_json::to_vec(&request).map_err(tool_failure)?;
    write_atomic(&file, &text)?;
    let mut command = onnx_runtime_command(run.exe);
    command.args(run.flag).arg(&file);
    // Windows looks for the library's Visual C++ runtime on the path; it sits beside the library.
    if let Some(folder) = run.library.parent() {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let searched =
            std::iter::once(folder.to_path_buf()).chain(std::env::split_paths(&inherited));
        let path = std::env::join_paths(searched).map_err(tool_failure)?;
        command.env("PATH", path);
    }
    run_with_progress(command, TOOL_NAME, run.cancel, |line| {
        if let Some(done) = line.strip_prefix(PROGRESS_PREFIX).and_then(parse_share) {
            progress(done);
        }
    })
}

fn parse_share(text: &str) -> Option<f64> {
    let (done, total) = text.split_once('/')?;
    Some(done.parse::<f64>().ok()? / total.parse::<f64>().ok()?)
}

/// A child that loads ONNX Runtime. On macOS `with_telemetry(false)` still opens the runtime's event store
/// under Application Support and flushes it at exit, where a child that had finished was seen to hang; only
/// this variable keeps that code from starting.
fn onnx_runtime_command(exe: &Path) -> Command {
    let mut command = background_command(exe);
    command.env(TELEMETRY_OFF_VARIABLE, "1");
    command
}

/// ONNX Runtime allows one environment per process, loaded from the first library asked for.
pub(crate) fn environment(library: &Path) -> AppResult<&'static Environment> {
    static ENVIRONMENT: OnceLock<Result<Environment, String>> = OnceLock::new();
    ENVIRONMENT
        .get_or_init(|| {
            ort::init_from(library)
                .map_err(message)?
                // People's speech passes through here; nothing about it may leave the machine.
                .with_telemetry(false)
                .build()
                .map_err(message)
        })
        .as_ref()
        .map_err(|error| invalid("load ONNX Runtime", error))
}

/// What the child process does with `--separate-voices REQUEST`.
pub fn run_request(file: &Path) -> AppResult<()> {
    let text = std::fs::read(file).map_err(io_context(format!("read {}", file.display())))?;
    let request: SeparationRequest =
        serde_json::from_slice(&text).map_err(|error| invalid("separation request", error))?;
    // shortcut: no GPU provider of ONNX Runtime has run this model yet; the catalog's `min_gpu_memory_gib`
    // of 0 keeps it on the CPU, and a provider registered here is all a GPU run would need.
    let RunsOn::Cpu = request.runs_on else {
        return Err(tool_failure("no GPU provider is available"));
    };
    let mut builder = Session::builder(environment(&request.library)?)
        .map_err(setup_failure)?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(setup_failure)?
        // Same output and speed; an 8 s range peaks at 1.65 GB instead of 2.4 GB.
        .with_memory_pattern(false)
        .map_err(setup_failure)?;
    let mut session = builder
        .commit_from_file(&request.model)
        .map_err(|error| invalid("load MossFormer2", error))?;
    // In order: a clip may be a track an earlier one wrote.
    let total = request.clips.len();
    request
        .clips
        .iter()
        .enumerate()
        .try_for_each(|(done, clip)| {
            separate_clip(&mut session, clip)?;
            eprintln!("{PROGRESS_PREFIX}{}/{total}", done + 1);
            Ok(())
        })
}

fn separate_clip(session: &mut Session, clip: &SeparationClip) -> AppResult<()> {
    let bytes =
        std::fs::read(&clip.input).map_err(io_context(format!("read {}", clip.input.display())))?;
    let wave =
        read_wav(&bytes).map_err(|error| invalid(&clip.input.display().to_string(), error))?;
    let samples = wave.len();
    let tracks = run_model(session, wave.clone())?;
    let level = rms(&wave);
    clip.outputs
        .iter()
        .zip(tracks)
        .try_for_each(|(output, track)| {
            write_atomic(output, &write_float_wav(&at_level(track, samples, level)))
        })
}

fn run_model(session: &mut Session, wave: Vec<f32>) -> AppResult<Vec<Vec<f32>>> {
    let shape = [1, wave.len()];
    let input = Tensor::from_array((shape, wave)).map_err(tool_failure)?;
    let outputs = session
        .run(ort::inputs![INPUT_NAME => input])
        .map_err(tool_failure)?;
    let value = outputs
        .get(OUTPUT_NAME)
        .ok_or_else(|| tool_failure("the model gave no tracks"))?;
    let (shape, data) = value.try_extract_tensor::<f32>().map_err(tool_failure)?;
    split_tracks(shape, data)
}

/// `[2, 1, samples]` as one list per track; anything else is not this model.
fn split_tracks(shape: &[i64], data: &[f32]) -> AppResult<Vec<Vec<f32>>> {
    let [tracks, 1, samples] = shape else {
        return Err(invalid(
            TOOL_NAME,
            format!("unexpected output shape {shape:?}"),
        ));
    };
    let samples = *samples as usize;
    if *tracks != TRACK_COUNT as i64 || samples == 0 {
        return Err(invalid(
            TOOL_NAME,
            format!("unexpected output shape {shape:?}"),
        ));
    }
    Ok(data.chunks(samples).map(<[f32]>::to_vec).collect())
}

fn rms(samples: &[f32]) -> f32 {
    let energy: f32 = samples.iter().map(|sample| sample * sample).sum();
    (energy / samples.len().max(1) as f32).sqrt()
}

/// As long as the input and at its level, which the exported graph leaves to the caller.
fn at_level(mut track: Vec<f32>, samples: usize, level: f32) -> Vec<f32> {
    track.resize(samples, 0.0);
    let gain = level / (rms(&track) + SILENCE_FLOOR);
    track.iter_mut().for_each(|sample| *sample *= gain);
    track
}

fn little_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn little_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn decode_samples(bytes: &[u8], float: bool) -> Vec<f32> {
    if float {
        return bytes
            .chunks_exact(4)
            .map(|four| f32::from_le_bytes([four[0], four[1], four[2], four[3]]))
            .collect();
    }
    bytes
        .chunks_exact(2)
        .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / FULL_SCALE)
        .collect()
}

/// Where the samples of a 16 kHz mono WAV are in its file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WavLayout {
    pub data_start: u64,
    pub data_bytes: u64,
    pub float: bool,
}

impl WavLayout {
    /// The first byte and the length of `[start, end)` seconds, inside the file and on whole samples.
    pub(crate) fn slice(&self, start: f64, end: f64) -> (u64, u64) {
        let sample_bytes = if self.float { 4 } else { 2 };
        let at = |seconds: f64| {
            let sample = (seconds * f64::from(SAMPLE_RATE)).round().max(0.0) as u64;
            (sample * sample_bytes).min(self.data_bytes / sample_bytes * sample_bytes)
        };
        (
            self.data_start + at(start),
            at(end).saturating_sub(at(start)),
        )
    }

    pub(crate) fn seconds(&self) -> f64 {
        let sample_bytes = if self.float { 4 } else { 2 };
        (self.data_bytes / sample_bytes) as f64 / f64::from(SAMPLE_RATE)
    }
}

/// Reads only the headers, so an hour of audio stays on the disk: 16-bit as `extract_audio` writes
/// it, or float as a track is written.
pub(crate) fn wav_layout(reader: &mut (impl Read + Seek)) -> Result<WavLayout, String> {
    let length = reader.seek(SeekFrom::End(0)).map_err(message)?;
    let mut riff = [0_u8; RIFF_HEADER_BYTES];
    reader.seek(SeekFrom::Start(0)).map_err(message)?;
    if reader.read_exact(&mut riff).is_err() || &riff[..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        return Err("not a WAV file".to_owned());
    }
    let mut at = RIFF_HEADER_BYTES as u64;
    let mut float = None;
    let mut chunk = [0_u8; CHUNK_HEADER_BYTES];
    while at + CHUNK_HEADER_BYTES as u64 <= length {
        reader.seek(SeekFrom::Start(at)).map_err(message)?;
        reader.read_exact(&mut chunk).map_err(message)?;
        let size = u64::from(little_u32(&chunk, 4));
        let body = at + CHUNK_HEADER_BYTES as u64;
        let available = size.min(length - body);
        if &chunk[..4] == b"fmt " && available >= FORMAT_CHUNK_BYTES as u64 {
            let mut format = [0_u8; FORMAT_CHUNK_BYTES];
            reader.read_exact(&mut format).map_err(message)?;
            float = Some(format_is_float(&format)?);
        } else if let (b"data", Some(float)) = (&chunk[..4], float) {
            return Ok(WavLayout {
                data_start: body,
                data_bytes: available,
                float,
            });
        }
        // Chunks are padded to an even size.
        at = body + size + (size & 1);
    }
    Err("no audio in the WAV file".to_owned())
}

fn format_is_float(format: &[u8]) -> Result<bool, String> {
    let form = (little_u16(format, 0), little_u16(format, 14));
    let wanted = (form == (WAV_FORMAT_PCM, 16) || form == (WAV_FORMAT_FLOAT, 32))
        && little_u16(format, 2) == 1
        && little_u32(format, 4) == SAMPLE_RATE;
    if !wanted {
        return Err("not 16 kHz mono audio".to_owned());
    }
    Ok(form.0 == WAV_FORMAT_FLOAT)
}

/// The samples of a 16 kHz mono WAV held in memory.
pub(crate) fn read_wav(bytes: &[u8]) -> Result<Vec<f32>, String> {
    let layout = wav_layout(&mut Cursor::new(bytes))?;
    let start = layout.data_start as usize;
    Ok(decode_samples(
        &bytes[start..start + layout.data_bytes as usize],
        layout.float,
    ))
}

/// The header of a 16 kHz mono WAV holding `data_bytes` of samples.
pub(crate) fn wav_head(data_bytes: u32, float: bool) -> Vec<u8> {
    let (form, width) = if float {
        (WAV_FORMAT_FLOAT, 4_u16)
    } else {
        (WAV_FORMAT_PCM, 2_u16)
    };
    let mut out = Vec::with_capacity(WAV_HEADER_BYTES as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(WAV_HEADER_BYTES - 8 + data_bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&(FORMAT_CHUNK_BYTES as u32).to_le_bytes());
    out.extend_from_slice(&form.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * u32::from(width)).to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&(width * 8).to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    out
}

/// 32-bit float: restoring the level can take a quiet track past full scale, where 16 bits would clip it.
fn write_float_wav(samples: &[f32]) -> Vec<u8> {
    let data_bytes = samples.len() as u32 * 4;
    let mut out = Vec::with_capacity((WAV_HEADER_BYTES + data_bytes) as usize);
    out.extend_from_slice(&wav_head(data_bytes, true));
    samples
        .iter()
        .for_each(|sample| out.extend_from_slice(&sample.to_le_bytes()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// `tracks` = the wave, and the wave at half level, as `[2, 1, samples]`.
    const TWO_TRACKS_MODEL: &str = "08083ab5010a1a0a04776176650a04617865731201612209556e73717565657a650a110a01610a0468616c6612016222034d756c0a230a01610a01621206747261636b732206436f6e6361742a0b0a04617869731800a001021201672a14080110074204617865734a0800000000000000002a0e1001420468616c664a040000003f5a170a0477617665120f0a0d080112090a0208010a0312016e621d0a06747261636b7312130a110801120d0a0208020a0208010a0312016e42040a001011";
    /// `tracks` = the wave itself, as `[1, samples]`: not the shape MossFormer2 gives.
    const WRONG_SHAPE_MODEL: &str = "08083a510a180a04776176651206747261636b7322084964656e746974791201675a170a0477617665120f0a0d080112090a0208010a0312016e62190a06747261636b73120f0a0d080112090a0208010a0312016e42040a001011";

    fn write_wav(samples: &[i16]) -> Vec<u8> {
        let mut out = write_float_wav(&[]);
        out[20] = WAV_FORMAT_PCM as u8;
        out[32] = 2;
        out[34] = 16;
        out[40..44].copy_from_slice(&(samples.len() as u32 * 2).to_le_bytes());
        samples
            .iter()
            .for_each(|sample| out.extend_from_slice(&sample.to_le_bytes()));
        out
    }

    fn read_float_wav(file: &Path) -> Vec<f32> {
        let bytes = std::fs::read(file).unwrap();
        assert_eq!(little_u16(&bytes, 20), WAV_FORMAT_FLOAT);
        bytes[WAV_HEADER_BYTES as usize..]
            .chunks_exact(4)
            .map(|four| f32::from_le_bytes([four[0], four[1], four[2], four[3]]))
            .collect()
    }

    fn scratch(name: &str) -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target-test")
            .join(format!("separation-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn from_hex(text: &str) -> Vec<u8> {
        (0..text.len() / 2)
            .map(|at| u8::from_str_radix(&text[at * 2..at * 2 + 2], 16).unwrap())
            .collect()
    }

    /// The library `fetch-runtime.sh` puts in the development runtime.
    fn library() -> PathBuf {
        runtime_library(&AppPaths {
            data: PathBuf::new(),
            resources: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            hf_hub: None,
        })
        .expect("scripts/release/fetch-runtime.sh has not been run")
    }

    fn clip_in(root: &Path, samples: &[i16]) -> SeparationClip {
        let input = root.join("clip.wav");
        std::fs::write(&input, write_wav(samples)).unwrap();
        SeparationClip {
            input,
            outputs: [root.join("a.wav"), root.join("b.wav")],
        }
    }

    fn request_file(root: &Path, model: &str, clips: Vec<SeparationClip>) -> PathBuf {
        let model_file = root.join("model.onnx");
        std::fs::write(&model_file, from_hex(model)).unwrap();
        let file = root.join("request.json");
        let request = SeparationRequest {
            library: library(),
            model: model_file,
            runs_on: RunsOn::Cpu,
            clips,
        };
        std::fs::write(&file, serde_json::to_vec(&request).unwrap()).unwrap();
        file
    }

    fn rounded(samples: &[f32]) -> Vec<i32> {
        samples
            .iter()
            .map(|sample| (sample * 1000.0).round() as i32)
            .collect()
    }

    #[test]
    fn the_model_gives_two_tracks_each_at_the_level_of_the_input() {
        let root = scratch("run");
        let clip = clip_in(&root, &[8192, -8192, 16384, -16384]);
        let request = request_file(&root, TWO_TRACKS_MODEL, vec![clip.clone()]);
        run_request(&request).unwrap();
        let tracks = clip.outputs.map(|output| read_float_wav(&output));
        // The second track came out at half level and goes back up to the input's.
        assert_eq!(rounded(&tracks[0]), vec![250, -250, 500, -500]);
        assert_eq!(rounded(&tracks[1]), vec![250, -250, 500, -500]);
    }

    #[test]
    fn a_model_of_another_shape_is_an_error_and_writes_nothing() {
        let root = scratch("shape");
        let clip = clip_in(&root, &[8192, -8192]);
        let request = request_file(&root, WRONG_SHAPE_MODEL, vec![clip.clone()]);
        assert_eq!(
            run_request(&request).unwrap_err().to_string(),
            "voice separation: unexpected output shape [1, 2]"
        );
        assert!(!clip.outputs[0].exists());
    }

    #[test]
    fn a_gpu_run_is_refused_until_a_provider_exists() {
        let root = scratch("gpu");
        let clip = clip_in(&root, &[8192]);
        let request = request_file(&root, TWO_TRACKS_MODEL, vec![clip]);
        let text = std::fs::read_to_string(&request)
            .unwrap()
            .replace("\"cpu\"", "\"gpu\"");
        std::fs::write(&request, text).unwrap();
        assert_eq!(
            run_request(&request).unwrap_err().to_string(),
            "voice separation: no GPU provider is available"
        );
    }

    #[test]
    fn a_broken_model_file_is_an_error() {
        let root = scratch("broken-model");
        let clip = clip_in(&root, &[8192]);
        let request = request_file(&root, "00ff", vec![clip]);
        assert!(run_request(&request)
            .unwrap_err()
            .to_string()
            .starts_with("load MossFormer2:"));
    }

    #[test]
    fn a_clip_that_cannot_be_read_is_an_error() {
        let root = scratch("clip");
        let clip = SeparationClip {
            input: root.join("missing.wav"),
            outputs: [root.join("a.wav"), root.join("b.wav")],
        };
        let request = request_file(&root, TWO_TRACKS_MODEL, vec![clip]);
        assert!(run_request(&request)
            .unwrap_err()
            .to_string()
            .starts_with("read "));
        let bad = root.join("bad.wav");
        std::fs::write(&bad, b"not audio").unwrap();
        let clip = SeparationClip {
            input: bad,
            outputs: [root.join("a.wav"), root.join("b.wav")],
        };
        let request = request_file(&root, TWO_TRACKS_MODEL, vec![clip]);
        assert!(run_request(&request)
            .unwrap_err()
            .to_string()
            .ends_with("not a WAV file"));
    }

    #[test]
    fn a_request_that_cannot_be_read_is_an_error() {
        let root = scratch("request");
        assert!(run_request(&root.join("missing.json"))
            .unwrap_err()
            .to_string()
            .starts_with("read "));
        let file = root.join("bad.json");
        std::fs::write(&file, b"{").unwrap();
        assert!(run_request(&file)
            .unwrap_err()
            .to_string()
            .starts_with("separation request:"));
    }

    #[test]
    fn only_the_expected_shape_splits_into_tracks() {
        assert_eq!(
            split_tracks(&[2, 1, 2], &[1.0, 2.0, 3.0, 4.0]).unwrap(),
            vec![vec![1.0, 2.0], vec![3.0, 4.0]]
        );
        assert!(split_tracks(&[3, 1, 1], &[1.0, 2.0, 3.0]).is_err());
        assert!(split_tracks(&[2, 1, 0], &[]).is_err());
        assert!(split_tracks(&[2, 2], &[1.0, 2.0, 3.0, 4.0]).is_err());
    }

    #[test]
    fn a_track_takes_the_length_and_level_of_the_input_and_silence_stays_silent() {
        assert_eq!(
            rounded(&at_level(vec![1.0, 1.0, 9.0], 2, 0.5)),
            vec![500, 500]
        );
        assert_eq!(rounded(&at_level(vec![1.0], 2, 0.5)), vec![707, 0]);
        assert_eq!(at_level(vec![0.0, 0.0], 2, 0.5), vec![0.0, 0.0]);
        assert_eq!(rms(&[]), 0.0);
    }

    #[test]
    fn a_wav_reads_back_and_a_track_past_full_scale_is_not_clipped() {
        assert_eq!(
            read_wav(&write_wav(&[16384, -16384, 32767, -32768])).unwrap(),
            vec![0.5, -0.5, 32_767.0 / 32_768.0, -1.0]
        );
        let root = scratch("loud");
        let file = root.join("loud.wav");
        std::fs::write(&file, write_float_wav(&[2.0, -2.0])).unwrap();
        assert_eq!(read_float_wav(&file), vec![2.0, -2.0]);
    }

    #[test]
    fn a_track_reads_back_as_it_was_written_for_a_second_pass() {
        assert_eq!(
            read_wav(&write_float_wav(&[2.0, -0.5])).unwrap(),
            vec![2.0, -0.5]
        );
    }

    #[test]
    fn clips_are_cut_at_their_seconds_and_stop_at_the_end_of_the_audio() {
        let root = scratch("cut");
        let wav = root.join("audio.wav");
        std::fs::write(&wav, write_wav(&vec![16384; 16_000])).unwrap();
        let (first, last) = (root.join("first.wav"), root.join("last.wav"));
        cut_clips(
            &wav,
            &[(0.0, 0.25, first.clone()), (0.5, 9.0, last.clone())],
        )
        .unwrap();
        assert_eq!(read_float_wav(&first).len(), 4000);
        assert_eq!(read_float_wav(&last).len(), 8000);
        assert!(cut_clips(&root.join("none.wav"), &[]).is_err());
        std::fs::write(&wav, b"nope").unwrap();
        assert!(cut_clips(&wav, &[]).is_err());
    }

    #[test]
    fn wavs_of_another_form_are_refused() {
        let good = write_wav(&[16384]);
        assert_eq!(read_wav(b"RIFF").unwrap_err(), "not a WAV file");
        let mut stereo = good.clone();
        stereo[22] = 2;
        assert_eq!(read_wav(&stereo).unwrap_err(), "not 16 kHz mono audio");
        let mut rate = good.clone();
        rate[24] = 0;
        assert_eq!(read_wav(&rate).unwrap_err(), "not 16 kHz mono audio");
        let mut float = good.clone();
        float[20] = 3;
        assert_eq!(read_wav(&float).unwrap_err(), "not 16 kHz mono audio");
        let mut depth = good.clone();
        depth[34] = 24;
        assert_eq!(read_wav(&depth).unwrap_err(), "not 16 kHz mono audio");
        assert_eq!(
            read_wav(&good[..36]).unwrap_err(),
            "no audio in the WAV file"
        );
    }

    #[test]
    fn extra_chunks_are_passed_over_with_their_padding() {
        let good = write_wav(&[16384]);
        let mut with_list = good[..36].to_vec();
        with_list.extend_from_slice(b"LIST");
        with_list.extend_from_slice(&3u32.to_le_bytes());
        with_list.extend_from_slice(b"abc\0");
        with_list.extend_from_slice(&good[36..]);
        assert_eq!(read_wav(&with_list).unwrap(), vec![0.5]);
    }

    #[test]
    fn the_child_process_gets_the_request_and_finds_the_library_on_its_path() {
        let root = scratch("child");
        let exe = root.join("child.sh");
        std::fs::write(
            &exe,
            "#!/bin/sh\necho \"$1\" > \"$2.flag\"\necho \"$PATH\" > \"$2.path\"\necho \"$ORT_DISABLE_TELEMETRY\" > \"$2.telemetry\"\necho 'separated 1/2' >&2\n",
        )
        .unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let clips = [clip_in(&root, &[16384])];
        separate_clips(
            &SeparationRun {
                flag: Some(SEPARATE_FLAG),
                exe: &exe,
                library: &root.join("lib/onnxruntime"),
                model: &root.join("model.onnx"),
                runs_on: RunsOn::Cpu,
                work: &root,
                cancel: &CancelToken::default(),
            },
            &clips,
            &mut |_| {},
        )
        .unwrap();
        let request = root.join(REQUEST_FILE);
        let saved: SeparationRequest =
            serde_json::from_slice(&std::fs::read(&request).unwrap()).unwrap();
        assert_eq!(saved.clips, clips);
        assert_eq!(saved.model, root.join("model.onnx"));
        let told = |extension: &str| {
            std::fs::read_to_string(format!("{}.{extension}", request.display())).unwrap()
        };
        assert_eq!(told("flag").trim(), SEPARATE_FLAG);
        assert_eq!(told("telemetry").trim(), "1");
        assert!(told("path").starts_with(root.join("lib").to_str().unwrap()));
    }

    #[test]
    fn progress_lines_from_the_child_become_shares() {
        assert_eq!(parse_share("1/4"), Some(0.25));
        assert_eq!(parse_share("4"), None);
        assert_eq!(parse_share("a/4"), None);
        assert_eq!(parse_share("1/b"), None);
    }

    #[test]
    fn the_runtime_library_must_be_in_the_bundle() {
        let root = scratch("library");
        let paths = AppPaths {
            data: root.join("data"),
            resources: root.clone(),
            hf_hub: None,
        };
        assert!(runtime_library(&paths)
            .unwrap_err()
            .to_string()
            .ends_with("is missing"));
    }

    #[test]
    fn a_span_is_copied_as_it_is() {
        let root = scratch("copy-span");
        let wav = root.join("a.wav");
        let samples: Vec<i16> = (0..1600).collect();
        std::fs::write(&wav, write_wav(&samples)).unwrap();
        let target = root.join("span.wav");
        copy_span(&wav, (0.01, 0.02), &target).unwrap();
        let copied = std::fs::read(&target).unwrap();
        assert_eq!(read_wav(&copied).unwrap().len(), 160);
        assert_eq!(copied[44..46], 160_i16.to_le_bytes());
        assert_eq!(copied[copied.len() - 2..], 319_i16.to_le_bytes());
    }

    #[test]
    fn a_span_of_something_unreadable_is_an_error() {
        let root = scratch("copy-span-bad");
        let missing = copy_span(&root.join("absent"), (0.0, 1.0), &root.join("o"));
        assert!(missing.unwrap_err().to_string().starts_with("read "));
        std::fs::write(root.join("bad.wav"), b"not a wav").unwrap();
        let bad = copy_span(&root.join("bad.wav"), (0.0, 1.0), &root.join("o"));
        assert!(bad.is_err());
    }
}
