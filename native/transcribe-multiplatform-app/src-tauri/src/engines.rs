//! Drives the bundled tools: ffmpeg/ffprobe for media, whisper.cpp for words, nemo-speech for speakers.

use crate::captions::{SpeakerTurn, Word};
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
const FALLBACK_FPS: (u32, u32) = (30, 1);
const QUARTER_TURN_DEGREES: i64 = 90;

/// Absolute paths of the tools one job uses.
#[derive(Clone, Debug)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub whisper: PathBuf,
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
            whisper: whisper.map_or_else(|| plain("whisper", "whisper-cli"), Ok)?,
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

    #[test]
    fn a_sortformer_model_is_driven_by_the_diarize_command() {
        let tools = Tools {
            ffmpeg: "f".into(),
            ffprobe: "p".into(),
            whisper: "w".into(),
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
}
