//! Drives the bundled tools: ffmpeg/ffprobe for media, whisper.cpp for words, nemo-speech for speakers.

use crate::captions::{SpeakerTurn, VoiceSpan, Word};
use crate::catalog::{Backend, ModelEntry, Role};
use crate::error::{io_context, AppError, AppResult, UserError};
use crate::paths::write_atomic;
use crate::paths::AppPaths;
use crate::process::{
    background_command, run_capture, run_with_progress, run_with_ticks, CancelToken,
};
use crate::separation::{copy_span, cut_clips, wav_head, wav_layout, WavLayout};
use crate::whisper_seam::spliced_batches;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use ureq::SendBody;

const TOOL_SEARCH_DEPTH: usize = 3;
const EXE_SUFFIX: &str = if cfg!(windows) { ".exe" } else { "" };
/// The vocabulary and settings of Nemotron 3.5's decoder; the rest of the file is zeros. The speaker
/// turns come from the encoder and never touch the decoder's weights, so this takes their place:
/// 2.8 GB against 22 GB at the peak over an hour, with identical turns.
/// `scripts/release/make-speaker-models.py` writes it from NVIDIA's release.
const SPEAKER_DECODER_HEAD: &str = "speaker-decoder-head.gguf";
const SPEAKER_DECODER_HEAD_SHA256: &str =
    "4072f5c796ddc46434140743953615bfacc40ba0aef5c12ebd58b8a9af5a280c";
const SPEAKER_DECODER_BYTES: u64 = 445_521_952;
/// `nemo-speech` refuses longer audio per request (`kMaxDurationSeconds`, 3600), whatever the model.
const LONGEST_SPEAKER_REQUEST_SECONDS: f64 = 3600.0;
/// Long audio goes in overlapping requests; this much in common tells whose number is whose.
const SPEAKER_REQUEST_OVERLAP_SECONDS: f64 = 300.0;
const CPU_DEVICE: &str = "cpu";
/// Warmup and speculative decoding both need the weights the stand-in lacks.
const SPEAKER_BRANCH_ARGS: [&str; 3] = ["--no-warmup", "--asr.salm.mtp_draft", "0"];
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
/// Whisper conditions each window on the text before it, so one repetition can lock it into saying
/// the same words for 20 minutes. Without that text it cannot lock, but it also loses about 0.04 of
/// its recall on speech that was fine, so only the looped ranges run this way.
const NO_ROLLING_CONTEXT_ARGS: [&str; 2] = ["-mc", "0"];
/// What the text of a segment is judged without: only the words can repeat.
pub(crate) const NOT_SPOKEN: &str = "、。!?！？,.・「」『』…ー~〜-";
/// A loop is at least this many characters said again, or the same short text this many times, since
/// a person does say `そうそう` twice.
const LOOP_UNIT_CHARACTERS: usize = 4;
const SHORT_TEXT_LOOP_SEGMENTS: usize = 3;
const WHOLE_TEXT_LOOP_SEGMENTS: usize = 2;
const UNIT_LOOP_REPEATS: usize = 3;
const LONGEST_LOOP_UNIT_CHARACTERS: usize = 40;
/// Around a looped range: the loop starts a little before the first segment that shows it.
const LOOP_MARGIN_SECONDS: f64 = 2.0;
/// Longer audio goes in batches of this, each its own process, so memory does not grow with the length.
const LONGEST_WHISPER_BATCH_SECONDS: f64 = 3600.0;
/// What two batches both hear, so they can be joined where they agree.
const WHISPER_BATCH_OVERLAP_SECONDS: f64 = 60.0;
/// Of the transcription stage's progress; the loops are usually a small part of a video.
const FIRST_PASS_PROGRESS: f64 = 0.9;
/// Files per whisper-cli run, as two long paths each must stay under Windows's 32 KB command line.
const WHISPER_BATCH_FILES: usize = 64;
const WHISPER_NO_GPU_FLAG: &str = "-ng";
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
    /// Run Whisper on the CPU whichever build it is: its model does not fit the GPU's memory.
    pub whisper_without_gpu: bool,
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
            whisper_without_gpu: false,
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
    without_gpu: bool,
    /// Whichever other arguments every file of the run takes.
    context: &'a [&'a str],
}

/// A segment of whisper.cpp's output, for judging whether it looped.
#[derive(Debug)]
struct HeardSegment {
    from: f64,
    to: f64,
    text: Vec<char>,
}

/// Seconds of a recording, start and end.
type TimeRange = (f64, f64);

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
        .args(run.context)
        .args(extra_args);
    if run.without_gpu {
        command.arg(WHISPER_NO_GPU_FLAG);
    }
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

/// Each batch's words and looped ranges in the clock of the whole audio, joined where the batches agree.
fn heard_in_batches(
    tools: &Tools,
    run: &WhisperRun<'_>,
    source: (&Path, &Path),
    spans: &[TimeRange],
    progress: &mut dyn FnMut(f64),
) -> AppResult<(Vec<Word>, Vec<TimeRange>)> {
    let (wav, work) = source;
    let total = spans.last().map_or(0.0, |span| span.1);
    let mut joined: Vec<Word> = Vec::new();
    let mut ranges: Vec<TimeRange> = Vec::new();
    for (index, span) in spans.iter().enumerate() {
        let base = work.join(format!("asr-batch-{index}"));
        let clip = WhisperClip {
            wav: base.with_extension("wav"),
            base: base.clone(),
        };
        copy_span(wav, *span, &clip.wav)?;
        let heard = transcribe_files(tools, run, std::slice::from_ref(&clip), &mut |fraction| {
            progress((span.0 + fraction * (span.1 - span.0)) / total);
        })?;
        // Batches take a lot of disk; only the words are needed from here on.
        let _ = std::fs::remove_file(&clip.wav);
        let moved = heard.into_iter().flatten().map(|word| Word {
            start: word.start + span.0,
            end: word.end + span.0,
            ..word
        });
        joined = if index == 0 {
            moved.collect()
        } else {
            spliced_batches(joined, moved.collect(), (span.0, spans[index - 1].1))
        };
        ranges.extend(
            looped_ranges_of(&base)
                .into_iter()
                .map(|range| (range.0 + span.0, range.1 + span.0)),
        );
    }
    close_warped_words(&mut joined);
    ranges.sort_by(|left, right| left.0.total_cmp(&right.0));
    Ok((joined, ranges.into_iter().fold(Vec::new(), with_merged)))
}

fn looped_ranges_of(base: &Path) -> Vec<TimeRange> {
    std::fs::read(base.with_extension("json"))
        .ok()
        .and_then(|json| whisper_segments(&json).ok())
        .map_or_else(Vec::new, |segments| looped_ranges(&segments))
}

/// The batches of `wav`; audio that fits one, or whose length cannot be read here, runs whole.
fn whisper_batches(wav: &Path) -> Vec<TimeRange> {
    File::open(wav)
        .ok()
        .and_then(|mut file| wav_layout(&mut file).ok())
        .map_or_else(Vec::new, |layout| {
            overlapping_spans(
                layout.seconds(),
                LONGEST_WHISPER_BATCH_SECONDS,
                WHISPER_BATCH_OVERLAP_SECONDS,
            )
        })
}

/// Runs whisper.cpp for the text and each word's warped time.
pub fn transcribe(
    tools: &Tools,
    model: &Path,
    wav: &Path,
    language: &str,
    work: &Path,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f64),
) -> AppResult<Vec<Word>> {
    let run = WhisperRun {
        model,
        language,
        cancel,
        without_gpu: tools.whisper_without_gpu,
        context: &[],
    };
    let mut first_pass = |fraction: f64| progress(fraction * FIRST_PASS_PROGRESS);
    let spans = whisper_batches(wav);
    let (words, ranges) = if spans.len() > 1 {
        heard_in_batches(tools, &run, (wav, work), &spans, &mut first_pass)?
    } else {
        let base = work.join("asr");
        let clip = WhisperClip {
            wav: wav.to_path_buf(),
            base: base.clone(),
        };
        let heard = transcribe_files(tools, &run, &[clip], &mut first_pass)?;
        (
            heard.into_iter().flatten().collect(),
            looped_ranges_of(&base),
        )
    };
    let redone = redo_ranges(tools, &run, wav, work, &ranges, &mut |fraction| {
        progress(FIRST_PASS_PROGRESS + fraction * (1.0 - FIRST_PASS_PROGRESS));
    });
    with_redone_ranges(words, &ranges, redone)
}

/// Each range on its own, without the text before it.
fn redo_ranges(
    tools: &Tools,
    first: &WhisperRun<'_>,
    wav: &Path,
    work: &Path,
    ranges: &[TimeRange],
    progress: &mut dyn FnMut(f64),
) -> AppResult<Vec<Vec<Word>>> {
    if ranges.is_empty() {
        return Ok(Vec::new());
    }
    let clips: Vec<WhisperClip> = (0..ranges.len())
        .map(|index| {
            let base = work.join(format!("asr-loop-{index}"));
            WhisperClip {
                wav: base.with_extension("wav"),
                base,
            }
        })
        .collect();
    let cuts: Vec<(f64, f64, PathBuf)> = ranges
        .iter()
        .zip(&clips)
        .map(|(range, clip)| (range.0, range.1, clip.wav.clone()))
        .collect();
    cut_clips(wav, &cuts)?;
    let run = WhisperRun {
        model: first.model,
        language: first.language,
        cancel: first.cancel,
        without_gpu: tools.whisper_without_gpu,
        context: &NO_ROLLING_CONTEXT_ARGS,
    };
    transcribe_files(tools, &run, &clips, progress)
}

/// A redo that failed leaves the first pass's words, which are wrong only where they looped; only
/// cancelling stops the job.
fn with_redone_ranges(
    words: Vec<Word>,
    ranges: &[TimeRange],
    redone: AppResult<Vec<Vec<Word>>>,
) -> AppResult<Vec<Word>> {
    match redone {
        Ok(files) if files.is_empty() => Ok(words),
        Ok(files) => Ok(spliced_words(words, ranges, files)),
        Err(AppError::Cancelled) => Err(AppError::Cancelled),
        Err(_) => Ok(words),
    }
}

fn is_inside(word: &Word, ranges: &[TimeRange]) -> bool {
    let middle = (word.start + word.end) / 2.0;
    ranges
        .iter()
        .any(|(from, to)| *from <= middle && middle < *to)
}

fn by_start(left: &Word, right: &Word) -> std::cmp::Ordering {
    left.start.total_cmp(&right.start)
}

fn spliced_words(words: Vec<Word>, ranges: &[TimeRange], files: Vec<Vec<Word>>) -> Vec<Word> {
    let kept = words.into_iter().filter(|word| !is_inside(word, ranges));
    let moved = ranges.iter().zip(files).flat_map(|(range, file)| {
        let offset = range.0;
        file.into_iter().map(move |word| Word {
            start: word.start + offset,
            end: word.end + offset,
            ..word
        })
    });
    let mut joined: Vec<Word> = kept.chain(moved).collect();
    joined.sort_by(by_start);
    close_warped_words(&mut joined);
    joined
}

fn whisper_segments(bytes: &[u8]) -> AppResult<Vec<HeardSegment>> {
    let value: Value = serde_json::from_str(&latin1_string(bytes))
        .map_err(|error| AppError::Invalid(format!("whisper JSON: {error}")))?;
    let segments = value["transcription"]
        .as_array()
        .ok_or_else(|| AppError::Invalid("whisper JSON has no transcription".into()))?;
    Ok(segments.iter().filter_map(heard_segment).collect())
}

fn heard_segment(segment: &Value) -> Option<HeardSegment> {
    let (from, to) = segment_bounds(segment)?;
    let bytes = latin1_bytes(segment["text"].as_str()?);
    let text = String::from_utf8_lossy(&bytes)
        .chars()
        .filter(|c| !c.is_whitespace() && !NOT_SPOKEN.contains(*c))
        .collect();
    Some(HeardSegment { from, to, text })
}

fn repeats_at(text: &[char], start: usize, unit: usize) -> usize {
    let pattern = &text[start..start + unit];
    text[start..]
        .chunks_exact(unit)
        .take_while(|chunk| *chunk == pattern)
        .count()
}

/// One text can hold the repetition itself when whisper.cpp did not split it.
fn holds_a_loop(text: &[char]) -> bool {
    (LOOP_UNIT_CHARACTERS..=LONGEST_LOOP_UNIT_CHARACTERS).any(|unit| {
        (0..text.len().saturating_sub(unit))
            .any(|start| repeats_at(text, start, unit) >= UNIT_LOOP_REPEATS)
    })
}

fn same_text_run(segments: &[HeardSegment]) -> usize {
    let text = &segments[0].text;
    segments
        .iter()
        .take_while(|segment| &segment.text == text)
        .count()
}

fn is_loop_run(text: &[char], run: usize) -> bool {
    if text.is_empty() {
        return false;
    }
    let long_enough = text.len() >= LOOP_UNIT_CHARACTERS && run >= WHOLE_TEXT_LOOP_SEGMENTS;
    long_enough || run >= SHORT_TEXT_LOOP_SEGMENTS || holds_a_loop(text)
}

fn with_merged(mut merged: Vec<TimeRange>, range: TimeRange) -> Vec<TimeRange> {
    match merged.last_mut() {
        Some(last) if range.0 <= last.1 => last.1 = last.1.max(range.1),
        _ => merged.push(range),
    }
    merged
}

/// Where the text repeats itself; the words alone decide, with nothing set per video.
fn looped_ranges(segments: &[HeardSegment]) -> Vec<TimeRange> {
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < segments.len() {
        let run = same_text_run(&segments[index..]);
        if is_loop_run(&segments[index].text, run) {
            ranges.push((
                (segments[index].from - LOOP_MARGIN_SECONDS).max(0.0),
                segments[index + run - 1].to + LOOP_MARGIN_SECONDS,
            ));
        }
        index += run;
    }
    ranges.into_iter().fold(Vec::new(), with_merged)
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
        without_gpu: tools.whisper_without_gpu,
        context: &[],
    };
    transcribe_files(tools, &run, clips, progress)
}

fn transcribe_files(
    tools: &Tools,
    run: &WhisperRun<'_>,
    clips: &[WhisperClip],
    progress: &mut dyn FnMut(f64),
) -> AppResult<Vec<Vec<Word>>> {
    let mut heard = Vec::with_capacity(clips.len());
    for batch in clips.chunks(WHISPER_BATCH_FILES) {
        let before = heard.len() as f64;
        let mut report = |fraction: f64| {
            progress((before + fraction * batch.len() as f64) / clips.len() as f64);
        };
        let warped = with_cpu_fallback(tools, |program| {
            run_whisper(program, run, &TOKEN_TIMING_ARGS, batch, &mut report)
        })
        .map(|mut files| {
            files.iter_mut().for_each(|words| close_warped_words(words));
            files
        });
        heard.extend(or_plain_times(warped, || {
            with_cpu_fallback(tools, |program| {
                run_whisper(program, run, &[], batch, &mut report)
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
    pub encoder: Option<&'a Path>,
    /// `--device`: the GPU when the model fits, else the CPU.
    pub device: &'a str,
    /// Holds the stand-in's head; see `SPEAKER_DECODER_HEAD`.
    pub resources: &'a Path,
}

fn speaker_server_command(
    tools: &Tools,
    model: &DiarizeModel<'_>,
    decoder: &Path,
    port: u16,
) -> AppResult<std::process::Command> {
    let mut command = background_command(&tools.nemo);
    command
        .args(["serve", "--no-ui", "--host", LOOPBACK, "--port"])
        .arg(port.to_string())
        .arg("--asr-model")
        .arg(decoder);
    if let Some(encoder) = model.encoder {
        command.arg("--salm-encoder").arg(encoder);
    }
    command.args(SPEAKER_BRANCH_ARGS);
    command.args(["--device", model.device]);
    command.args(["--read-timeout", SERVER_SOCKET_SECONDS]);
    command.args(["--write-timeout", SERVER_SOCKET_SECONDS]);
    Ok(command)
}

/// The stand-in for the decoder, made in `work`: a damaged or missing head is an error, not a silently
/// wrong model.
fn speaker_decoder_stand_in(resources: &Path, work: &Path) -> AppResult<PathBuf> {
    let source = resources.join("resources").join(SPEAKER_DECODER_HEAD);
    let head = std::fs::read(&source).map_err(io_context("read the speaker decoder"))?;
    if format!("{:x}", Sha256::digest(&head)) != SPEAKER_DECODER_HEAD_SHA256 {
        return Err(AppError::Invalid(format!(
            "{} is damaged; reinstall the app",
            source.display()
        )));
    }
    let file = work.join(SPEAKER_DECODER_HEAD);
    write_atomic(&file, &head)?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(&file)
        .and_then(|opened| opened.set_len(SPEAKER_DECODER_BYTES))
        .map_err(io_context("extend the speaker decoder"))?;
    Ok(file)
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

/// The parts long audio goes in; audio that fits one is left whole.
fn overlapping_spans(seconds: f64, longest: f64, overlap: f64) -> Vec<TimeRange> {
    let mut spans = Vec::new();
    let mut start = 0.0;
    loop {
        let end = (start + longest).min(seconds);
        spans.push((start, end));
        if end >= seconds {
            return spans;
        }
        start = end - overlap;
    }
}

fn speaker_spans(seconds: f64) -> Vec<TimeRange> {
    overlapping_spans(
        seconds,
        LONGEST_SPEAKER_REQUEST_SECONDS,
        SPEAKER_REQUEST_OVERLAP_SECONDS,
    )
}

/// One span of `wav` as a form field, read as it is sent so it never sits in memory.
fn post_audio(
    agent: &ureq::Agent,
    url: &str,
    wav: &Path,
    layout: &WavLayout,
    span: TimeRange,
) -> AppResult<Vec<u8>> {
    let (first, bytes) = layout.slice(span.0, span.1);
    let mut audio = File::open(wav).map_err(io_context("read the audio for speakers"))?;
    audio
        .seek(SeekFrom::Start(first))
        .map_err(io_context("read the audio for speakers"))?;
    let wav_header = wav_head(u32::try_from(bytes).unwrap_or(u32::MAX), layout.float);
    let head = format!(
        "--{UPLOAD_BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: {AUDIO_CONTENT_TYPE}\r\n\r\n"
    );
    let tail = format!("\r\n--{UPLOAD_BOUNDARY}--\r\n");
    let length = head.len() as u64 + wav_header.len() as u64 + bytes + tail.len() as u64;
    let mut body = Cursor::new(head)
        .chain(Cursor::new(wav_header))
        .chain(audio.take(bytes))
        .chain(Cursor::new(tail));
    let mut reply = agent
        .post(url)
        .content_type(format!("multipart/form-data; boundary={UPLOAD_BOUNDARY}"))
        .header("Content-Length", length.to_string())
        .send(SendBody::from_reader(&mut body))
        .map_err(server_failure)?;
    let status = reply.status();
    let answer = reply
        .body_mut()
        .with_config()
        .limit(MOST_SEGMENT_BYTES)
        .read_to_vec()
        .map_err(server_failure)?;
    if status != HTTP_OK {
        return Err(server_failure(format!(
            "{status}: {}",
            String::from_utf8_lossy(&answer)
        )));
    }
    Ok(answer)
}

/// Asks once the server is up, request by request. Its speaker branch needs no decoding, so a
/// transcript the alignment cannot place (one character outside its vocabulary loses a whole
/// recording's word times) costs nothing here.
fn ask_for_turns(port: u16, wav: &Path, stop: &CancelToken) -> AppResult<Vec<SpeakerTurn>> {
    let agent = local_agent();
    let base = format!("http://{LOOPBACK}:{port}");
    let mut source = File::open(wav).map_err(io_context("read the audio for speakers"))?;
    let layout = wav_layout(&mut source)
        .map_err(|error| AppError::Invalid(format!("{}: {error}", wav.display())))?;
    drop(source);
    wait_for_server(&agent, &base, stop)?;
    let url = format!("{base}/v1/audio/diarizations");
    let mut parts = Vec::new();
    for span in speaker_spans(layout.seconds()) {
        if stop.is_cancelled() {
            return Err(server_failure("the speaker server stopped"));
        }
        let answer = post_audio(&agent, &url, wav, &layout, span)?;
        parts.push((span, moved_turns(parse_speaker_segments(&answer)?, span.0)));
    }
    Ok(stitched_turns(parts))
}

fn moved_turns(turns: Vec<SpeakerTurn>, offset: f64) -> Vec<SpeakerTurn> {
    turns
        .into_iter()
        .map(|turn| SpeakerTurn {
            start: turn.start + offset,
            end: turn.end + offset,
            ..turn
        })
        .collect()
}

fn shared_seconds(left: &SpeakerTurn, right: &SpeakerTurn, range: TimeRange) -> f64 {
    let start = left.start.max(right.start).max(range.0);
    let end = left.end.min(right.end).min(range.1);
    (end - start).max(0.0)
}

fn by_most_shared(left: &(u32, u32, f64), right: &(u32, u32, f64)) -> std::cmp::Ordering {
    right.2.total_cmp(&left.2)
}

/// Which number of the earlier turns each number of `later` is, by who speaks together in `range`.
/// A voice the overlap does not hear gets a number of its own.
fn matched_speakers(
    earlier: &[SpeakerTurn],
    later: &[SpeakerTurn],
    range: TimeRange,
) -> HashMap<u32, u32> {
    let mut shared: HashMap<(u32, u32), f64> = HashMap::new();
    for old in earlier.iter().filter(|turn| turn.end > range.0) {
        for new in later.iter().filter(|turn| turn.start < range.1) {
            *shared.entry((new.speaker, old.speaker)).or_default() +=
                shared_seconds(old, new, range);
        }
    }
    let mut pairs: Vec<(u32, u32, f64)> = shared
        .into_iter()
        .filter(|(_, seconds)| *seconds > 0.0)
        .map(|((new, old), seconds)| (new, old, seconds))
        .collect();
    pairs.sort_by(by_most_shared);
    let mut matched = HashMap::new();
    let mut taken = HashSet::new();
    for (new, old, _) in pairs {
        if !matched.contains_key(&new) && taken.insert(old) {
            matched.insert(new, old);
        }
    }
    matched
}

/// One list of turns from overlapping requests: each request speaks up to the middle of its overlap.
fn stitched_turns(parts: Vec<(TimeRange, Vec<SpeakerTurn>)>) -> Vec<SpeakerTurn> {
    let mut joined: Vec<SpeakerTurn> = Vec::new();
    let mut heard_until = 0.0_f64;
    for ((start, end), turns) in parts {
        heard_until = heard_until.max(start);
        if joined.is_empty() {
            joined = turns;
            heard_until = end;
            continue;
        }
        let overlap = (start, heard_until);
        let middle = (start + heard_until) / 2.0;
        let matched = matched_speakers(&joined, &turns, overlap);
        let mut unused = joined.iter().map(|turn| turn.speaker).max().unwrap_or(0);
        let mut named: HashMap<u32, u32> = matched;
        joined.retain(|turn| turn.start < middle);
        joined
            .iter_mut()
            .for_each(|turn| turn.end = turn.end.min(middle));
        for turn in turns.into_iter().filter(|turn| turn.end > middle) {
            let speaker = *named.entry(turn.speaker).or_insert_with(|| {
                unused += 1;
                unused
            });
            joined.push(SpeakerTurn {
                start: turn.start.max(middle),
                speaker,
                ..turn
            });
        }
        heard_until = end;
    }
    joined
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
    work: &Path,
    cancel: &CancelToken,
    on_tick: &mut dyn FnMut(),
) -> AppResult<Vec<SpeakerTurn>> {
    if model.entry.role != Role::Diarize {
        return Err(AppError::Invalid(format!(
            "{} is not a diarization model",
            model.entry.id
        )));
    }
    let stand_in = speaker_decoder_stand_in(model.resources, work);
    let first = stand_in
        .as_ref()
        .map_err(|failure| AppError::Invalid(failure.to_string()))
        .and_then(|file| turns_from_server(tools, model, file, wav, cancel, on_tick));
    let failure = match first {
        Err(AppError::Cancelled) | Ok(_) => return first,
        Err(failure) => failure,
    };
    // A GPU the stand-in does not load on: the CPU reads it the same way, only slower.
    match (&stand_in, &failure) {
        (Ok(file), AppError::Tool { .. }) if model.device != CPU_DEVICE => {
            let on_cpu = DiarizeModel {
                device: CPU_DEVICE,
                ..model.clone()
            };
            turns_from_server(tools, &on_cpu, file, wav, cancel, on_tick)
        }
        _ => Err(failure),
    }
}

fn turns_from_server(
    tools: &Tools,
    model: &DiarizeModel<'_>,
    decoder: &Path,
    wav: &Path,
    cancel: &CancelToken,
    on_tick: &mut dyn FnMut(),
) -> AppResult<Vec<SpeakerTurn>> {
    let port = free_port()?;
    let command = speaker_server_command(tools, model, decoder, port)?;
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
            whisper_without_gpu: false,
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
    fn the_speaker_server_reads_the_encoder_and_never_decodes() {
        let tools = Tools {
            ffmpeg: "f".into(),
            ffprobe: "p".into(),
            whisper: "w".into(),
            whisper_cpu: None,
            voice_detector: "d".into(),
            nemo: "n".into(),
            whisper_without_gpu: false,
            backend: Backend::Default,
        };
        let model = DiarizeModel {
            entry: crate::catalog::find_entry("nemotron-3.5-speaker").unwrap(),
            encoder: Some(Path::new("e.gguf")),
            device: "cpu",
            resources: Path::new("r"),
        };
        let command = speaker_server_command(&tools, &model, Path::new("m.gguf"), 4321).unwrap();
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
                "--no-warmup",
                "--asr.salm.mtp_draft",
                "0",
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
            whisper_without_gpu: false,
            backend: Backend::Default,
        };
        let model = DiarizeModel {
            entry: crate::catalog::find_entry("nemotron-3.5-speaker").unwrap(),
            encoder: None,
            device: "auto",
            resources: Path::new("r"),
        };
        let command = speaker_server_command(&tools, &model, Path::new("m.gguf"), 1).unwrap();
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
                "--no-warmup",
                "--asr.salm.mtp_draft",
                "0",
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
    fn the_stand_in_is_the_verified_head_followed_by_zeros() {
        let root = std::env::temp_dir().join(format!("stand-in-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let file = speaker_decoder_stand_in(Path::new(env!("CARGO_MANIFEST_DIR")), &root).unwrap();
        let mut start = [0_u8; 4];
        std::io::Read::read_exact(&mut File::open(&file).unwrap(), &mut start).unwrap();
        let bytes = std::fs::metadata(&file).unwrap().len();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(&start, b"GGUF");
        assert_eq!(bytes, 445_521_952);
    }

    #[test]
    fn a_damaged_or_missing_head_is_an_error() {
        let root = std::env::temp_dir().join(format!("stand-in-bad-{}", std::process::id()));
        std::fs::create_dir_all(root.join("resources")).unwrap();
        let missing = speaker_decoder_stand_in(&root, &root)
            .unwrap_err()
            .to_string();
        std::fs::write(root.join("resources/speaker-decoder-head.gguf"), b"GGUF").unwrap();
        let damaged = speaker_decoder_stand_in(&root, &root)
            .unwrap_err()
            .to_string();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            missing.starts_with("read the speaker decoder: "),
            "{missing}"
        );
        assert!(
            damaged.ends_with("is damaged; reinstall the app"),
            "{damaged}"
        );
    }

    #[test]
    fn audio_that_fits_one_request_goes_whole() {
        assert_eq!(speaker_spans(3600.0), vec![(0.0, 3600.0)]);
        assert_eq!(speaker_spans(0.0), vec![(0.0, 0.0)]);
    }

    #[test]
    fn long_audio_goes_in_overlapping_requests() {
        assert_eq!(
            speaker_spans(7000.0),
            vec![(0.0, 3600.0), (3300.0, 6900.0), (6600.0, 7000.0)]
        );
    }

    fn voice(start: f64, end: f64, speaker: u32) -> SpeakerTurn {
        SpeakerTurn {
            start,
            end,
            speaker,
        }
    }

    #[test]
    fn requests_agree_on_who_is_who_through_their_overlap() {
        let parts = vec![
            (
                (0.0, 100.0),
                vec![
                    voice(0.0, 50.0, 1),
                    voice(70.0, 80.0, 1),
                    voice(85.0, 100.0, 2),
                ],
            ),
            (
                (60.0, 160.0),
                vec![
                    voice(68.0, 82.0, 2),
                    voice(84.0, 110.0, 1),
                    voice(150.0, 160.0, 3),
                ],
            ),
        ];
        assert_eq!(
            stitched_turns(parts),
            vec![
                voice(0.0, 50.0, 1),
                voice(70.0, 80.0, 1),
                voice(80.0, 82.0, 1),
                voice(84.0, 110.0, 2),
                voice(150.0, 160.0, 3),
            ]
        );
    }

    #[test]
    fn a_single_request_keeps_its_turns_and_numbers() {
        let parts = vec![((0.0, 10.0), vec![voice(1.0, 2.0, 2), voice(1.5, 3.0, 1)])];
        assert_eq!(
            stitched_turns(parts),
            vec![voice(1.0, 2.0, 2), voice(1.5, 3.0, 1)]
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

    fn heard(from: f64, to: f64, text: &str) -> HeardSegment {
        HeardSegment {
            from,
            to,
            text: text.chars().collect(),
        }
    }

    #[test]
    fn the_same_sentence_again_is_a_loop_with_a_margin_around_it() {
        let segments = [
            heard(5.0, 6.0, "はいどうも"),
            heard(10.0, 12.0, "おまかに入ってくる"),
            heard(12.0, 14.0, "おまかに入ってくる"),
            heard(14.0, 16.0, "おまかに入ってくる"),
            heard(30.0, 31.0, "さようなら"),
        ];
        assert_eq!(looped_ranges(&segments), vec![(8.0, 18.0)]);
    }

    #[test]
    fn a_short_word_said_twice_is_speech_and_three_times_a_loop() {
        let twice = [heard(1.0, 2.0, "そう"), heard(2.0, 3.0, "そう")];
        assert_eq!(looped_ranges(&twice), Vec::<TimeRange>::new());
        let thrice = [
            heard(4.0, 5.0, "そう"),
            heard(5.0, 6.0, "そう"),
            heard(6.0, 7.0, "そう"),
        ];
        assert_eq!(looped_ranges(&thrice), vec![(2.0, 9.0)]);
    }

    #[test]
    fn a_repetition_inside_one_segment_is_a_loop() {
        let segments = [heard(0.5, 9.0, "はいチェイサ牛乳チェイサ牛乳チェイサ牛乳")];
        assert_eq!(looped_ranges(&segments), vec![(0.0, 11.0)]);
    }

    #[test]
    fn empty_text_and_two_short_repeats_inside_a_segment_are_no_loop() {
        let segments = [
            heard(1.0, 2.0, ""),
            heard(2.0, 3.0, ""),
            heard(3.0, 4.0, ""),
            heard(5.0, 6.0, "そうそうそう"),
            heard(7.0, 8.0, "牛乳チェイサ牛乳チェイサ"),
        ];
        assert_eq!(looped_ranges(&segments), Vec::<TimeRange>::new());
    }

    #[test]
    fn loops_close_together_become_one_range() {
        let segments = [
            heard(10.0, 11.0, "はいどうも"),
            heard(11.0, 12.0, "はいどうも"),
            heard(13.0, 14.0, "さようなら"),
            heard(14.0, 15.0, "さようなら"),
            heard(40.0, 41.0, "もう一回やる"),
            heard(41.0, 42.0, "もう一回やる"),
        ];
        assert_eq!(looped_ranges(&segments), vec![(8.0, 17.0), (38.0, 44.0)]);
    }

    #[test]
    fn a_segments_text_is_judged_without_spaces_or_punctuation() {
        let json = br#"{"transcription":[
            {"offsets":{"from":1000,"to":2000},"text":" \u00e3\u0081\u00af\u00e3\u0080\u0081 a"},
            {"text":"no bounds"}
        ]}"#;
        let segments = whisper_segments(json).unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!((segments[0].from, segments[0].to), (1.0, 2.0));
        assert_eq!(segments[0].text, vec!['は', 'a']);
        assert_eq!(
            whisper_segments(b"{").unwrap_err().to_string(),
            "whisper JSON: EOF while parsing an object at line 1 column 1"
        );
        assert_eq!(
            whisper_segments(b"{}").unwrap_err().to_string(),
            "whisper JSON has no transcription"
        );
    }

    #[test]
    fn redone_words_replace_the_looped_ones_on_the_recordings_clock() {
        let words = vec![
            timed(1.0, 2.0, "前"),
            timed(11.0, 12.0, "ループ"),
            timed(13.0, 14.0, "ループ"),
            timed(30.0, 31.0, "後"),
        ];
        let redone = vec![vec![timed(1.0, 1.25, "本"), timed(3.0, 3.25, "物")]];
        let joined = with_redone_ranges(words, &[(10.0, 20.0)], Ok(redone)).unwrap();
        assert_eq!(
            joined
                .iter()
                .map(|word| (word.text.as_str(), word.start))
                .collect::<Vec<_>>(),
            vec![("前", 1.0), ("本", 11.0), ("物", 13.0), ("後", 30.0)]
        );
        assert_eq!(joined[0].end, 1.25);
    }

    #[test]
    fn with_nothing_to_redo_the_words_stay_exactly_as_heard() {
        let words = vec![timed(1.0, 9.0, "前")];
        let kept = with_redone_ranges(words, &[], Ok(Vec::new())).unwrap();
        assert_eq!(kept, vec![timed(1.0, 9.0, "前")]);
    }

    #[test]
    fn a_failed_redo_keeps_the_first_pass_but_a_cancel_stops() {
        let words = vec![timed(1.0, 2.0, "前")];
        let kept = with_redone_ranges(
            words.clone(),
            &[(0.0, 5.0)],
            Err(AppError::Invalid("no audio".into())),
        );
        assert_eq!(kept.unwrap(), vec![timed(1.0, 2.0, "前")]);
        let stopped = with_redone_ranges(words, &[(0.0, 5.0)], Err(AppError::Cancelled));
        assert_eq!(stopped.unwrap_err().to_string(), "cancelled");
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
