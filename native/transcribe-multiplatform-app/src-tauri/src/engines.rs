//! Drives the bundled tools: ffmpeg/ffprobe for media, whisper.cpp for words, nemo-speech for speakers.

use crate::captions::{SpeakerTurn, VoiceSpan, Word};
use crate::catalog::{Backend, ModelEntry, Role};
use crate::error::{io_context, AppError, AppResult, UserError};
use crate::paths::AppPaths;
use crate::process::{
    background_command, run_capture, run_with_progress, run_with_ticks, CancelToken,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::ffi::OsStr;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const TOOL_SEARCH_DEPTH: usize = 3;
const EXE_SUFFIX: &str = if cfg!(windows) { ".exe" } else { "" };
const LOOPBACK: &str = "127.0.0.1";
const SERVER_POLL: Duration = Duration::from_millis(100);
/// Loading about 20 GB from a slow disk can take minutes; past this the server is stuck.
const SERVER_READY_TIMEOUT: Duration = Duration::from_secs(1800);
/// The most `serve` accepts; an hour of audio takes minutes even on a CPU.
const SERVER_SOCKET_SECONDS: &str = "3600";
const UPLOAD_BOUNDARY: &str = "transcribe-captions-3f7c1e9a5b2d4860a1c7e3f95d0b6824";
/// More than any real answer, and the bound on what is held in memory.
const MOST_SEGMENT_BYTES: u64 = 256 * 1024 * 1024;
const AUDIO_CONTENT_TYPE: &str = "audio/wav";
const HTTP_OK: u16 = 200;
const AUDIO_ARGS: [&str; 8] = ["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le", "-y"];
const MILLIS_PER_SECOND: f64 = 1000.0;
/// Dynamic time warping over the decoder's attention gives each token a time of its own (mean
/// error 0.22 s against 0.52 s for plain token timestamps, measured against Nemotron). Stock
/// whisper.cpp needs flash attention off for it, which makes the text worse (similarity to
/// Nemotron's 0.82 against 0.87); the bundled build is patched to warp with flash attention on
/// (`scripts/release/whisper-cpp.patch`), so one pass gives both.
const TOKEN_TIMING_ARGS: [&str; 2] = ["-dtw", "large.v3.turbo"];
/// Files per whisper-cli run, as two long paths each must stay under Windows's 32 KB command line.
const WHISPER_BATCH_FILES: usize = 64;
/// Speech stays above about five syllables a second: a word after a pause started at most this
/// long per beat before its end.
const LONGEST_ONSET_SECONDS_PER_CHARACTER: f64 = 0.2;
const KANJI_BEATS: usize = 2;
/// Words on each side that tell how fast the speech around a word is.
const LOCAL_RATE_WORDS: usize = 5;
/// `t_dtw` counts in hundredths of a second.
const DTW_TICKS_PER_SECOND: f64 = 100.0;
const CENTISECONDS_PER_SECOND: f64 = 100.0;
const VOICE_MODEL: &str = "ggml-silero-v6.2.0.bin";
/// A word timed by warping has only a start; it is said in at most this much per character, so the
/// last word before a pause does not last through the pause.
const LONGEST_SPOKEN_CHARACTER_SECONDS: f64 = 0.25;
/// Segment bounds are coarser than warping: without this margin words piled up at a bound (24 of
/// 130 starts over 0.2 s early against Nemotron, none with it).
const SEGMENT_MARGIN_SECONDS: f64 = 1.0;
const FALLBACK_FPS: (u32, u32) = (30, 1);
const QUARTER_TURN_DEGREES: i64 = 90;

/// Absolute paths of the tools one job uses.
#[derive(Clone, Debug)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub whisper: PathBuf,
    /// The CPU build to fall back on when `whisper` is a GPU build and it fails.
    pub whisper_cpu: Option<PathBuf>,
    /// Always the CPU build: Silero needs no GPU, and it takes a fraction of a percent of the audio's length.
    pub voice_detector: PathBuf,
    pub nemo: PathBuf,
    /// The build `nemo` and `whisper` actually came from.
    pub backend: Backend,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub duration: f64,
    pub fps_num: u32,
    pub fps_den: u32,
    /// `None` when the source did not say; a transcript from an older version also has none.
    #[serde(default)]
    pub audio: Option<AudioInfo>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct AudioInfo {
    pub sample_rate: u32,
    pub channels: u32,
    /// The audio stream's own seconds, which can differ a little from the video's.
    pub duration: f64,
}

fn exe_name(tool: &str) -> String {
    format!("{tool}{EXE_SUFFIX}")
}

fn find_tool(root: &Path, file_name: &str, depth: usize) -> Option<PathBuf> {
    let candidate = root.join(file_name);
    if candidate.is_file() {
        return Some(candidate);
    }
    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(root).ok()?;
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .find_map(|path| find_tool(&path, file_name, depth - 1))
}

impl Tools {
    /// The builds for `backend`, or the bundled CPU ones where those are not installed.
    pub fn locate(paths: &AppPaths, backend: Backend) -> AppResult<Self> {
        let bundled = paths.bundled_runtime_dir();
        let nemo_root = match backend {
            Backend::Cuda => Some(paths.downloaded_runtime_dir().join("geforce/nemo")),
            Backend::Vulkan => Some(bundled.join("nemo-vulkan")),
            Backend::Default | Backend::SpeakersOnCpu => None,
        };
        let whisper_root = (backend != Backend::Default).then(|| bundled.join("whisper-vulkan"));
        let accelerated = |root: Option<PathBuf>, tool: &str| {
            root.and_then(|root| find_tool(&root, &exe_name(tool), TOOL_SEARCH_DEPTH))
        };
        let plain = |folder: &str, tool: &str| {
            find_tool(&bundled.join(folder), &exe_name(tool), TOOL_SEARCH_DEPTH).ok_or_else(|| {
                AppError::Invalid(format!("{tool} is missing from {}", bundled.display()))
            })
        };
        let nemo = accelerated(nemo_root, "nemo-speech");
        let whisper = accelerated(whisper_root, "whisper-cli");
        Ok(Self {
            backend: match (&nemo, &whisper) {
                (Some(_), _) => backend,
                (None, Some(_)) => Backend::SpeakersOnCpu,
                (None, None) => Backend::Default,
            },
            ffmpeg: plain("ffmpeg", "ffmpeg")?,
            ffprobe: plain("ffmpeg", "ffprobe")?,
            whisper_cpu: whisper
                .is_some()
                .then(|| plain("whisper", "whisper-cli"))
                .transpose()?,
            whisper: whisper.map_or_else(|| plain("whisper", "whisper-cli"), Ok)?,
            voice_detector: plain("whisper", "whisper-vad-speech-segments")?,
            nemo: nemo.map_or_else(|| plain("nemo", "nemo-speech"), Ok)?,
        })
    }
}

fn parse_rate(text: &str) -> Option<(u32, u32)> {
    let (num, den) = text.split_once('/')?;
    let (num, den) = (num.parse().ok()?, den.parse().ok()?);
    (num > 0 && den > 0).then_some((num, den))
}

fn rotation_degrees(stream: &Value) -> i64 {
    stream["side_data_list"]
        .as_array()
        .and_then(|list| list.iter().find_map(|item| item["rotation"].as_f64()))
        .or_else(|| {
            stream["tags"]["rotate"]
                .as_str()
                .and_then(|text| text.parse().ok())
        })
        .map_or(0, |degrees: f64| degrees.round() as i64)
}

fn audio_info(stream: &Value) -> Option<AudioInfo> {
    if stream["codec_type"] != "audio" {
        return None;
    }
    Some(AudioInfo {
        sample_rate: stream["sample_rate"].as_str()?.parse().ok()?,
        channels: u32::try_from(stream["channels"].as_u64()?).ok()?,
        duration: stream["duration"].as_str()?.parse().ok()?,
    })
}

/// Reads display size (rotation applied) and timing from ffprobe's JSON.
pub fn parse_probe(json: &[u8]) -> AppResult<VideoInfo> {
    let value: Value = serde_json::from_slice(json)
        .map_err(|error| AppError::Invalid(format!("ffprobe output: {error}")))?;
    let streams = value["streams"]
        .as_array()
        .ok_or_else(|| AppError::Invalid("ffprobe found no streams".into()))?;
    let video = streams
        .iter()
        .find(|stream| {
            stream["codec_type"] == "video" && stream["disposition"]["attached_pic"] != 1
        })
        .ok_or(UserError::NoVideoStream)?;
    if !streams.iter().any(|stream| stream["codec_type"] == "audio") {
        return Err(UserError::NoAudioStream.into());
    }
    let dimension = |name: &str| {
        video[name]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
    };
    let (Some(width), Some(height)) = (dimension("width"), dimension("height")) else {
        return Err(UserError::UnreadableSize.into());
    };
    let (width, height) = if rotation_degrees(video) % (2 * QUARTER_TURN_DEGREES) == 0 {
        (width, height)
    } else {
        (height, width)
    };
    let duration = value["format"]["duration"]
        .as_str()
        .and_then(|text| text.parse::<f64>().ok())
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .ok_or(UserError::UnreadableDuration)?;
    let rate = video["avg_frame_rate"]
        .as_str()
        .and_then(parse_rate)
        .or_else(|| video["r_frame_rate"].as_str().and_then(parse_rate));
    let (fps_num, fps_den) = rate.unwrap_or(FALLBACK_FPS);
    Ok(VideoInfo {
        width,
        height,
        duration,
        fps_num,
        fps_den,
        audio: streams.iter().find_map(audio_info),
    })
}

pub fn probe(tools: &Tools, video: &Path) -> AppResult<VideoInfo> {
    let mut command = background_command(&tools.ffprobe);
    command
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(video);
    parse_probe(&run_capture(command, "ffprobe")?)
}

/// `time=00:01:02.50` from an ffmpeg status line, in seconds.
fn parse_ffmpeg_time(line: &str) -> Option<f64> {
    let start = line.find("time=")? + "time=".len();
    let clock = line[start..].split_whitespace().next()?;
    let mut fields = clock.split(':').map(str::parse::<f64>);
    let (hours, minutes, seconds) = (
        fields.next()?.ok()?,
        fields.next()?.ok()?,
        fields.next()?.ok()?,
    );
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

/// Extracts 16 kHz mono PCM, the format both speech engines read.
pub fn extract_audio(
    tools: &Tools,
    video: &Path,
    wav: &Path,
    duration: f64,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f64),
) -> AppResult<()> {
    let mut command = background_command(&tools.ffmpeg);
    command
        .args(["-hide_banner", "-i"])
        .arg(video)
        .args(AUDIO_ARGS)
        .arg(wav);
    run_with_progress(command, "ffmpeg", cancel, |line| {
        if let Some(seconds) = parse_ffmpeg_time(line) {
            progress((seconds / duration).clamp(0.0, 1.0));
        }
    })
}

fn parse_whisper_progress(line: &str) -> Option<f64> {
    let (_, rest) = line.split_once("progress =")?;
    let percent: f64 = rest.trim().trim_end_matches('%').trim().parse().ok()?;
    Some((percent / 100.0).clamp(0.0, 1.0))
}

/// Latin-1 keeps every raw byte in a `char`, because whisper.cpp splits UTF-8 across tokens
/// and writes those fragments into its JSON unescaped.
fn latin1_string(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}

fn latin1_bytes(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
        .collect()
}

/// One whisper.cpp invocation's fixed inputs.
struct WhisperRun<'a> {
    model: &'a Path,
    language: &'a str,
    cancel: &'a CancelToken,
}

struct TokenTimes {
    start: f64,
    end: f64,
    warped: bool,
}

struct TokenMerger {
    bytes: Vec<u8>,
    start: f64,
    warped: bool,
}

impl TokenMerger {
    /// Returns a word once the buffered bytes form complete UTF-8.
    fn push(&mut self, text: &[u8], times: TokenTimes) -> Option<Word> {
        if self.bytes.is_empty() {
            self.start = times.start;
            self.warped = times.warped;
        }
        self.bytes.extend_from_slice(text);
        let decoded = std::str::from_utf8(&self.bytes).ok()?;
        let word = Word {
            start: self.start,
            end: if self.warped {
                self.start
            } else {
                times.end.max(self.start)
            },
            text: decoded.to_owned(),
            speaker: None,
        };
        self.bytes.clear();
        Some(word)
    }
}

fn is_control_token(text: &[u8]) -> bool {
    text.starts_with(b"[_") && text.ends_with(b"]")
}

/// Reads whisper.cpp's full JSON into timed words.
pub fn parse_whisper_json(bytes: &[u8]) -> AppResult<Vec<Word>> {
    let value: Value = serde_json::from_str(&latin1_string(bytes))
        .map_err(|error| AppError::Invalid(format!("whisper JSON: {error}")))?;
    let segments = value["transcription"]
        .as_array()
        .ok_or_else(|| AppError::Invalid("whisper JSON has no transcription".into()))?;
    let mut merger = TokenMerger {
        bytes: Vec::new(),
        start: 0.0,
        warped: false,
    };
    let mut words = Vec::new();
    for segment in segments {
        read_segment(segment, &mut merger, &mut words);
    }
    open_warped_words(&mut words);
    Ok(words)
}

fn read_segment(segment: &Value, merger: &mut TokenMerger, words: &mut Vec<Word>) {
    let bounds = segment_bounds(segment);
    let mut previous = None;
    for token in segment["tokens"].as_array().into_iter().flatten() {
        let text = latin1_bytes(token["text"].as_str().unwrap_or(""));
        if text.is_empty() || is_control_token(&text) {
            continue;
        }
        let times = token_times(token, bounds, previous);
        previous = Some(times.start);
        words.extend(merger.push(&text, times));
    }
}

fn segment_bounds(segment: &Value) -> Option<(f64, f64)> {
    let from = segment["offsets"]["from"].as_f64()?;
    let to = segment["offsets"]["to"].as_f64()?;
    Some((from / MILLIS_PER_SECOND, to / MILLIS_PER_SECOND))
}

/// Warping is reliable in and just around its segment; a segment's first and last tokens can land
/// seconds outside it (64 s for a token of a segment ending at 62.4 s), so they come back: the first
/// to the segment's start, the last to just after the token before it.
fn warped_start(warped: f64, bounds: Option<(f64, f64)>, previous: Option<f64>) -> f64 {
    let Some((from, to)) = bounds else {
        return warped;
    };
    if warped < from - SEGMENT_MARGIN_SECONDS {
        return from;
    }
    if warped <= to + SEGMENT_MARGIN_SECONDS {
        return warped;
    }
    previous.map_or(to, |earlier| {
        (earlier + LONGEST_SPOKEN_CHARACTER_SECONDS).min(to)
    })
}

fn token_times(token: &Value, bounds: Option<(f64, f64)>, previous: Option<f64>) -> TokenTimes {
    let from = token["offsets"]["from"].as_f64().unwrap_or(0.0) / MILLIS_PER_SECOND;
    let to = token["offsets"]["to"]
        .as_f64()
        .map_or(from, |to| to / MILLIS_PER_SECOND);
    match token["t_dtw"].as_f64().filter(|ticks| *ticks >= 0.0) {
        Some(ticks) => TokenTimes {
            start: warped_start(ticks / DTW_TICKS_PER_SECOND, bounds, previous),
            end: to,
            warped: true,
        },
        None => TokenTimes {
            start: from,
            end: to,
            warped: false,
        },
    }
}

/// About how many beats a text takes: a kanji is usually read in two (今日 is three), a small kana
/// or long mark adds none of its own to the character before it.
fn spoken_beats(text: &str) -> usize {
    text.chars()
        .map(|c| match c {
            '\u{3400}'..='\u{9FFF}' => KANJI_BEATS,
            'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' | 'ゃ' | 'ゅ' | 'ょ' | 'ァ' | 'ィ' | 'ゥ' | 'ェ'
            | 'ォ' | 'ャ' | 'ュ' | 'ョ' => 0,
            _ => 1,
        })
        .sum()
}

/// How long a beat takes around word `index`: the median over nearby words said without a pause,
/// since a word after a pause has no span of its own to measure.
fn local_character_seconds(spans: &[(f64, usize)], index: usize) -> f64 {
    let first = index.saturating_sub(LOCAL_RATE_WORDS).max(1);
    let last = (index + LOCAL_RATE_WORDS + 1).min(spans.len());
    let mut rates: Vec<f64> = (first..last)
        .filter(|near| *near != index && spans[*near].1 > 0)
        .map(|near| (spans[near].0 - spans[near - 1].0) / spans[near].1 as f64)
        .filter(|rate| *rate > 0.0 && *rate <= LONGEST_ONSET_SECONDS_PER_CHARACTER)
        .collect();
    rates.sort_by(f64::total_cmp);
    rates
        .get(rates.len() / 2)
        .copied()
        .unwrap_or(LONGEST_ONSET_SECONDS_PER_CHARACTER)
}

/// `t_dtw` marks where a token ends, not where it starts: against Nemotron it sits 0.01 s from a
/// one-character token's end and 0.13 s after its start, further with each character. So a word
/// ends there and starts after the word before, or as long before its end as its characters take
/// at the local rate when a pause comes first. Words read without warping have their own span.
fn open_warped_words(words: &mut [Word]) {
    let mut latest = 0.0_f64;
    let spans: Vec<(f64, usize)> = words
        .iter()
        .map(|word| {
            latest = latest.max(word.end);
            (latest, spoken_beats(word.text.trim()))
        })
        .collect();
    let mut previous_end = 0.0_f64;
    for (index, word) in words.iter_mut().enumerate() {
        if word.end > word.start {
            previous_end = previous_end.max(word.end);
            continue;
        }
        let end = spans[index].0;
        let length = spans[index].1 as f64 * local_character_seconds(&spans, index);
        word.start = (end - length).max(previous_end);
        word.end = end;
        previous_end = end;
    }
}

/// Warped starts can step back a little; each word ends where the next starts or when its
/// characters have been said, whichever comes first.
fn close_warped_words(words: &mut [Word]) {
    let mut latest = 0.0_f64;
    for word in words.iter_mut() {
        word.start = word.start.max(latest);
        word.end = word.start;
        latest = word.start;
    }
    for index in 0..words.len() {
        let characters = words[index].text.trim().chars().count() as f64;
        let said = words[index].start + characters * LONGEST_SPOKEN_CHARACTER_SECONDS;
        words[index].end = words
            .get(index + 1)
            .map_or(said, |next| next.start.min(said));
    }
}

/// One input of a whisper-cli run and where its output goes, without the extension.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhisperClip {
    pub wav: PathBuf,
    pub base: PathBuf,
}

/// All `clips` in one process, so the model is loaded once. Each file's percentage starts over, so a
/// drop means the next file has begun.
fn run_whisper(
    program: &Path,
    run: &WhisperRun<'_>,
    extra_args: &[&str],
    clips: &[WhisperClip],
    progress: &mut dyn FnMut(f64),
) -> AppResult<Vec<Vec<Word>>> {
    let mut command = background_command(program);
    command.arg("-m").arg(run.model);
    clips.iter().for_each(|clip| {
        command.arg("-f").arg(&clip.wav).arg("-of").arg(&clip.base);
    });
    command
        .args(["-l", run.language, "-ojf", "-pp"])
        .args(extra_args);
    let (mut finished, mut last) = (0.0, 0.0);
    run_with_progress(command, "whisper-cli", run.cancel, |line| {
        if let Some(fraction) = parse_whisper_progress(line) {
            if fraction < last {
                finished += 1.0;
            }
            last = fraction;
            progress((finished + fraction) / clips.len() as f64);
        }
    })?;
    clips
        .iter()
        .map(|clip| {
            let json = std::fs::read(clip.base.with_extension("json"))
                .map_err(io_context("read whisper output"))?;
            parse_whisper_json(&json)
        })
        .collect()
}

fn parse_voice_spans(output: &str) -> Vec<VoiceSpan> {
    output
        .lines()
        .filter_map(|line| {
            let (_, times) = line
                .strip_prefix("Speech segment ")?
                .split_once(": start = ")?;
            let (start, end) = times.split_once(", end = ")?;
            Some(VoiceSpan {
                start: start.trim().parse::<f64>().ok()? / CENTISECONDS_PER_SECOND,
                end: end.trim().parse::<f64>().ok()? / CENTISECONDS_PER_SECOND,
            })
        })
        .collect()
}

/// Where Silero hears a voice; its model lies beside the tool. `run_capture` cannot be cancelled,
/// which is fine at about 13 s for a 40-minute video.
pub fn voice_spans(tools: &Tools, wav: &Path) -> AppResult<Vec<VoiceSpan>> {
    let mut command = background_command(&tools.voice_detector);
    command
        .arg("-f")
        .arg(wav)
        .arg("-vm")
        .arg(tools.voice_detector.with_file_name(VOICE_MODEL))
        .arg("-np");
    let output = run_capture(command, "whisper-vad-speech-segments")?;
    Ok(parse_voice_spans(&String::from_utf8_lossy(&output)))
}

/// Runs `pass` on `tools.whisper`, and on the CPU build if a GPU build exited with a failure, such
/// as a driver that crashes or a CPU missing an instruction.
fn with_cpu_fallback<T>(
    tools: &Tools,
    mut pass: impl FnMut(&Path) -> AppResult<T>,
) -> AppResult<T> {
    let failure = match pass(&tools.whisper) {
        Ok(value) => return Ok(value),
        Err(failure) => failure,
    };
    match (&failure, &tools.whisper_cpu) {
        (AppError::Tool { .. }, Some(cpu)) => pass(cpu),
        _ => Err(failure),
    }
}

/// Runs whisper.cpp once for the text and each word's warped time.
pub fn transcribe(
    tools: &Tools,
    model: &Path,
    wav: &Path,
    language: &str,
    work: &Path,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f64),
) -> AppResult<Vec<Word>> {
    let clip = WhisperClip {
        wav: wav.to_path_buf(),
        base: work.join("asr"),
    };
    let heard = transcribe_many(tools, model, &[clip], language, cancel, progress)?;
    Ok(heard.into_iter().flatten().collect())
}

/// `transcribe` for many clips, each with the words it heard, reloading the model only per batch.
pub fn transcribe_many(
    tools: &Tools,
    model: &Path,
    clips: &[WhisperClip],
    language: &str,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f64),
) -> AppResult<Vec<Vec<Word>>> {
    let run = WhisperRun {
        model,
        language,
        cancel,
    };
    let mut heard = Vec::with_capacity(clips.len());
    for batch in clips.chunks(WHISPER_BATCH_FILES) {
        let before = heard.len() as f64;
        let mut report = |fraction: f64| {
            progress((before + fraction * batch.len() as f64) / clips.len() as f64);
        };
        let warped = with_cpu_fallback(tools, |program| {
            run_whisper(program, &run, &TOKEN_TIMING_ARGS, batch, &mut report)
        })
        .map(|mut files| {
            files.iter_mut().for_each(|words| close_warped_words(words));
            files
        });
        heard.extend(or_plain_times(warped, || {
            with_cpu_fallback(tools, |program| {
                run_whisper(program, &run, &[], batch, &mut report)
            })
        })?);
    }
    Ok(heard)
}

/// Warping that failed leaves plain token times, which are coarse but usable; only cancelling stops the job.
fn or_plain_times<T>(warped: AppResult<T>, plain: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
    match warped {
        Err(AppError::Cancelled) => Err(AppError::Cancelled),
        Err(_) => plain(),
        words => words,
    }
}

fn segment_to_turn(segment: &Value) -> Option<SpeakerTurn> {
    let (start, end) = (segment["start"].as_f64()?, segment["end"].as_f64()?);
    let speaker = segment["speaker"]
        .as_u64()
        .and_then(|number| u32::try_from(number).ok())
        .filter(|number| *number > 0)?;
    Some(SpeakerTurn {
        start,
        end: end.max(start),
        speaker,
    })
}

/// `/v1/audio/diarizations`: speaker turns, which overlap when people talk at once.
pub fn parse_speaker_segments(bytes: &[u8]) -> AppResult<Vec<SpeakerTurn>> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| AppError::Invalid(format!("speaker JSON: {error}")))?;
    let segments = value["segments"]
        .as_array()
        .ok_or_else(|| AppError::Invalid("speaker JSON has no segments".into()))?;
    Ok(segments.iter().filter_map(segment_to_turn).collect())
}

/// Decoder and encoder paths of a speech-LLM model.
#[derive(Clone, Debug)]
pub struct DiarizeModel<'a> {
    pub entry: &'a ModelEntry,
    pub main_file: &'a Path,
    pub encoder: Option<&'a Path>,
    /// `--device`: the GPU when the model fits, else the CPU.
    pub device: &'a str,
}

fn speaker_server_command(
    tools: &Tools,
    model: &DiarizeModel<'_>,
    port: u16,
) -> AppResult<std::process::Command> {
    if model.entry.role != Role::Diarize {
        return Err(AppError::Invalid(format!(
            "{} is not a diarization model",
            model.entry.id
        )));
    }
    let mut command = background_command(&tools.nemo);
    command
        .args(["serve", "--no-ui", "--host", LOOPBACK, "--port"])
        .arg(port.to_string())
        .arg("--asr-model")
        .arg(model.main_file);
    if let Some(encoder) = model.encoder {
        command.arg("--salm-encoder").arg(encoder);
    }
    command.args(model.entry.extra_args.iter().map(OsStr::new));
    command.args(["--device", model.device]);
    command.args(["--read-timeout", SERVER_SOCKET_SECONDS]);
    command.args(["--write-timeout", SERVER_SOCKET_SECONDS]);
    Ok(command)
}

fn free_port() -> AppResult<u16> {
    let listener = TcpListener::bind((LOOPBACK, 0)).map_err(io_context("reserve a local port"))?;
    let address = listener
        .local_addr()
        .map_err(io_context("reserve a local port"))?;
    Ok(address.port())
}

fn server_failure(detail: impl ToString) -> AppError {
    AppError::Tool {
        tool: "nemo-speech".to_owned(),
        detail: detail.to_string(),
    }
}

/// The server is on this computer, whatever proxy the system names.
fn local_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .proxy(None)
        .http_status_as_error(false)
        .build()
        .into()
}

fn wait_for_server(agent: &ureq::Agent, base: &str, stop: &CancelToken) -> AppResult<()> {
    let began = Instant::now();
    while !stop.is_cancelled() {
        let health = agent.get(format!("{base}/health")).call();
        if health.is_ok_and(|reply| reply.status() == HTTP_OK) {
            return Ok(());
        }
        if began.elapsed() > SERVER_READY_TIMEOUT {
            return Err(server_failure("the speaker server never became ready"));
        }
        std::thread::sleep(SERVER_POLL);
    }
    Err(server_failure(
        "the speaker server stopped before it was ready",
    ))
}

fn wav_upload(wav: &Path) -> AppResult<Vec<u8>> {
    let audio = std::fs::read(wav).map_err(io_context("read the audio for speakers"))?;
    let head = format!(
        "--{UPLOAD_BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: {AUDIO_CONTENT_TYPE}\r\n\r\n"
    );
    let tail = format!("\r\n--{UPLOAD_BOUNDARY}--\r\n");
    Ok([head.as_bytes(), &audio, tail.as_bytes()].concat())
}

/// Asks once the server is up. Its speaker branch needs no decoding, so a transcript the alignment
/// cannot place (one character outside its vocabulary loses a whole recording's word times) costs
/// nothing here.
fn ask_for_turns(port: u16, wav: &Path, stop: &CancelToken) -> AppResult<Vec<SpeakerTurn>> {
    let agent = local_agent();
    let base = format!("http://{LOOPBACK}:{port}");
    let upload = wav_upload(wav)?;
    wait_for_server(&agent, &base, stop)?;
    let mut reply = agent
        .post(format!("{base}/v1/audio/diarizations"))
        .content_type(format!("multipart/form-data; boundary={UPLOAD_BOUNDARY}"))
        .send(&upload[..])
        .map_err(server_failure)?;
    let status = reply.status();
    let body = reply
        .body_mut()
        .with_config()
        .limit(MOST_SEGMENT_BYTES)
        .read_to_vec()
        .map_err(server_failure)?;
    if status != HTTP_OK {
        return Err(server_failure(format!(
            "{status}: {}",
            String::from_utf8_lossy(&body)
        )));
    }
    parse_speaker_segments(&body)
}

/// Cancelling wins; then the answer; then why the server died, which says more than a lost connection.
fn settled_turns(
    cancel: &CancelToken,
    server: AppResult<()>,
    turns: AppResult<Vec<SpeakerTurn>>,
) -> AppResult<Vec<SpeakerTurn>> {
    cancel.check()?;
    match (turns, server) {
        (Ok(turns), _) => Ok(turns),
        (Err(_), Err(failure @ AppError::Tool { .. })) => Err(failure),
        (Err(failure), _) => Err(failure),
    }
}

/// Runs the chosen speaker model; it reports no percentage, so callers show elapsed time.
pub fn diarize(
    tools: &Tools,
    model: &DiarizeModel<'_>,
    wav: &Path,
    cancel: &CancelToken,
    on_tick: &mut dyn FnMut(),
) -> AppResult<Vec<SpeakerTurn>> {
    let port = free_port()?;
    let command = speaker_server_command(tools, model, port)?;
    let stop = CancelToken::default();
    std::thread::scope(|scope| {
        let answer = scope.spawn(|| {
            let turns = ask_for_turns(port, wav, &stop);
            stop.cancel();
            turns
        });
        let server = run_with_ticks(command, "nemo-speech", &stop, || {
            if cancel.is_cancelled() {
                stop.cancel();
            }
            on_tick();
        });
        // A server that exited by itself must end the wait for it.
        stop.cancel();
        let turns = answer
            .join()
            .unwrap_or_else(|_| Err(server_failure("the speaker request stopped")));
        settled_turns(cancel, server, turns)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu_tools(whisper_cpu: Option<&str>) -> Tools {
        Tools {
            ffmpeg: "f".into(),
            ffprobe: "p".into(),
            whisper: "gpu".into(),
            whisper_cpu: whisper_cpu.map(Into::into),
            voice_detector: "d".into(),
            nemo: "n".into(),
            backend: Backend::Vulkan,
        }
    }

    fn crashed() -> AppError {
        AppError::Tool {
            tool: "whisper-cli".to_owned(),
            detail: "exit code: 0xc000001d".to_owned(),
        }
    }

    #[test]
    fn a_working_gpu_build_never_starts_the_cpu_build() {
        let tools = gpu_tools(Some("cpu"));
        let mut started = Vec::new();
        let value = with_cpu_fallback(&tools, |program| {
            started.push(program.to_path_buf());
            Ok(7)
        })
        .unwrap();
        assert_eq!(value, 7);
        assert_eq!(started, vec![PathBuf::from("gpu")]);
    }

    #[test]
    fn a_crashed_gpu_build_falls_back_to_the_cpu_build() {
        let tools = gpu_tools(Some("cpu"));
        let mut started = Vec::new();
        let value = with_cpu_fallback(&tools, |program| {
            started.push(program.to_path_buf());
            if program == Path::new("gpu") {
                Err(crashed())
            } else {
                Ok(7)
            }
        })
        .unwrap();
        assert_eq!(value, 7);
        assert_eq!(started, vec![PathBuf::from("gpu"), PathBuf::from("cpu")]);
    }

    #[test]
    fn the_cpu_builds_own_failure_is_the_one_reported() {
        let tools = gpu_tools(Some("cpu"));
        let error = with_cpu_fallback::<()>(&tools, |program| {
            Err(AppError::Tool {
                tool: "whisper-cli".to_owned(),
                detail: program.display().to_string(),
            })
        })
        .unwrap_err()
        .to_string();
        assert_eq!(error, "whisper-cli failed: cpu");
    }

    #[test]
    fn cancelling_does_not_start_the_cpu_build() {
        let tools = gpu_tools(Some("cpu"));
        let mut starts = 0;
        let result = with_cpu_fallback::<()>(&tools, |_| {
            starts += 1;
            Err(AppError::Cancelled)
        });
        assert!(matches!(result, Err(AppError::Cancelled)));
        assert_eq!(starts, 1);
    }

    #[test]
    fn without_a_cpu_build_the_failure_stands() {
        let tools = gpu_tools(None);
        let mut starts = 0;
        let result = with_cpu_fallback::<()>(&tools, |_| {
            starts += 1;
            Err(crashed())
        });
        assert!(matches!(result, Err(AppError::Tool { .. })));
        assert_eq!(starts, 1);
    }

    #[test]
    fn ffmpeg_status_lines_parse() {
        assert_eq!(
            parse_ffmpeg_time("size=  10kB time=00:01:02.50 bitrate=1kbits/s"),
            Some(62.5)
        );
        assert_eq!(parse_ffmpeg_time("Press [q] to stop"), None);
        assert_eq!(parse_ffmpeg_time("size=N/A time=N/A bitrate=N/A"), None);
    }

    #[test]
    fn whisper_progress_lines_parse() {
        assert_eq!(
            parse_whisper_progress("whisper_print_progress_callback: progress =  45%"),
            Some(0.45)
        );
        assert_eq!(parse_whisper_progress("whisper_full: other line"), None);
    }

    #[test]
    fn split_utf8_tokens_merge_into_one_word() {
        // "話" is E8 A9 B1; whisper.cpp may split it across two tokens.
        let json: &[u8] = b"{\"transcription\":[{\"tokens\":[
            {\"text\":\"[_BEG_]\",\"offsets\":{\"from\":0,\"to\":0}},
            {\"text\":\"\xE8\xA9\",\"offsets\":{\"from\":100,\"to\":200}},
            {\"text\":\"\xB1\",\"offsets\":{\"from\":200,\"to\":300}},
            {\"text\":\"\xE3\x81\x99\",\"offsets\":{\"from\":300,\"to\":500}}
        ]}]}";
        let words = parse_whisper_json(json).unwrap();
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].text, "話");
        assert_eq!(words[0].start, 0.1);
        assert_eq!(words[0].end, 0.3);
        assert_eq!(words[1].text, "す");
    }

    #[test]
    fn whisper_json_without_transcription_is_an_error() {
        assert_eq!(
            parse_whisper_json(b"{}").unwrap_err().to_string(),
            "whisper JSON has no transcription"
        );
    }

    #[test]
    fn speaker_segments_keep_their_overlap() {
        let json = br#"{"segments":[
            {"start":0.0,"end":1.0,"speaker":1},
            {"start":0.5,"end":1.5,"speaker":2},
            {"start":2.0,"end":1.0,"speaker":1},
            {"start":3.0,"end":4.0,"speaker":0},
            {"start":5.0,"end":6.0}
        ]}"#;
        assert_eq!(
            parse_speaker_segments(json).unwrap(),
            vec![
                SpeakerTurn {
                    start: 0.0,
                    end: 1.0,
                    speaker: 1
                },
                SpeakerTurn {
                    start: 0.5,
                    end: 1.5,
                    speaker: 2
                },
                SpeakerTurn {
                    start: 2.0,
                    end: 2.0,
                    speaker: 1
                },
            ]
        );
    }

    #[test]
    fn probe_reads_size_and_rate() {
        let json = br#"{"format":{"duration":"12.5"},"streams":[
            {"codec_type":"video","width":1920,"height":1080,"avg_frame_rate":"30000/1001"},
            {"codec_type":"audio"}]}"#;
        assert_eq!(
            parse_probe(json).unwrap(),
            VideoInfo {
                width: 1920,
                height: 1080,
                duration: 12.5,
                fps_num: 30000,
                fps_den: 1001,
                audio: None,
            }
        );
    }

    #[test]
    fn rotated_phone_video_swaps_width_and_height() {
        let json = br#"{"format":{"duration":"1"},"streams":[
            {"codec_type":"video","width":1920,"height":1080,"avg_frame_rate":"30/1","side_data_list":[{"rotation":-90}]},
            {"codec_type":"audio"}]}"#;
        let info = parse_probe(json).unwrap();
        assert_eq!((info.width, info.height), (1080, 1920));
    }

    #[test]
    fn unusable_rate_falls_back() {
        let json = br#"{"format":{"duration":"1"},"streams":[
            {"codec_type":"video","width":640,"height":360,"avg_frame_rate":"0/0","r_frame_rate":"0/0"},
            {"codec_type":"audio"}]}"#;
        let info = parse_probe(json).unwrap();
        assert_eq!((info.fps_num, info.fps_den), (30, 1));
    }

    #[test]
    fn files_without_video_or_audio_are_refused() {
        let audio_only = br#"{"format":{"duration":"1"},"streams":[{"codec_type":"audio"}]}"#;
        assert_eq!(
            parse_probe(audio_only).unwrap_err().to_string(),
            "the file has no video stream"
        );
        let silent = br#"{"format":{"duration":"1"},"streams":[{"codec_type":"video","width":2,"height":2}]}"#;
        assert_eq!(
            parse_probe(silent).unwrap_err().to_string(),
            "the file has no audio stream"
        );
    }

    #[test]
    fn a_bundled_cover_picture_is_not_video() {
        let json = br#"{"format":{"duration":"1"},"streams":[
            {"codec_type":"video","width":2,"height":2,"disposition":{"attached_pic":1}},
            {"codec_type":"audio"}]}"#;
        assert_eq!(
            parse_probe(json).unwrap_err().to_string(),
            "the file has no video stream"
        );
    }

    #[test]
    fn a_video_without_a_size_or_duration_is_refused() {
        let no_size = br#"{"format":{"duration":"1"},"streams":[{"codec_type":"video"},{"codec_type":"audio"}]}"#;
        assert_eq!(
            parse_probe(no_size).unwrap_err().to_string(),
            "the video size could not be read"
        );
        let no_duration = br#"{"format":{},"streams":[{"codec_type":"video","width":2,"height":2},{"codec_type":"audio"}]}"#;
        assert_eq!(
            parse_probe(no_duration).unwrap_err().to_string(),
            "the video duration could not be read"
        );
    }

    #[test]
    fn a_rotate_tag_also_swaps_the_size() {
        let json = br#"{"format":{"duration":"1"},"streams":[
            {"codec_type":"video","width":4,"height":2,"r_frame_rate":"25/1","tags":{"rotate":"90"}},
            {"codec_type":"audio"}]}"#;
        assert_eq!(
            parse_probe(json).unwrap(),
            VideoInfo {
                width: 2,
                height: 4,
                duration: 1.0,
                fps_num: 25,
                fps_den: 1,
                audio: None,
            }
        );
    }

    #[test]
    fn broken_tool_json_is_refused() {
        assert_eq!(
            parse_probe(b"{").unwrap_err().to_string(),
            "ffprobe output: EOF while parsing an object at line 1 column 1"
        );
        assert_eq!(
            parse_probe(b"{}").unwrap_err().to_string(),
            "ffprobe found no streams"
        );
        assert_eq!(
            parse_whisper_json(b"{").unwrap_err().to_string(),
            "whisper JSON: EOF while parsing an object at line 1 column 1"
        );
        assert_eq!(
            parse_speaker_segments(b"{").unwrap_err().to_string(),
            "speaker JSON: EOF while parsing an object at line 1 column 1"
        );
        assert_eq!(
            parse_speaker_segments(b"{}").unwrap_err().to_string(),
            "speaker JSON has no segments"
        );
    }

    fn args_of(command: &std::process::Command) -> Vec<String> {
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn the_speaker_server_gets_its_encoder_and_extra_arguments() {
        let tools = Tools {
            ffmpeg: "f".into(),
            ffprobe: "p".into(),
            whisper: "w".into(),
            whisper_cpu: None,
            voice_detector: "d".into(),
            nemo: "n".into(),
            backend: Backend::Default,
        };
        let model = DiarizeModel {
            entry: crate::catalog::find_entry("nemotron-3.5-transcribe-q2m").unwrap(),
            main_file: Path::new("m.gguf"),
            encoder: Some(Path::new("e.gguf")),
            device: "cpu",
        };
        let command = speaker_server_command(&tools, &model, 4321).unwrap();
        assert_eq!(
            args_of(&command),
            vec![
                "serve",
                "--no-ui",
                "--host",
                "127.0.0.1",
                "--port",
                "4321",
                "--asr-model",
                "m.gguf",
                "--salm-encoder",
                "e.gguf",
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
                "--device",
                "cpu",
                "--read-timeout",
                "3600",
                "--write-timeout",
                "3600"
            ]
        );
    }

    #[test]
    fn a_model_with_its_encoder_named_beside_it_needs_no_encoder_argument() {
        let tools = Tools {
            ffmpeg: "f".into(),
            ffprobe: "p".into(),
            whisper: "w".into(),
            whisper_cpu: None,
            voice_detector: "d".into(),
            nemo: "n".into(),
            backend: Backend::Default,
        };
        let model = DiarizeModel {
            entry: crate::catalog::find_entry("nemotron-3.5-transcribe-q2l").unwrap(),
            main_file: Path::new("m.gguf"),
            encoder: None,
            device: "auto",
        };
        let command = speaker_server_command(&tools, &model, 1).unwrap();
        assert_eq!(
            args_of(&command),
            vec![
                "serve",
                "--no-ui",
                "--host",
                "127.0.0.1",
                "--port",
                "1",
                "--asr-model",
                "m.gguf",
                "--device",
                "auto",
                "--read-timeout",
                "3600",
                "--write-timeout",
                "3600"
            ]
        );
    }

    #[test]
    fn the_upload_wraps_the_audio_as_one_form_field() {
        let root = std::env::temp_dir().join(format!("wav-upload-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let wav = root.join("a.wav");
        std::fs::write(&wav, b"RIFF").unwrap();
        let body = wav_upload(&wav).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            String::from_utf8(body).unwrap(),
            "--transcribe-captions-3f7c1e9a5b2d4860a1c7e3f95d0b6824\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\nRIFF\r\n--transcribe-captions-3f7c1e9a5b2d4860a1c7e3f95d0b6824--\r\n"
        );
    }

    #[test]
    fn an_answer_outlives_the_server_it_stopped() {
        let turn = SpeakerTurn {
            start: 0.0,
            end: 1.0,
            speaker: 1,
        };
        let kept = settled_turns(
            &CancelToken::default(),
            Err(AppError::Cancelled),
            Ok(vec![turn]),
        );
        assert_eq!(kept.unwrap(), vec![turn]);
    }

    #[test]
    fn a_server_that_died_explains_the_lost_answer() {
        let error = settled_turns(
            &CancelToken::default(),
            Err(crashed()),
            Err(AppError::Invalid("connection lost".into())),
        );
        assert_eq!(
            error.unwrap_err().to_string(),
            "whisper-cli failed: exit code: 0xc000001d"
        );
    }

    #[test]
    fn a_bad_answer_from_a_living_server_is_reported() {
        let error = settled_turns(
            &CancelToken::default(),
            Err(AppError::Cancelled),
            Err(AppError::Invalid("speaker JSON has no segments".into())),
        );
        assert_eq!(
            error.unwrap_err().to_string(),
            "speaker JSON has no segments"
        );
    }

    #[test]
    fn cancelling_wins_over_any_answer() {
        let cancel = CancelToken::default();
        cancel.cancel();
        let error = settled_turns(&cancel, Ok(()), Ok(Vec::new()));
        assert_eq!(error.unwrap_err().to_string(), "cancelled");
    }

    #[test]
    fn a_warped_time_is_where_a_word_ends() {
        let json = br#"{"transcription":[{"tokens":[
            {"text":"[_BEG_]","offsets":{"from":0,"to":0},"t_dtw":-1},
            {"text":"\u00e3\u0081\u00af","offsets":{"from":0,"to":5000},"t_dtw":120},
            {"text":"\u00e3\u0081\u0084","offsets":{"from":5000,"to":5100},"t_dtw":150},
            {"text":"\u00e3\u0081\u0086","offsets":{"from":5100,"to":9000},"t_dtw":600}
        ]}]}"#;
        let words = parse_whisper_json(json).unwrap();
        assert_eq!(
            words
                .iter()
                .map(|word| (word.start, word.end))
                .collect::<Vec<_>>(),
            vec![(1.0, 1.2), (1.3, 1.5), (5.8, 6.0)]
        );
    }

    #[test]
    fn a_warped_start_never_steps_back() {
        let json = br#"{"transcription":[{"tokens":[
            {"text":"a","offsets":{"from":0,"to":100},"t_dtw":200},
            {"text":"b","offsets":{"from":100,"to":200},"t_dtw":150}
        ]}]}"#;
        let words = parse_whisper_json(json).unwrap();
        assert_eq!(
            (words[0].end, words[1].start, words[1].end),
            (2.0, 2.0, 2.0)
        );
    }

    #[test]
    fn a_token_without_a_warped_time_keeps_its_offsets() {
        let json = br#"{"transcription":[{"tokens":[
            {"text":"a","offsets":{"from":1000,"to":1500},"t_dtw":-1},
            {"text":"b","offsets":{"from":2000,"to":2500}}
        ]}]}"#;
        let words = parse_whisper_json(json).unwrap();
        assert_eq!(
            words
                .iter()
                .map(|word| (word.start, word.end))
                .collect::<Vec<_>>(),
            vec![(1.0, 1.5), (2.0, 2.5)]
        );
    }

    #[test]
    fn warped_times_outside_their_segment_come_back() {
        assert_eq!(warped_start(5.0, Some((4.0, 6.0)), Some(4.5)), 5.0);
        assert_eq!(warped_start(2.5, Some((4.0, 6.0)), None), 4.0);
        assert_eq!(warped_start(8.0, Some((4.0, 6.0)), Some(4.5)), 4.75);
        assert_eq!(warped_start(8.0, Some((4.0, 6.0)), Some(5.9)), 6.0);
        assert_eq!(warped_start(8.0, Some((4.0, 6.0)), None), 6.0);
        assert_eq!(warped_start(8.0, None, None), 8.0);
    }

    #[test]
    fn warped_times_just_outside_their_segment_stay() {
        assert_eq!(warped_start(3.5, Some((4.0, 6.0)), None), 3.5);
        assert_eq!(warped_start(6.5, Some((4.0, 6.0)), Some(4.5)), 6.5);
    }

    #[test]
    fn the_last_token_of_a_segment_stays_with_the_word_before_it() {
        let json = br#"{"transcription":[{"offsets":{"from":1000,"to":3000},"tokens":[
            {"text":"a","offsets":{"from":1000,"to":1100},"t_dtw":120},
            {"text":"b","offsets":{"from":1100,"to":3000},"t_dtw":900}
        ]}]}"#;
        let words = parse_whisper_json(json).unwrap();
        assert_eq!(words[1].end, 1.45);
    }

    fn timed(start: f64, end: f64, text: &str) -> Word {
        Word {
            start,
            end,
            text: text.to_owned(),
            speaker: None,
        }
    }

    #[test]
    fn warped_words_are_closed_at_the_next_start_or_their_said_length() {
        let mut words = vec![
            timed(1.0, 1.0, "味濃い"),
            timed(2.0, 2.0, " じゃん"),
            timed(1.5, 1.5, "です"),
        ];
        close_warped_words(&mut words);
        assert_eq!(
            words
                .iter()
                .map(|word| (word.text.as_str(), word.start, word.end))
                .collect::<Vec<_>>(),
            vec![
                ("味濃い", 1.0, 1.75),
                (" じゃん", 2.0, 2.0),
                ("です", 2.0, 2.5)
            ]
        );
    }

    #[test]
    fn warping_changes_the_times_but_never_the_text() {
        let plain = br#"{"transcription":[{"tokens":[
            {"text":"[_BEG_]","offsets":{"from":0,"to":0}},
            {"text":"\u00e3\u0081\u00af","offsets":{"from":0,"to":1000}},
            {"text":" a","offsets":{"from":1000,"to":2000}}
        ]}]}"#;
        let warped = br#"{"transcription":[{"tokens":[
            {"text":"[_BEG_]","offsets":{"from":0,"to":0},"t_dtw":-1},
            {"text":"\u00e3\u0081\u00af","offsets":{"from":0,"to":1000},"t_dtw":70},
            {"text":" a","offsets":{"from":1000,"to":2000},"t_dtw":150}
        ]}]}"#;
        let text = |json: &[u8]| {
            parse_whisper_json(json)
                .unwrap()
                .into_iter()
                .map(|word| word.text)
                .collect::<Vec<_>>()
        };
        assert_eq!(text(plain), vec!["は", " a"]);
        assert_eq!(text(warped), vec!["は", " a"]);
    }

    #[test]
    fn a_failed_warping_run_falls_back_to_plain_times() {
        let words = vec![timed(1.0, 2.0, "はい")];
        let kept = or_plain_times(Err(crashed()), || Ok(words.clone())).unwrap();
        assert_eq!(kept, vec![timed(1.0, 2.0, "はい")]);
    }

    #[test]
    fn a_cancel_during_warping_starts_no_plain_run() {
        let mut starts = 0;
        let result: AppResult<Vec<Word>> = or_plain_times(Err(AppError::Cancelled), || {
            starts += 1;
            Ok(Vec::new())
        });
        assert_eq!(result.unwrap_err().to_string(), "cancelled");
        assert_eq!(starts, 0);
    }

    #[test]
    fn a_good_warping_run_needs_no_plain_run() {
        let mut starts = 0;
        let kept = or_plain_times(Ok(vec![timed(5.0, 5.2, "は")]), || {
            starts += 1;
            Ok(Vec::new())
        })
        .unwrap();
        assert_eq!(kept, vec![timed(5.0, 5.2, "は")]);
        assert_eq!(starts, 0);
    }

    #[test]
    fn a_long_word_after_a_pause_starts_before_its_end_by_its_length() {
        let json = br#"{"transcription":[{"tokens":[
            {"text":" ","offsets":{"from":1900,"to":2100},"t_dtw":190},
            {"text":"abc","offsets":{"from":2100,"to":3900},"t_dtw":282},
            {"text":"d","offsets":{"from":3900,"to":4000},"t_dtw":404}
        ]}]}"#;
        let words = parse_whisper_json(json).unwrap();
        assert_eq!(
            words
                .iter()
                .map(|word| ((word.start * 100.0).round(), (word.end * 100.0).round()))
                .collect::<Vec<_>>(),
            vec![(190.0, 190.0), (222.0, 282.0), (384.0, 404.0)]
        );
    }

    #[test]
    fn a_kanji_takes_two_beats_and_a_small_kana_none() {
        assert_eq!(spoken_beats("今日は"), 5);
        assert_eq!(spoken_beats("ちょっと"), 3);
        assert_eq!(spoken_beats("abc"), 3);
    }

    #[test]
    fn a_word_after_a_pause_opens_at_the_speed_of_the_speech_around_it() {
        let spans = [(1.0, 1), (1.1, 1), (1.2, 1), (3.0, 2), (3.1, 1)];
        assert_eq!((local_character_seconds(&spans, 3) * 100.0).round(), 10.0);
        assert_eq!(local_character_seconds(&[(1.0, 1), (5.0, 2)], 1), 0.2);
    }

    #[test]
    fn words_in_steady_speech_start_where_the_last_one_ended() {
        let json = br#"{"transcription":[{"tokens":[
            {"text":"a","offsets":{"from":0,"to":100},"t_dtw":100},
            {"text":"bc","offsets":{"from":100,"to":200},"t_dtw":120}
        ]}]}"#;
        let words = parse_whisper_json(json).unwrap();
        assert_eq!(words[1].start, 1.0);
    }

    #[test]
    fn voice_spans_read_the_tools_centiseconds() {
        assert_eq!(
            parse_voice_spans(
                "\nDetected 2 speech segments:\nSpeech segment 0: start = 0.00, end = 80.00\nSpeech segment 1: start = 211.00, end = 349.50\n\n"
            ),
            vec![
                VoiceSpan { start: 0.0, end: 0.8 },
                VoiceSpan { start: 2.11, end: 3.495 },
            ]
        );
    }

    #[test]
    fn voice_span_lines_that_do_not_parse_are_left_out() {
        assert_eq!(
            parse_voice_spans("Speech segment 0: start = x, end = 1.00\nSpeech segment 1: start = 1.00, end = y\nother\nSpeech segment 2: nothing"),
            vec![]
        );
    }

    #[test]
    fn a_plain_word_after_warped_words_keeps_its_span() {
        let json = br#"{"transcription":[{"tokens":[
            {"text":"a","offsets":{"from":1000,"to":1500},"t_dtw":-1},
            {"text":"b","offsets":{"from":1500,"to":1600},"t_dtw":200}
        ]}]}"#;
        let words = parse_whisper_json(json).unwrap();
        assert_eq!((words[0].start, words[0].end), (1.0, 1.5));
        assert_eq!((words[1].start, words[1].end), (1.8, 2.0));
    }

    #[test]
    fn audio_details_are_read_when_stated() {
        let json = br#"{"format":{"duration":"30.03"},"streams":[
            {"codec_type":"video","width":720,"height":1280,"avg_frame_rate":"30/1","nb_frames":"1074"},
            {"codec_type":"audio","sample_rate":"44100","channels":2,"duration":"30.00381"}]}"#;
        assert_eq!(
            parse_probe(json).unwrap(),
            VideoInfo {
                width: 720,
                height: 1280,
                duration: 30.03,
                fps_num: 30,
                fps_den: 1,
                audio: Some(AudioInfo {
                    sample_rate: 44_100,
                    channels: 2,
                    duration: 30.00381,
                }),
            }
        );
    }

    #[test]
    fn a_transcript_from_before_audio_details_still_reads() {
        let info: VideoInfo = serde_json::from_str(
            r#"{"width":2,"height":2,"duration":1.0,"fps_num":30,"fps_den":1}"#,
        )
        .unwrap();
        assert_eq!(info.audio, None);
    }
}
