//! Drives the bundled tools: ffmpeg/ffprobe for media, whisper.cpp for words, nemo-speech for speakers.

use crate::captions::{SpeakerTurn, VoiceSpan, Word};
use crate::catalog::{Backend, DiarizeKind, ModelEntry};
use crate::error::{io_context, AppError, AppResult, UserError};
use crate::paths::AppPaths;
use crate::process::{
    background_command, run_capture, run_with_progress, run_with_ticks, CancelToken,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

const TOOL_SEARCH_DEPTH: usize = 3;
const EXE_SUFFIX: &str = if cfg!(windows) { ".exe" } else { "" };
const AUTO_LANGUAGE: &str = "auto";
const AUDIO_ARGS: [&str; 8] = ["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le", "-y"];
const MILLIS_PER_SECOND: f64 = 1000.0;
/// Dynamic time warping over the decoder's attention gives each token a time of its own (mean
/// error 0.22 s against 0.52 s for plain token timestamps, measured against Nemotron). whisper.cpp
/// needs flash attention off for it, which also makes the text worse (similarity to Nemotron's
/// 0.82 against 0.87), so one pass gives the text and a second pass only its times.
const TOKEN_TIMING_ARGS: [&str; 3] = ["-nfa", "-dtw", "large.v3.turbo"];
/// Speech stays above about five syllables a second: a word after a pause started at most this
/// long per beat before its end.
const LONGEST_ONSET_SECONDS_PER_CHARACTER: f64 = 0.2;
const KANJI_BEATS: usize = 2;
/// Words on each side that tell how fast the speech around a word is.
const LOCAL_RATE_WORDS: usize = 5;
/// How far the two passes' texts may drift apart while being matched, in characters.
const ALIGNMENT_BAND_CHARACTERS: usize = 200;
const STEP_MATCH: u8 = 1;
const STEP_UP: u8 = 2;
const STEP_LEFT: u8 = 3;
/// `t_dtw` counts in hundredths of a second.
const DTW_TICKS_PER_SECOND: f64 = 100.0;
const CENTISECONDS_PER_SECOND: f64 = 100.0;
const VOICE_MODEL: &str = "ggml-silero-v6.2.0.bin";
/// A word timed by warping has only a start; it is said in at most this much per character, so the
/// last word before a pause does not last through the pause.
const LONGEST_SPOKEN_CHARACTER_SECONDS: f64 = 0.25;
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
    wav: &'a Path,
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

/// Warping is reliable inside its segment; a segment's first and last tokens can land seconds
/// outside it (64 s for a token of a segment ending at 62.4 s), so they come back: the first to the
/// segment's start, the last to just after the token before it.
fn warped_start(warped: f64, bounds: Option<(f64, f64)>, previous: Option<f64>) -> f64 {
    let Some((from, to)) = bounds else {
        return warped;
    };
    if warped < from {
        return from;
    }
    if warped <= to {
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
/// characters have been said, whichever comes first. Unwarped words keep their own end.
fn close_warped_words(words: &mut [Word]) {
    let mut latest = 0.0_f64;
    for word in words.iter_mut() {
        let length = word.end - word.start;
        word.start = word.start.max(latest);
        word.end = word.start + length.max(0.0);
        latest = word.start;
    }
    for index in 0..words.len() {
        if words[index].end > words[index].start {
            continue;
        }
        let characters = words[index].text.trim().chars().count() as f64;
        let said = words[index].start + characters * LONGEST_SPOKEN_CHARACTER_SECONDS;
        words[index].end = words
            .get(index + 1)
            .map_or(said, |next| next.start.min(said));
    }
}

fn run_whisper(
    program: &Path,
    run: &WhisperRun<'_>,
    extra_args: &[&str],
    base: &Path,
    progress: &mut dyn FnMut(f64),
) -> AppResult<Vec<Word>> {
    let mut command = background_command(program);
    command
        .arg("-m")
        .arg(run.model)
        .arg("-f")
        .arg(run.wav)
        .args(["-l", run.language, "-ojf", "-pp"])
        .args(extra_args)
        .arg("-of")
        .arg(base);
    run_with_progress(command, "whisper-cli", run.cancel, |line| {
        if let Some(fraction) = parse_whisper_progress(line) {
            progress(fraction);
        }
    })?;
    let json =
        std::fs::read(base.with_extension("json")).map_err(io_context("read whisper output"))?;
    parse_whisper_json(&json)
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
/// as a driver that crashes or a CPU missing an instruction. Returns the program that worked.
fn with_cpu_fallback<T>(
    tools: &Tools,
    mut pass: impl FnMut(&Path) -> AppResult<T>,
) -> AppResult<(T, &Path)> {
    let failure = match pass(&tools.whisper) {
        Ok(value) => return Ok((value, &tools.whisper)),
        Err(failure) => failure,
    };
    match (&failure, &tools.whisper_cpu) {
        (AppError::Tool { .. }, Some(cpu)) => Ok((pass(cpu)?, cpu)),
        _ => Err(failure),
    }
}

/// Runs whisper.cpp twice: the text from its best decoding, the times from warping. The second pass
/// uses whichever build gave the text, so a build that failed is not started again.
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
        wav,
        language,
        cancel,
    };
    let (words, program) = with_cpu_fallback(tools, |program| {
        run_whisper(program, &run, &[], &work.join("asr"), &mut |fraction| {
            progress(fraction / 2.0);
        })
    })?;
    let timing = run_whisper(
        program,
        &run,
        &TOKEN_TIMING_ARGS,
        &work.join("asr-timing"),
        &mut |fraction| {
            progress(0.5 + fraction / 2.0);
        },
    );
    with_warped_times(words, timing)
}

/// The text's words on the warped clock. A timing pass that failed leaves plain token times, which
/// are coarse but usable; only cancelling stops the job.
fn with_warped_times(mut words: Vec<Word>, timing: AppResult<Vec<Word>>) -> AppResult<Vec<Word>> {
    match timing {
        Ok(timed) => {
            retime_words(&mut words, &timed);
            Ok(words)
        }
        Err(AppError::Cancelled) => Err(AppError::Cancelled),
        Err(_) => Ok(words),
    }
}

/// One character's time for each spoken character of `words`.
fn timed_characters(words: &[Word]) -> Vec<(char, f64)> {
    let mut characters = Vec::new();
    for word in words {
        let spoken: Vec<char> = word.text.chars().filter(|c| !c.is_whitespace()).collect();
        let step = (word.end - word.start) / spoken.len().max(1) as f64;
        characters.extend(
            spoken
                .into_iter()
                .enumerate()
                .map(|(index, c)| (c, word.start + step * index as f64)),
        );
    }
    characters
}

/// For each character of `text`, the index of the character of `other` it matches: a longest common
/// subsequence inside a band, since both are decodings of the same audio.
fn matched_characters(text: &[char], other: &[char]) -> Vec<Option<usize>> {
    let (rows, columns) = (text.len(), other.len());
    let band = ALIGNMENT_BAND_CHARACTERS + rows.abs_diff(columns);
    let width = 2 * band + 1;
    let mut steps = vec![0_u8; (rows + 1) * width];
    let mut previous = vec![0_u32; width + 2];
    for row in 1..=rows {
        let mut current = vec![0_u32; width + 2];
        for slot in 0..width {
            let Some(column) = (row + slot)
                .checked_sub(band)
                .filter(|c| *c >= 1 && *c <= columns)
            else {
                continue;
            };
            let up = previous[slot + 2];
            let left = current[slot];
            let diagonal = previous[slot + 1] + u32::from(text[row - 1] == other[column - 1]);
            let (best, step) = if diagonal >= up.max(left) && text[row - 1] == other[column - 1] {
                (diagonal, STEP_MATCH)
            } else if up >= left {
                (up, STEP_UP)
            } else {
                (left, STEP_LEFT)
            };
            current[slot + 1] = best;
            steps[row * width + slot] = step;
        }
        previous = current;
    }
    let mut matched = vec![None; rows];
    let (mut row, mut column) = (rows, columns);
    while row > 0 && column > 0 {
        let slot = column + band - row;
        match steps
            .get(row * width + slot)
            .copied()
            .filter(|_| slot < width)
        {
            Some(STEP_MATCH) => {
                matched[row - 1] = Some(column - 1);
                row -= 1;
                column -= 1;
            }
            Some(STEP_LEFT) => column -= 1,
            _ => row -= 1,
        }
    }
    matched
}

/// Moves `words` onto the times of `timed`. Characters only one pass heard share the time between
/// their matched neighbours.
fn retime_words(words: &mut [Word], timed: &[Word]) {
    let other = timed_characters(timed);
    if other.is_empty() {
        return;
    }
    let own: Vec<char> = words
        .iter()
        .flat_map(|word| word.text.chars())
        .filter(|c| !c.is_whitespace())
        .collect();
    let keys: Vec<char> = other.iter().map(|(c, _)| *c).collect();
    let matched = matched_characters(&own, &keys);
    let anchors: Vec<(usize, f64)> = matched
        .iter()
        .enumerate()
        .filter_map(|(index, found)| found.map(|at| (index, other[at].1)))
        .collect();
    let mut position = 0;
    for word in words.iter_mut() {
        word.start = time_at(position, &anchors).unwrap_or(word.start);
        word.end = word.start;
        position += word.text.chars().filter(|c| !c.is_whitespace()).count();
    }
    close_warped_words(words);
}

/// The time of character `position`, between the anchors around it.
fn time_at(position: usize, anchors: &[(usize, f64)]) -> Option<f64> {
    let after = anchors.partition_point(|(index, _)| *index < position);
    match (
        after.checked_sub(1).map(|at| anchors[at]),
        anchors.get(after),
    ) {
        (_, Some(&(index, time))) if index == position => Some(time),
        (Some((before, from)), Some(&(next, to))) => {
            Some(from + (to - from) * (position - before) as f64 / (next - before) as f64)
        }
        (Some((_, from)), None) => Some(from),
        (None, Some(&(_, to))) => Some(to),
        (None, None) => None,
    }
}

fn speaker_number(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|number| u32::try_from(number).ok())
        .filter(|number| *number > 0)
}

/// `nemo-speech diarize --format json`: `{"segments":[{"start","end","speaker"}]}`.
pub fn parse_diarization(bytes: &[u8]) -> AppResult<Vec<SpeakerTurn>> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| AppError::Invalid(format!("diarization JSON: {error}")))?;
    let segments = value["segments"]
        .as_array()
        .ok_or_else(|| AppError::Invalid("diarization JSON has no segments".into()))?;
    segments
        .iter()
        .map(|segment| {
            let (start, end) = (segment["start"].as_f64(), segment["end"].as_f64());
            match (start, end, speaker_number(&segment["speaker"])) {
                (Some(start), Some(end), Some(speaker)) if start >= 0.0 && end > start => {
                    Ok(SpeakerTurn {
                        start,
                        end,
                        speaker,
                    })
                }
                _ => Err(AppError::Invalid(format!(
                    "bad diarization segment: {segment}"
                ))),
            }
        })
        .collect()
}

fn word_to_turn(word: &Value) -> Option<SpeakerTurn> {
    let (start, end) = (word["start"].as_f64()?, word["end"].as_f64()?);
    Some(SpeakerTurn {
        start,
        end: end.max(start),
        speaker: speaker_number(&word["speaker"])?,
    })
}

/// `nemo-speech transcribe --diarize --format json`: per-word speakers, merged into turns.
pub fn parse_speech_llm(bytes: &[u8]) -> AppResult<Vec<SpeakerTurn>> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| AppError::Invalid(format!("speech-LLM JSON: {error}")))?;
    let words = value["words"]
        .as_array()
        .ok_or_else(|| AppError::Invalid("speech-LLM JSON has no words".into()))?;
    let mut turns: Vec<SpeakerTurn> = Vec::new();
    for turn in words.iter().filter_map(word_to_turn) {
        match turns.last_mut() {
            Some(last) if last.speaker == turn.speaker => last.end = last.end.max(turn.end),
            _ => turns.push(turn),
        }
    }
    Ok(turns)
}

/// Decoder and encoder paths of a speech-LLM model.
#[derive(Clone, Debug)]
pub struct DiarizeModel<'a> {
    pub entry: &'a ModelEntry,
    pub main_file: &'a Path,
    pub encoder: Option<&'a Path>,
    /// `--device`: the GPU when the model fits, else the CPU.
    pub device: &'a str,
    /// How many people speak, 0 when unknown.
    pub speaker_count: u32,
    /// A lower speaker activation threshold, for a retry when too few voices were found.
    pub onset: Option<f64>,
}

/// The prompt guide's speaker instruction, naming exactly the tags for the people known to be speaking.
fn speaker_instruction(speaker_count: u32) -> Option<String> {
    (speaker_count >= 2).then(|| {
        let tags: Vec<String> = (0..speaker_count).map(|index| format!("<spk:{index}>")).collect();
        format!(
            "Transcribe this audio with punctuation and capitalization. Include all spoken words and verbal fillers. Write numbers in their standard written form. Put speaker tags {} on the transcription.",
            tags.join(" ")
        )
    })
}

fn diarize_command(
    tools: &Tools,
    model: &DiarizeModel<'_>,
    wav: &Path,
    language: &str,
    out: &Path,
) -> AppResult<std::process::Command> {
    let kind = model.entry.diarize_kind.ok_or_else(|| {
        AppError::Invalid(format!("{} is not a diarization model", model.entry.id))
    })?;
    let mut command = background_command(&tools.nemo);
    match kind {
        DiarizeKind::Sortformer => {
            command
                .arg("diarize")
                .arg(wav)
                .arg("--model")
                .arg(model.main_file)
                .args(["--format", "json", "--force"]);
            if let Some(onset) = model.onset {
                command.arg("--onset").arg(onset.to_string());
            }
        }
        DiarizeKind::SpeechLlm => {
            command
                .arg("transcribe")
                .arg(wav)
                .arg("--model")
                .arg(model.main_file)
                .args(["--format", "json", "--diarize", "--force"]);
            if language != AUTO_LANGUAGE {
                command.args(["--language", language]);
            }
            if let Some(instruction) = speaker_instruction(model.speaker_count) {
                command.arg("--instruction").arg(instruction);
            }
            if let Some(encoder) = model.encoder {
                command.arg("--salm-encoder").arg(encoder);
            }
            command.args(model.entry.extra_args.iter().map(OsStr::new));
        }
    }
    command
        .args(["--device", model.device])
        .arg("--output")
        .arg(out);
    Ok(command)
}

/// Runs the chosen speaker model; it reports no percentage, so callers show elapsed time.
pub fn diarize(
    tools: &Tools,
    model: &DiarizeModel<'_>,
    wav: &Path,
    language: &str,
    work: &Path,
    cancel: &CancelToken,
    on_tick: &mut dyn FnMut(),
) -> AppResult<Vec<SpeakerTurn>> {
    let out = work.join("speakers.json");
    let command = diarize_command(tools, model, wav, language, &out)?;
    run_with_ticks(command, "nemo-speech", cancel, on_tick)?;
    let bytes = std::fs::read(&out).map_err(io_context("read speaker output"))?;
    match model.entry.diarize_kind {
        Some(DiarizeKind::SpeechLlm) => parse_speech_llm(&bytes),
        _ => parse_diarization(&bytes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn diarization_segments_parse_and_reject_bad_times() {
        let turns = parse_diarization(br#"{"segments":[{"start":0.0,"end":4.2,"speaker":1},{"start":4.4,"end":8.3,"speaker":2}]}"#).unwrap();
        assert_eq!(
            turns,
            vec![
                SpeakerTurn {
                    start: 0.0,
                    end: 4.2,
                    speaker: 1
                },
                SpeakerTurn {
                    start: 4.4,
                    end: 8.3,
                    speaker: 2
                },
            ]
        );
        assert!(parse_diarization(br#"{"segments":[{"start":5,"end":4,"speaker":1}]}"#).is_err());
        assert!(parse_diarization(br#"{"nothing":[]}"#).is_err());
    }

    #[test]
    fn speech_llm_words_merge_into_turns() {
        let json = br#"{"words":[
            {"word":"a","start":0.0,"end":0.5,"speaker":1},
            {"word":"b","start":0.5,"end":1.0,"speaker":1},
            {"word":"c","start":1.2,"end":1.5,"speaker":2},
            {"word":"d","start":1.6,"end":1.7}
        ]}"#;
        assert_eq!(
            parse_speech_llm(json).unwrap(),
            vec![
                SpeakerTurn {
                    start: 0.0,
                    end: 1.0,
                    speaker: 1
                },
                SpeakerTurn {
                    start: 1.2,
                    end: 1.5,
                    speaker: 2
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
                fps_den: 1001
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
                fps_den: 1
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
            parse_diarization(b"{").unwrap_err().to_string(),
            "diarization JSON: EOF while parsing an object at line 1 column 1"
        );
        assert_eq!(
            parse_speech_llm(b"{").unwrap_err().to_string(),
            "speech-LLM JSON: EOF while parsing an object at line 1 column 1"
        );
        assert_eq!(
            parse_speech_llm(b"{}").unwrap_err().to_string(),
            "speech-LLM JSON has no words"
        );
    }

    fn args_of(command: &std::process::Command) -> Vec<String> {
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

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
        let (value, program) = with_cpu_fallback(&tools, |program| {
            started.push(program.to_path_buf());
            Ok(7)
        })
        .unwrap();
        assert_eq!((value, program), (7, Path::new("gpu")));
        assert_eq!(started, vec![PathBuf::from("gpu")]);
    }

    #[test]
    fn a_crashed_gpu_build_falls_back_to_the_cpu_build() {
        let tools = gpu_tools(Some("cpu"));
        let mut started = Vec::new();
        let (value, program) = with_cpu_fallback(&tools, |program| {
            started.push(program.to_path_buf());
            if program == Path::new("gpu") {
                Err(crashed())
            } else {
                Ok(7)
            }
        })
        .unwrap();
        assert_eq!((value, program), (7, Path::new("cpu")));
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
    fn a_sortformer_model_is_driven_by_the_diarize_command() {
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
            entry: crate::catalog::find_entry("nemotron-3-diarization").unwrap(),
            main_file: Path::new("m.gguf"),
            encoder: None,
            device: "cpu",
            speaker_count: 0,
            onset: None,
        };
        let command = diarize_command(
            &tools,
            &model,
            Path::new("a.wav"),
            "ja",
            Path::new("o.json"),
        )
        .unwrap();
        assert_eq!(
            args_of(&command),
            vec![
                "diarize", "a.wav", "--model", "m.gguf", "--format", "json", "--force", "--device",
                "cpu", "--output", "o.json"
            ]
        );
    }

    #[test]
    fn a_speech_llm_gets_its_language_encoder_and_extra_arguments() {
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
            speaker_count: 0,
            onset: None,
        };
        let command = diarize_command(
            &tools,
            &model,
            Path::new("a.wav"),
            "ja",
            Path::new("o.json"),
        )
        .unwrap();
        assert_eq!(
            args_of(&command),
            vec![
                "transcribe",
                "a.wav",
                "--model",
                "m.gguf",
                "--format",
                "json",
                "--diarize",
                "--force",
                "--language",
                "ja",
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
                "--output",
                "o.json"
            ]
        );
    }

    #[test]
    fn an_automatic_language_is_left_to_the_model() {
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
            device: "cpu",
            speaker_count: 0,
            onset: None,
        };
        let command = diarize_command(
            &tools,
            &model,
            Path::new("a.wav"),
            "auto",
            Path::new("o.json"),
        )
        .unwrap();
        assert_eq!(
            args_of(&command),
            vec![
                "transcribe",
                "a.wav",
                "--model",
                "m.gguf",
                "--format",
                "json",
                "--diarize",
                "--force",
                "--device",
                "cpu",
                "--output",
                "o.json"
            ]
        );
    }

    #[test]
    fn a_retry_lowers_the_sortformer_onset() {
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
            entry: crate::catalog::find_entry("nemotron-3-diarization").unwrap(),
            main_file: Path::new("m.gguf"),
            encoder: None,
            device: "auto",
            speaker_count: 3,
            onset: Some(0.3),
        };
        let command = diarize_command(
            &tools,
            &model,
            Path::new("a.wav"),
            "ja",
            Path::new("o.json"),
        )
        .unwrap();
        assert_eq!(
            args_of(&command),
            vec![
                "diarize", "a.wav", "--model", "m.gguf", "--format", "json", "--force", "--onset",
                "0.3", "--device", "auto", "--output", "o.json"
            ]
        );
    }

    #[test]
    fn a_known_speaker_count_names_exactly_those_tags() {
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
            speaker_count: 2,
            onset: None,
        };
        let command = diarize_command(
            &tools,
            &model,
            Path::new("a.wav"),
            "auto",
            Path::new("o.json"),
        )
        .unwrap();
        assert_eq!(
            args_of(&command),
            vec![
                "transcribe", "a.wav", "--model", "m.gguf", "--format", "json", "--diarize", "--force",
                "--instruction",
                "Transcribe this audio with punctuation and capitalization. Include all spoken words and verbal fillers. Write numbers in their standard written form. Put speaker tags <spk:0> <spk:1> on the transcription.",
                "--device", "auto", "--output", "o.json"
            ]
        );
    }

    #[test]
    fn an_unknown_or_single_speaker_count_needs_no_instruction() {
        assert_eq!(speaker_instruction(0), None);
        assert_eq!(speaker_instruction(1), None);
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
        assert_eq!(warped_start(3.0, Some((4.0, 6.0)), None), 4.0);
        assert_eq!(warped_start(8.0, Some((4.0, 6.0)), Some(4.5)), 4.75);
        assert_eq!(warped_start(8.0, Some((4.0, 6.0)), Some(5.9)), 6.0);
        assert_eq!(warped_start(8.0, Some((4.0, 6.0)), None), 6.0);
        assert_eq!(warped_start(8.0, None, None), 8.0);
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
    fn the_text_keeps_its_words_and_takes_the_warped_times() {
        let mut words = vec![
            timed(0.0, 9.0, "味濃い"),
            timed(9.0, 9.5, " じゃん"),
            timed(9.5, 12.0, "です"),
        ];
        let warped = [
            timed(1.0, 1.75, "味濃い"),
            timed(2.0, 2.75, "じゃん"),
            timed(5.0, 5.5, "です"),
        ];
        retime_words(&mut words, &warped);
        assert_eq!(
            words
                .iter()
                .map(|word| (word.text.as_str(), word.start, word.end))
                .collect::<Vec<_>>(),
            vec![
                ("味濃い", 1.0, 1.75),
                (" じゃん", 2.0, 2.75),
                ("です", 5.0, 5.5)
            ]
        );
    }

    #[test]
    fn words_only_the_text_pass_heard_share_the_time_between_their_neighbours() {
        let mut words = vec![
            timed(0.0, 1.0, "はい"),
            timed(1.0, 2.0, "味濃"),
            timed(2.0, 3.0, "です"),
        ];
        let warped = [timed(1.0, 1.5, "はい"), timed(6.0, 6.5, "です")];
        retime_words(&mut words, &warped);
        assert_eq!((words[1].start * 100.0).round(), 283.0);
        assert_eq!(words[2].start, 6.0);
    }

    #[test]
    fn words_before_and_after_every_match_take_the_nearest_time() {
        assert_eq!(time_at(0, &[(2, 5.0), (4, 6.0)]), Some(5.0));
        assert_eq!(time_at(3, &[(2, 5.0), (4, 6.0)]), Some(5.5));
        assert_eq!(time_at(4, &[(2, 5.0), (4, 6.0)]), Some(6.0));
        assert_eq!(time_at(9, &[(2, 5.0), (4, 6.0)]), Some(6.0));
        assert_eq!(time_at(1, &[]), None);
    }

    #[test]
    fn nothing_in_common_leaves_the_plain_times() {
        let mut words = vec![timed(1.0, 2.0, "はい")];
        retime_words(&mut words, &[timed(5.0, 6.0, "abc")]);
        assert_eq!(words[0].start, 1.0);
    }

    #[test]
    fn an_empty_timing_pass_changes_nothing() {
        let mut words = vec![timed(1.0, 2.0, "はい")];
        retime_words(&mut words, &[]);
        assert_eq!((words[0].start, words[0].end), (1.0, 2.0));
    }

    #[test]
    fn matching_tolerates_texts_of_different_lengths_and_order() {
        let text: Vec<char> = "あいうえお".chars().collect();
        let other: Vec<char> = "xあうxxお".chars().collect();
        assert_eq!(
            matched_characters(&text, &other),
            vec![Some(1), None, Some(2), None, Some(5)]
        );
        assert_eq!(matched_characters(&[], &other), Vec::<Option<usize>>::new());
        assert_eq!(matched_characters(&text, &[]), vec![None; 5]);
    }

    #[test]
    fn a_failed_timing_pass_keeps_the_plain_times_but_a_cancel_stops() {
        let words = vec![timed(1.0, 2.0, "はい")];
        let kept = with_warped_times(words.clone(), Err(AppError::Invalid("no dtw".into())));
        assert_eq!(kept.unwrap(), words);
        let cancelled = with_warped_times(words, Err(AppError::Cancelled));
        assert_eq!(cancelled.unwrap_err().to_string(), "cancelled");
    }

    #[test]
    fn a_good_timing_pass_is_used() {
        let words = vec![timed(1.0, 2.0, "は")];
        let kept = with_warped_times(words, Ok(vec![timed(5.0, 5.2, "は")])).unwrap();
        assert_eq!((kept[0].start, kept[0].end), (5.0, 5.25));
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
}
