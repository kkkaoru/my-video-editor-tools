//! Writes one transcript as the subtitle formats DaVinci Resolve reads. No Resolve API is involved:
//! the JSON is the source of truth (report §7.1), the others are generated from it.

use crate::captions::{Cue, Metrics, Word};
use crate::engines::VideoInfo;
use crate::error::{AppError, AppResult};
use crate::face::FaceSample;
use crate::paths::write_atomic;
use crate::settings::{Settings, SpeakerColor};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const FORMAT_VERSION: u32 = 1;
const MILLIS_PER_HOUR: u64 = 3_600_000;
const MILLIS_PER_MINUTE: u64 = 60_000;
const MILLIS_PER_SECOND: u64 = 1000;
const BASIC_TITLE_UID: &str = ".../Titles.localized/Bumper:Opener.localized/Basic Title.localized/Basic Title.moti";
/// Basic Title's position parameter key, in pixels from the frame centre.
const TITLE_POSITION_KEY: &str = "9999/999166631/999166633/1/100/101";
const FCPXML_VERSION: &str = "1.10";

/// Everything needed to lay captions out again without running the models.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Transcript {
    pub format_version: u32,
    pub source: PathBuf,
    pub info: VideoInfo,
    pub metrics: Metrics,
    pub speaker_count: u32,
    pub words: Vec<Word>,
    pub faces: Vec<FaceSample>,
    pub cues: Vec<Cue>,
}

impl Transcript {
    #[must_use]
    pub fn new(source: PathBuf, info: VideoInfo, metrics: Metrics, words: Vec<Word>, faces: Vec<FaceSample>, cues: Vec<Cue>) -> Self {
        let speaker_count = words.iter().filter_map(|word| word.speaker).max().unwrap_or(0);
        Self { format_version: FORMAT_VERSION, source, info, metrics, speaker_count, words, faces, cues }
    }

    pub fn load(path: &Path) -> AppResult<Self> {
        let bytes = std::fs::read(path).map_err(crate::error::io_context(format!("read {}", path.display())))?;
        serde_json::from_slice(&bytes).map_err(|error| AppError::Invalid(format!("transcript file: {error}")))
    }
}

fn escape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

fn split_clock(seconds: f64) -> (u64, u64, u64, u64) {
    let total = (seconds.max(0.0) * 1000.0).round() as u64;
    (
        total / MILLIS_PER_HOUR,
        total % MILLIS_PER_HOUR / MILLIS_PER_MINUTE,
        total % MILLIS_PER_MINUTE / MILLIS_PER_SECOND,
        total % MILLIS_PER_SECOND,
    )
}

fn srt_clock(seconds: f64) -> String {
    let (hours, minutes, whole, millis) = split_clock(seconds);
    format!("{hours:02}:{minutes:02}:{whole:02},{millis:03}")
}

fn ttml_clock(seconds: f64) -> String {
    let (hours, minutes, whole, millis) = split_clock(seconds);
    format!("{hours:02}:{minutes:02}:{whole:02}.{millis:03}")
}

/// Colour survives in Resolve's reader only if it honours `<font>`; text and timing always do.
#[must_use]
pub fn render_srt(cues: &[Cue], settings: &Settings) -> String {
    let mut out = String::new();
    for (index, cue) in cues.iter().enumerate() {
        let color = settings.color_for(cue.speaker);
        let text = escape_xml(&cue.lines.join("\n"));
        let _ = write!(
            out,
            "{}\n{} --> {}\n<font color=\"{}\">{text}</font>\n\n",
            index + 1,
            srt_clock(cue.start),
            srt_clock(cue.end),
            color.fill,
        );
    }
    out
}

fn ttml_span(line: &str, color: &SpeakerColor, outline_px: f64) -> String {
    format!(
        "<span tts:color=\"{fill}\" tts:textOutline=\"{outline} {outline_px:.1}px 0px\">{text}</span>",
        fill = color.fill,
        outline = color.outline,
        text = escape_xml(line),
    )
}

fn percent(value: f64, frame: u32) -> f64 {
    value / f64::from(frame) * 100.0
}

/// TTML2 with per-span fill and `tts:textOutline`, positioned by one region per cue.
#[must_use]
pub fn render_ttml(cues: &[Cue], info: &VideoInfo, metrics: &Metrics, settings: &Settings) -> String {
    let mut regions = String::new();
    let mut body = String::new();
    let region_height = metrics.font_px * 3.0;
    for (index, cue) in cues.iter().enumerate() {
        let width = metrics.max_line_px + 2.0 * metrics.outline_px;
        let left = (cue.x - width / 2.0).clamp(0.0, (f64::from(info.width) - width).max(0.0));
        let top = (cue.y - region_height / 2.0).clamp(0.0, (f64::from(info.height) - region_height).max(0.0));
        let _ = writeln!(
            regions,
            "      <region xml:id=\"r{index}\" tts:origin=\"{:.3}% {:.3}%\" tts:extent=\"{:.3}% {:.3}%\" tts:displayAlign=\"center\" tts:textAlign=\"center\"/>",
            percent(left, info.width),
            percent(top, info.height),
            percent(width, info.width),
            percent(region_height, info.height),
        );
        let color = settings.color_for(cue.speaker);
        let spans: Vec<String> = cue.lines.iter().map(|line| ttml_span(line, color, metrics.outline_px)).collect();
        let _ = writeln!(
            body,
            "      <p region=\"r{index}\" begin=\"{}\" end=\"{}\">{}</p>",
            ttml_clock(cue.start),
            ttml_clock(cue.end),
            spans.join("<br/>"),
        );
    }
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<tt xmlns=\"http://www.w3.org/ns/ttml\" xmlns:tts=\"http://www.w3.org/ns/ttml#styling\" ",
            "xmlns:ttp=\"http://www.w3.org/ns/ttml#parameter\" ttp:timeBase=\"media\" xml:lang=\"{lang}\" tts:extent=\"{width}px {height}px\">\n",
            "  <head>\n    <layout>\n{regions}    </layout>\n  </head>\n",
            "  <body tts:fontFamily=\"{font}\" tts:fontSize=\"{size}px\" tts:fontWeight=\"bold\">\n    <div>\n{body}    </div>\n  </body>\n",
            "</tt>\n",
        ),
        lang = settings.language,
        width = info.width,
        height = info.height,
        regions = regions,
        font = settings.font_family,
        size = metrics.font_px,
        body = body,
    )
}

/// `n/den s` snapped to the frame grid, as FCPXML requires.
fn fcp_time(seconds: f64, info: &VideoInfo) -> String {
    let frames = (seconds.max(0.0) * f64::from(info.fps_num) / f64::from(info.fps_den)).round() as u64;
    format!("{}/{}s", frames * u64::from(info.fps_den), info.fps_num)
}

/// RGB in the 0-1 floats FCPXML text styles use.
fn fcp_color(hex: &str) -> String {
    let channel = |range: std::ops::Range<usize>| {
        hex.get(range).and_then(|digits| u8::from_str_radix(digits, 16).ok()).map_or(0.0, |value| f64::from(value) / 255.0)
    };
    format!("{:.4} {:.4} {:.4} 1", channel(1..3), channel(3..5), channel(5..7))
}

fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let encoded: String = text
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' | b':' => char::from(byte).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect();
    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

fn fcp_title(index: usize, cue: &Cue, info: &VideoInfo, metrics: &Metrics, settings: &Settings) -> String {
    let color = settings.color_for(cue.speaker);
    let x = cue.x - f64::from(info.width) / 2.0;
    let y = f64::from(info.height) / 2.0 - cue.y;
    let text = escape_xml(&cue.lines.join("\n"));
    let label = escape_xml(cue.lines.first().map_or("", String::as_str));
    format!(
        concat!(
            "            <title ref=\"r2\" lane=\"1\" name=\"{label}\" offset=\"{offset}\" start=\"0s\" duration=\"{duration}\">\n",
            "              <param name=\"Position\" key=\"{key}\" value=\"{x:.1} {y:.1}\"/>\n",
            "              <text><text-style ref=\"ts{index}\">{text}</text-style></text>\n",
            "              <text-style-def id=\"ts{index}\"><text-style font=\"{font}\" fontSize=\"{size}\" fontFace=\"Bold\" ",
            "fontColor=\"{fill}\" strokeColor=\"{stroke}\" strokeWidth=\"{width:.1}\" alignment=\"center\"/></text-style-def>\n",
            "            </title>\n",
        ),
        label = label,
        offset = fcp_time(cue.start, info),
        duration = fcp_time((cue.end - cue.start).max(0.0), info),
        key = TITLE_POSITION_KEY,
        x = x,
        y = y,
        index = index,
        text = text,
        font = settings.font_family,
        size = metrics.font_px,
        fill = fcp_color(&color.fill),
        stroke = fcp_color(&color.outline),
        width = metrics.outline_px,
    )
}

/// A timeline with the video and one title per cue, which carries colour, outline and position.
#[must_use]
pub fn render_fcpxml(transcript: &Transcript, settings: &Settings) -> String {
    let info = &transcript.info;
    let name = escape_xml(&transcript.source.file_stem().map_or_else(String::new, |stem| stem.to_string_lossy().into_owned()));
    let duration = fcp_time(info.duration, info);
    let titles: String = transcript
        .cues
        .iter()
        .enumerate()
        .map(|(index, cue)| fcp_title(index, cue, info, &transcript.metrics, settings))
        .collect();
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE fcpxml>\n<fcpxml version=\"{version}\">\n  <resources>\n",
            "    <format id=\"r1\" frameDuration=\"{den}/{num}s\" width=\"{width}\" height=\"{height}\"/>\n",
            "    <effect id=\"r2\" name=\"Basic Title\" uid=\"{uid}\"/>\n",
            "    <asset id=\"r3\" name=\"{name}\" src=\"{src}\" start=\"0s\" duration=\"{duration}\" hasVideo=\"1\" hasAudio=\"1\" format=\"r1\" audioSources=\"1\" audioChannels=\"2\"/>\n",
            "  </resources>\n  <library>\n    <event name=\"{name}\">\n      <project name=\"{name}\">\n",
            "        <sequence format=\"r1\" duration=\"{duration}\" tcStart=\"0s\" tcFormat=\"NDF\" audioLayout=\"stereo\" audioRate=\"48k\">\n          <spine>\n",
            "          <asset-clip ref=\"r3\" name=\"{name}\" offset=\"0s\" start=\"0s\" duration=\"{duration}\" format=\"r1\">\n{titles}          </asset-clip>\n",
            "          </spine>\n        </sequence>\n      </project>\n    </event>\n  </library>\n</fcpxml>\n",
        ),
        version = FCPXML_VERSION,
        den = info.fps_den,
        num = info.fps_num,
        width = info.width,
        height = info.height,
        uid = BASIC_TITLE_UID,
        name = name,
        src = escape_xml(&file_url(&transcript.source)),
        duration = duration,
        titles = titles,
    )
}

fn sibling(base: &Path, extension: &str) -> PathBuf {
    let mut name = base.as_os_str().to_owned();
    name.push(extension);
    PathBuf::from(name)
}

const OUTPUT_EXTENSIONS: [&str; 4] = [".ttml", ".srt", ".fcpxml", ".transcript.json"];

/// A base path beside the video that clobbers none of an earlier run's files.
#[must_use]
pub fn unused_base(video: &Path) -> PathBuf {
    let stem = video.with_extension("");
    let taken = |base: &Path| OUTPUT_EXTENSIONS.iter().any(|extension| sibling(base, extension).exists());
    if !taken(&stem) {
        return stem;
    }
    (2u32..)
        .map(|number| {
            let mut name = stem.as_os_str().to_owned();
            name.push(format!("-{number}"));
            PathBuf::from(name)
        })
        .find(|candidate| !taken(candidate))
        .unwrap_or(stem)
}

/// Writes all formats next to `base`; each file is replaced atomically.
pub fn write_outputs(base: &Path, transcript: &Transcript, settings: &Settings) -> AppResult<Vec<PathBuf>> {
    let json = serde_json::to_vec_pretty(transcript).map_err(|error| AppError::Invalid(error.to_string()))?;
    let outputs = [
        (".ttml", render_ttml(&transcript.cues, &transcript.info, &transcript.metrics, settings).into_bytes()),
        (".srt", render_srt(&transcript.cues, settings).into_bytes()),
        (".fcpxml", render_fcpxml(transcript, settings).into_bytes()),
        (".transcript.json", json),
    ];
    let mut written = Vec::with_capacity(outputs.len());
    for (extension, bytes) in outputs {
        let path = sibling(base, extension);
        write_atomic(&path, &bytes)?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: VideoInfo = VideoInfo { width: 1920, height: 1080, duration: 10.0, fps_num: 30000, fps_den: 1001 };
    const METRICS: Metrics = Metrics { font_px: 52.0, outline_px: 6.0, max_line_px: 1716.0 };

    fn cue(speaker: Option<u32>, text: &str) -> Cue {
        Cue { start: 1.5, end: 3.25, speaker, lines: vec![text.to_owned()], x: 960.0, y: 980.0 }
    }

    #[test]
    fn srt_has_clocks_and_speaker_colour() {
        let out = render_srt(&[cue(Some(2), "A&B")], &Settings::default());
        assert_eq!(out, "1\n00:00:01,500 --> 00:00:03,250\n<font color=\"#7CDC8C\">A&amp;B</font>\n\n");
    }

    #[test]
    fn clocks_handle_hours() {
        assert_eq!(srt_clock(3723.004), "01:02:03,004");
        assert_eq!(ttml_clock(3723.004), "01:02:03.004");
    }

    #[test]
    fn ttml_spans_carry_fill_and_outline() {
        let out = render_ttml(&[cue(Some(1), "はい")], &INFO, &METRICS, &Settings::default());
        assert!(out.contains("<span tts:color=\"#C48CFF\" tts:textOutline=\"#FFFFFF 6.0px 0px\">はい</span>"), "{out}");
        assert!(out.contains("<p region=\"r0\" begin=\"00:00:01.500\" end=\"00:00:03.250\">"), "{out}");
        assert!(out.contains("tts:extent=\"1920px 1080px\""), "{out}");
    }

    #[test]
    fn ttml_region_follows_the_cue_position() {
        let out = render_ttml(&[cue(None, "x")], &INFO, &METRICS, &Settings::default());
        assert!(out.contains("tts:origin=\"5.000% 83.519%\""), "{out}");
    }

    #[test]
    fn unassigned_speech_uses_the_grey_colour() {
        let out = render_ttml(&[cue(None, "x")], &INFO, &METRICS, &Settings::default());
        assert!(out.contains("tts:color=\"#C8C8C8\""), "{out}");
    }

    #[test]
    fn fcp_times_snap_to_frames() {
        assert_eq!(fcp_time(1.0, &INFO), "30030/30000s");
        assert_eq!(fcp_time(0.0, &INFO), "0/30000s");
    }

    #[test]
    fn fcp_colours_are_unit_floats() {
        assert_eq!(fcp_color("#FF0000"), "1.0000 0.0000 0.0000 1");
    }

    #[test]
    fn file_urls_are_percent_encoded() {
        assert_eq!(file_url(Path::new("/a b/動.mp4")), "file:///a%20b/%E5%8B%95.mp4");
        assert_eq!(file_url(Path::new("C:\\v\\a.mp4")), "file:///C:/v/a.mp4");
    }

    #[test]
    fn fcpxml_titles_carry_colour_outline_and_position() {
        let transcript = Transcript::new(PathBuf::from("/v/a.mp4"), INFO, METRICS, Vec::new(), Vec::new(), vec![cue(Some(1), "はい")]);
        let out = render_fcpxml(&transcript, &Settings::default());
        assert!(out.contains("value=\"0.0 -440.0\""), "{out}");
        assert!(out.contains("fontColor=\"0.7686 0.5490 1.0000 1\""), "{out}");
        assert!(out.contains("strokeColor=\"1.0000 1.0000 1.0000 1\""), "{out}");
        assert!(out.contains("src=\"file:///v/a.mp4\""), "{out}");
    }

    #[test]
    fn speaker_count_comes_from_the_words() {
        let words = vec![
            Word { start: 0.0, end: 1.0, text: "a".to_owned(), speaker: Some(3) },
            Word { start: 1.0, end: 2.0, text: "b".to_owned(), speaker: None },
        ];
        let transcript = Transcript::new(PathBuf::from("/v/a.mp4"), INFO, METRICS, words, Vec::new(), Vec::new());
        assert_eq!(transcript.speaker_count, 3);
    }
}
