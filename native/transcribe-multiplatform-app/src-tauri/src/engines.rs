//! Drives the bundled tools: ffmpeg/ffprobe for media, whisper.cpp for words, nemo-speech for speakers.

use crate::captions::{SpeakerTurn, Word};
use crate::catalog::{DiarizeKind, ModelEntry};
use crate::error::{io_context, AppError, AppResult, UserError};
use crate::paths::AppPaths;
use crate::process::{background_command, run_capture, run_with_progress, CancelToken};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

const TOOL_SEARCH_DEPTH: usize = 3;
const EXE_SUFFIX: &str = if cfg!(windows) { ".exe" } else { "" };
const AUTO_LANGUAGE: &str = "auto";
const AUDIO_ARGS: [&str; 8] = ["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le", "-y"];
const MILLIS_PER_SECOND: f64 = 1000.0;
const FALLBACK_FPS: (u32, u32) = (30, 1);
const QUARTER_TURN_DEGREES: i64 = 90;

/// Absolute paths of the tools one job uses.
#[derive(Clone, Debug)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub whisper: PathBuf,
    pub nemo: PathBuf,
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
    /// Bundled tools, with the downloaded CUDA builds taking over when `use_cuda` is set.
    pub fn locate(paths: &AppPaths, use_cuda: bool) -> AppResult<Self> {
        let bundled = paths.bundled_runtime_dir();
        let geforce = paths.downloaded_runtime_dir().join("geforce");
        let resolve = |folder: &str, tool: &str| -> AppResult<PathBuf> {
            let file_name = exe_name(tool);
            let accelerated = use_cuda
                .then(|| find_tool(&geforce.join(folder), &file_name, TOOL_SEARCH_DEPTH))
                .flatten();
            accelerated
                .or_else(|| find_tool(&bundled.join(folder), &file_name, TOOL_SEARCH_DEPTH))
                .ok_or_else(|| {
                    AppError::Invalid(format!("{tool} is missing from {}", bundled.display()))
                })
        };
        Ok(Self {
            ffmpeg: resolve("ffmpeg", "ffmpeg")?,
            ffprobe: resolve("ffmpeg", "ffprobe")?,
            whisper: resolve("whisper", "whisper-cli")?,
            nemo: resolve("nemo", "nemo-speech")?,
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

struct TokenMerger {
    bytes: Vec<u8>,
    start_ms: f64,
}

impl TokenMerger {
    /// Returns a word once the buffered bytes form complete UTF-8.
    fn push(&mut self, text: &[u8], from_ms: f64, to_ms: f64) -> Option<Word> {
        if self.bytes.is_empty() {
            self.start_ms = from_ms;
        }
        self.bytes.extend_from_slice(text);
        let decoded = std::str::from_utf8(&self.bytes).ok()?;
        let word = Word {
            start: self.start_ms / MILLIS_PER_SECOND,
            end: to_ms.max(self.start_ms) / MILLIS_PER_SECOND,
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
        start_ms: 0.0,
    };
    let mut words = Vec::new();
    for token in segments
        .iter()
        .filter_map(|segment| segment["tokens"].as_array())
        .flatten()
    {
        let text = latin1_bytes(token["text"].as_str().unwrap_or(""));
        if text.is_empty() || is_control_token(&text) {
            continue;
        }
        let from = token["offsets"]["from"].as_f64().unwrap_or(0.0);
        let to = token["offsets"]["to"].as_f64().unwrap_or(from);
        words.extend(merger.push(&text, from, to));
    }
    Ok(words)
}

pub fn transcribe(
    tools: &Tools,
    model: &Path,
    wav: &Path,
    language: &str,
    work: &Path,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f64),
) -> AppResult<Vec<Word>> {
    let base = work.join("asr");
    let mut command = background_command(&tools.whisper);
    command
        .arg("-m")
        .arg(model)
        .arg("-f")
        .arg(wav)
        .args(["-l", language, "-ojf", "-pp", "-of"])
        .arg(&base);
    run_with_progress(command, "whisper-cli", cancel, |line| {
        if let Some(fraction) = parse_whisper_progress(line) {
            progress(fraction);
        }
    })?;
    let json =
        std::fs::read(base.with_extension("json")).map_err(io_context("read whisper output"))?;
    parse_whisper_json(&json)
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
            if let Some(encoder) = model.encoder {
                command.arg("--salm-encoder").arg(encoder);
            }
            command.args(model.entry.extra_args.iter().map(OsStr::new));
        }
    }
    command.arg("--output").arg(out);
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
) -> AppResult<Vec<SpeakerTurn>> {
    let out = work.join("speakers.json");
    let command = diarize_command(tools, model, wav, language, &out)?;
    run_with_progress(command, "nemo-speech", cancel, |_| {})?;
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
}
