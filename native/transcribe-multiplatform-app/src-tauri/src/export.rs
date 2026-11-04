//! Writes one transcript as the subtitle formats DaVinci Resolve reads. No Resolve API is involved:
//! the JSON is the source of truth (report §7.1), the others are generated from it.

use crate::captions::{Cue, Metrics, Word};
use crate::engines::VideoInfo;
use crate::error::{AppError, AppResult, UserError};
use crate::face::FaceSample;
use crate::paths::write_atomic;
use crate::settings::{CaptionShadow, Settings, SpeakerColor};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const FORMAT_VERSION: u32 = 1;
const MILLIS_PER_HOUR: u64 = 3_600_000;
const MILLIS_PER_MINUTE: u64 = 60_000;
const MILLIS_PER_SECOND: u64 = 1000;
const BASIC_TITLE_UID: &str =
    ".../Titles.localized/Bumper:Opener.localized/Basic Title.localized/Basic Title.moti";
/// Verified in Resolve 21.1.2: it ignores Basic Title's Position parameter, reads `adjust-transform`
/// in percent of the frame height, and treats a positive `strokeWidth` as outline-only, so a filled
/// outline is negative, in percent of the font size, centred on the glyph edge.
const PERCENT: f64 = 100.0;
const STROKE_SIDES: f64 = 2.0;
const FCPXML_VERSION: &str = "1.10";
/// Style 20's shadow straight down, relative to the font size; the UI preview uses the same values.
const SHADOW_OFFSET_EM: f64 = 16.0 / 88.0;
const SHADOW_BLUR_EM: f64 = 8.0 / 88.0;
/// FCPXML's shadow direction in degrees for straight down.
const SHADOW_ANGLE_DOWN: u32 = 270;
const BLACK: &str = "#000000";
/// `lineSpacing` in FCPXML is added to the title's own line height.
const FCP_NATURAL_LINE_HEIGHT: f64 = 1.2;
const BYTE_MAX: f64 = 255.0;

/// One stacked title of a caption.
struct TitleLayer {
    fill: String,
    /// Colour and width per side in pixels.
    stroke: Option<(String, f64)>,
}

/// What every layer of one caption shares.
struct CueMarkup<'a> {
    label: &'a str,
    offset: &'a str,
    duration: &'a str,
    x: f64,
    y: f64,
    text: &'a str,
    font: &'a str,
    size: f64,
    line_height: f64,
    metrics: &'a Metrics,
}

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
    pub fn new(
        source: PathBuf,
        info: VideoInfo,
        metrics: Metrics,
        words: Vec<Word>,
        faces: Vec<FaceSample>,
        cues: Vec<Cue>,
    ) -> Self {
        let speaker_count = words
            .iter()
            .filter_map(|word| word.speaker)
            .max()
            .unwrap_or(0);
        Self {
            format_version: FORMAT_VERSION,
            source,
            info,
            metrics,
            speaker_count,
            words,
            faces,
            cues,
        }
    }

    pub fn load(path: &Path) -> AppResult<Self> {
        let bytes = std::fs::read(path)
            .map_err(crate::error::io_context(format!("read {}", path.display())))?;
        serde_json::from_slice(&bytes)
            .map_err(|error| AppError::Invalid(format!("transcript file: {error}")))
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
        // SRT is plain text: XML escapes would show up literally in Resolve.
        let text = cue.lines.join("\n");
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

/// TTML2 `tts:textShadow`, or nothing when shadows are off.
fn ttml_shadow(metrics: &Metrics, settings: &Settings) -> String {
    let CaptionShadow { enabled, strength } = settings.shadow;
    if !enabled {
        return String::new();
    }
    let offset = metrics.font_px * SHADOW_OFFSET_EM;
    format!(
        " tts:textShadow=\"0px {offset:.1}px {blur:.1}px rgba(0,0,0,{alpha})\"",
        blur = metrics.font_px * SHADOW_BLUR_EM,
        alpha = (strength * BYTE_MAX).round() as u8,
    )
}

fn ttml_span(line: &str, color: &SpeakerColor, outline_px: f64, shadow: &str) -> String {
    format!(
        "<span tts:color=\"{fill}\" tts:textOutline=\"{outline} {outline_px:.1}px 0px\"{shadow}>{text}</span>",
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
pub fn render_ttml(
    cues: &[Cue],
    info: &VideoInfo,
    metrics: &Metrics,
    settings: &Settings,
) -> String {
    let mut regions = String::new();
    let mut body = String::new();
    let region_height = metrics.font_px * 3.0;
    let shadow = ttml_shadow(metrics, settings);
    for (index, cue) in cues.iter().enumerate() {
        let width = metrics.max_line_px + 2.0 * metrics.outline_px;
        let left = (cue.x - width / 2.0).clamp(0.0, (f64::from(info.width) - width).max(0.0));
        let top = (cue.y - region_height / 2.0)
            .clamp(0.0, (f64::from(info.height) - region_height).max(0.0));
        let _ = writeln!(
            regions,
            "      <region xml:id=\"r{index}\" tts:origin=\"{:.3}% {:.3}%\" tts:extent=\"{:.3}% {:.3}%\" tts:displayAlign=\"center\" tts:textAlign=\"center\"/>",
            percent(left, info.width),
            percent(top, info.height),
            percent(width, info.width),
            percent(region_height, info.height),
        );
        let color = settings.color_for(cue.speaker);
        let spans: Vec<String> = cue
            .lines
            .iter()
            .map(|line| ttml_span(line, color, metrics.outline_px, &shadow))
            .collect();
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
            "  <body tts:fontFamily=\"{font}\" tts:fontSize=\"{size}px\" tts:lineHeight=\"{line_height:.1}px\" tts:fontWeight=\"bold\">\n    <div>\n{body}    </div>\n  </body>\n",
            "</tt>\n",
        ),
        lang = settings.language,
        width = info.width,
        height = info.height,
        regions = regions,
        font = settings.font_family,
        size = metrics.font_px,
        line_height = metrics.font_px * settings.line_height,
        body = body,
    )
}

/// `n/den s` snapped to the frame grid, as FCPXML requires.
fn fcp_time(seconds: f64, info: &VideoInfo) -> String {
    let frames =
        (seconds.max(0.0) * f64::from(info.fps_num) / f64::from(info.fps_den)).round() as u64;
    format!("{}/{}s", frames * u64::from(info.fps_den), info.fps_num)
}

fn frame_at(seconds: f64, info: &VideoInfo) -> u64 {
    (seconds.max(0.0) * f64::from(info.fps_num) / f64::from(info.fps_den)).round() as u64
}

fn fcp_frames(frames: u64, info: &VideoInfo) -> String {
    format!("{}/{}s", frames * u64::from(info.fps_den), info.fps_num)
}

/// Offset and duration on the frame grid: ends are rounded the same way as starts, so a title can
/// never run into the next one, and it always lasts at least one frame.
fn fcp_span(start: f64, end: f64, info: &VideoInfo) -> (String, String) {
    let first = frame_at(start, info);
    let frames = frame_at(end, info).saturating_sub(first).max(1);
    (fcp_frames(first, info), fcp_frames(frames, info))
}

fn fcp_shadow(metrics: &Metrics, settings: &Settings) -> String {
    let CaptionShadow { enabled, strength } = settings.shadow;
    if !enabled {
        return String::new();
    }
    format!(
        " shadowColor=\"0 0 0 {strength:.2}\" shadowOffset=\"{offset:.1} {SHADOW_ANGLE_DOWN}\" shadowBlurRadius=\"{blur:.1}\"",
        offset = metrics.font_px * SHADOW_OFFSET_EM,
        blur = metrics.font_px * SHADOW_BLUR_EM,
    )
}

/// RGB in the 0-1 floats FCPXML text styles use.
fn fcp_color(hex: &str) -> String {
    let channel = |range: std::ops::Range<usize>| {
        hex.get(range)
            .and_then(|digits| u8::from_str_radix(digits, 16).ok())
            .map_or(0.0, |value| f64::from(value) / 255.0)
    };
    format!(
        "{:.4} {:.4} {:.4} 1",
        channel(1..3),
        channel(3..5),
        channel(5..7)
    )
}

fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let encoded: String = text
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' | b':' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect();
    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

/// Bottom to top, as the repository's captions are drawn: black rim, outline band, then the fill with
/// no stroke at all, so one glyph's outline can never paint over its neighbour's fill.
fn title_layers(color: &SpeakerColor, metrics: &Metrics, settings: &Settings) -> Vec<TitleLayer> {
    let band = metrics.outline_px;
    let rim = settings.rim_ratio * metrics.font_px;
    let needs_rim = rim > 0.0 && !color.outline.eq_ignore_ascii_case(BLACK);
    let rim_layer = needs_rim.then(|| TitleLayer {
        fill: BLACK.to_owned(),
        stroke: Some((BLACK.to_owned(), band + rim)),
    });
    rim_layer
        .into_iter()
        .chain([
            TitleLayer {
                fill: color.outline.clone(),
                stroke: Some((color.outline.clone(), band)),
            },
            TitleLayer {
                fill: color.fill.clone(),
                stroke: None,
            },
        ])
        .collect()
}

/// A negative `strokeWidth` is percent of the font size for the stroke centred on the glyph edge.
fn fcp_stroke(layer: &TitleLayer, metrics: &Metrics) -> String {
    layer
        .stroke
        .as_ref()
        .map_or_else(String::new, |(color, per_side)| {
            format!(
                " strokeColor=\"{}\" strokeWidth=\"-{:.1}\"",
                fcp_color(color),
                per_side * STROKE_SIDES / metrics.font_px * PERCENT
            )
        })
}

fn fcp_layer(
    id: &str,
    lane: usize,
    layer: &TitleLayer,
    shadow: &str,
    cue_markup: &CueMarkup<'_>,
) -> String {
    let CueMarkup {
        label,
        offset,
        duration,
        x,
        y,
        text,
        font,
        size,
        line_height,
        metrics,
    } = cue_markup;
    format!(
        concat!(
            "            <title ref=\"r2\" lane=\"{lane}\" name=\"{label}\" offset=\"{offset}\" start=\"0s\" duration=\"{duration}\">\n",
            "              <adjust-transform position=\"{x:.3} {y:.3}\"/>\n",
            "              <text><text-style ref=\"{id}\">{text}</text-style></text>\n",
            "              <text-style-def id=\"{id}\"><text-style font=\"{font}\" fontSize=\"{size}\" fontFace=\"Bold\" lineSpacing=\"{spacing:.1}\" ",
            "fontColor=\"{fill}\"{stroke} alignment=\"center\"{shadow}/></text-style-def>\n",
            "            </title>\n",
        ),
        lane = lane,
        id = id,
        label = label,
        offset = offset,
        duration = duration,
        x = x,
        y = y,
        text = text,
        font = font,
        size = size,
        spacing = *size * (*line_height - FCP_NATURAL_LINE_HEIGHT),
        fill = fcp_color(&layer.fill),
        stroke = fcp_stroke(layer, metrics),
        shadow = shadow,
    )
}

fn fcp_title(
    index: usize,
    cue: &Cue,
    info: &VideoInfo,
    metrics: &Metrics,
    settings: &Settings,
) -> String {
    let frame_height = f64::from(info.height);
    let (offset, duration) = fcp_span(cue.start, cue.end, info);
    let text = escape_xml(&cue.lines.join("\n"));
    let label = escape_xml(cue.lines.first().map_or("", String::as_str));
    let markup = CueMarkup {
        label: &label,
        offset: &offset,
        duration: &duration,
        x: (cue.x - f64::from(info.width) / 2.0) / frame_height * PERCENT,
        y: (frame_height / 2.0 - cue.y) / frame_height * PERCENT,
        text: &text,
        font: &settings.font_family,
        size: metrics.font_px,
        line_height: settings.line_height,
        metrics,
    };
    let shadow = fcp_shadow(metrics, settings);
    title_layers(settings.color_for(cue.speaker), metrics, settings)
        .iter()
        .enumerate()
        .map(|(layer_index, layer)| {
            // Only the bottom layer casts the shadow; a shadow per layer would darken the band.
            let layer_shadow = if layer_index == 0 {
                shadow.as_str()
            } else {
                ""
            };
            fcp_layer(
                &format!("ts{index}-{layer_index}"),
                layer_index + 1,
                layer,
                layer_shadow,
                &markup,
            )
        })
        .collect()
}

/// A timeline with the video and one title per cue, which carries colour, outline and position.
#[must_use]
pub fn render_fcpxml(transcript: &Transcript, settings: &Settings) -> String {
    let info = &transcript.info;
    let name = escape_xml(
        &transcript
            .source
            .file_stem()
            .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned()),
    );
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

const TRANSCRIPT_EXTENSION: &str = ".transcript.json";
const OUTPUT_EXTENSIONS: [&str; 4] = [".ttml", ".srt", ".fcpxml", TRANSCRIPT_EXTENSION];

/// A base path beside the video that clobbers none of an earlier run's files.
#[must_use]
pub fn unused_base(video: &Path) -> PathBuf {
    unused_stem(&video.with_extension(""))
}

/// `stem` itself when free, otherwise `stem-2`, `stem-3` and so on.
#[must_use]
pub fn unused_stem(stem: &Path) -> PathBuf {
    let taken = |base: &Path| {
        OUTPUT_EXTENSIONS
            .iter()
            .any(|extension| sibling(base, extension).exists())
    };
    if !taken(stem) {
        return stem.to_path_buf();
    }
    (2u32..)
        .map(|number| {
            let mut name = stem.as_os_str().to_owned();
            name.push(format!("-{number}"));
            PathBuf::from(name)
        })
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| stem.to_path_buf())
}

/// The path every output of a transcript shares, without the extension.
#[must_use]
pub fn transcript_base(transcript_file: &Path) -> PathBuf {
    let name = transcript_file.to_string_lossy();
    PathBuf::from(name.strip_suffix(TRANSCRIPT_EXTENSION).unwrap_or(&name))
}

/// The same base whichever of a transcript's outputs was dropped.
#[must_use]
pub fn dropped_base(dropped: &Path) -> PathBuf {
    let name = dropped.to_string_lossy();
    if name.ends_with(TRANSCRIPT_EXTENSION) {
        return transcript_base(dropped);
    }
    dropped.with_extension("")
}

/// The transcript a dropped subtitle file was written from; only it holds the timing and speakers.
pub fn transcript_beside(dropped: &Path) -> AppResult<PathBuf> {
    let transcript = sibling(&dropped_base(dropped), TRANSCRIPT_EXTENSION);
    if transcript.is_file() {
        return Ok(transcript);
    }
    Err(UserError::TranscriptMissing {
        file: dropped
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
    }
    .into())
}

/// Writes all formats next to `base`; each file is replaced atomically.
pub fn write_outputs(
    base: &Path,
    transcript: &Transcript,
    settings: &Settings,
) -> AppResult<Vec<PathBuf>> {
    let json = serde_json::to_vec_pretty(transcript)
        .map_err(|error| AppError::Invalid(error.to_string()))?;
    let outputs = [
        (
            ".ttml",
            render_ttml(
                &transcript.cues,
                &transcript.info,
                &transcript.metrics,
                settings,
            )
            .into_bytes(),
        ),
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

    #[test]
    fn a_stem_in_use_moves_to_the_next_free_number() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target-test/export-unused-stem");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(unused_stem(&root.join("clip")), root.join("clip"));
        std::fs::write(root.join("clip.srt"), b"x").unwrap();
        assert_eq!(unused_stem(&root.join("clip")), root.join("clip-2"));
        std::fs::write(root.join("clip-2.transcript.json"), b"x").unwrap();
        assert_eq!(unused_stem(&root.join("clip")), root.join("clip-3"));
    }

    #[test]
    fn every_output_of_a_transcript_shares_one_base() {
        assert_eq!(
            dropped_base(Path::new("/a/clip.v2.transcript.json")),
            Path::new("/a/clip.v2")
        );
        assert_eq!(
            dropped_base(Path::new("/a/clip.v2.ttml")),
            Path::new("/a/clip.v2")
        );
        assert_eq!(dropped_base(Path::new("/a/clip.SRT")), Path::new("/a/clip"));
        assert_eq!(
            transcript_base(Path::new("/a/clip.transcript.json")),
            Path::new("/a/clip")
        );
        assert_eq!(
            transcript_base(Path::new("/a/clip.json")),
            Path::new("/a/clip.json")
        );
    }

    #[test]
    fn a_dropped_subtitle_finds_the_transcript_of_the_same_name() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target-test/export-transcript-beside");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("clip.transcript.json"), b"{}").unwrap();
        assert_eq!(
            transcript_beside(&root.join("clip.fcpxml")).unwrap(),
            root.join("clip.transcript.json")
        );
        assert_eq!(
            transcript_beside(&root.join("clip.transcript.json")).unwrap(),
            root.join("clip.transcript.json")
        );
        assert_eq!(
            serde_json::to_value(transcript_beside(&root.join("other.ttml")).unwrap_err()).unwrap(),
            serde_json::json!({ "code": "transcript_missing", "file": "other.ttml" })
        );
    }

    const INFO: VideoInfo = VideoInfo {
        width: 1920,
        height: 1080,
        duration: 10.0,
        fps_num: 30000,
        fps_den: 1001,
    };
    const METRICS: Metrics = Metrics {
        font_px: 52.0,
        outline_px: 6.0,
        max_line_px: 1716.0,
    };

    fn cue(speaker: Option<u32>, text: &str) -> Cue {
        Cue {
            start: 1.5,
            end: 3.25,
            speaker,
            lines: vec![text.to_owned()],
            x: 960.0,
            y: 980.0,
        }
    }

    #[test]
    fn srt_has_clocks_and_speaker_colour() {
        let out = render_srt(&[cue(Some(2), "A&B")], &Settings::default());
        assert_eq!(
            out,
            "1\n00:00:01,500 --> 00:00:03,250\n<font color=\"#005A9C\">A&B</font>\n\n"
        );
    }

    #[test]
    fn clocks_handle_hours() {
        assert_eq!(srt_clock(3723.004), "01:02:03,004");
        assert_eq!(ttml_clock(3723.004), "01:02:03.004");
    }

    #[test]
    fn ttml_spans_carry_fill_and_outline() {
        let out = render_ttml(
            &[cue(Some(1), "はい")],
            &INFO,
            &METRICS,
            &Settings::default(),
        );
        assert!(
            out.contains(
                "<span tts:color=\"#FFFFFF\" tts:textOutline=\"#000000 6.0px 0px\" tts:textShadow=\"0px 9.5px 4.7px rgba(0,0,0,204)\">はい</span>"
            ),
            "{out}"
        );
        assert!(
            out.contains("<p region=\"r0\" begin=\"00:00:01.500\" end=\"00:00:03.250\">"),
            "{out}"
        );
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
        assert!(out.contains("tts:color=\"#595959\""), "{out}");
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
        assert_eq!(
            file_url(Path::new("/a b/動.mp4")),
            "file:///a%20b/%E5%8B%95.mp4"
        );
        assert_eq!(file_url(Path::new("C:\\v\\a.mp4")), "file:///C:/v/a.mp4");
    }

    #[test]
    fn fcpxml_titles_carry_colour_outline_and_position() {
        let transcript = Transcript::new(
            PathBuf::from("/v/a.mp4"),
            INFO,
            METRICS,
            Vec::new(),
            Vec::new(),
            vec![cue(Some(1), "はい")],
        );
        let out = render_fcpxml(&transcript, &Settings::default());
        assert!(
            out.contains("<adjust-transform position=\"0.000 -40.741\"/>"),
            "{out}"
        );
        assert!(
            out.contains("fontColor=\"1.0000 1.0000 1.0000 1\""),
            "{out}"
        );
        assert!(
            out.contains("strokeColor=\"0.0000 0.0000 0.0000 1\" strokeWidth=\"-23.1\""),
            "{out}"
        );
        assert!(out.contains("src=\"file:///v/a.mp4\""), "{out}");
    }

    #[test]
    fn speaker_count_comes_from_the_words() {
        let words = vec![
            Word {
                start: 0.0,
                end: 1.0,
                text: "a".to_owned(),
                speaker: Some(3),
            },
            Word {
                start: 1.0,
                end: 2.0,
                text: "b".to_owned(),
                speaker: None,
            },
        ];
        let transcript = Transcript::new(
            PathBuf::from("/v/a.mp4"),
            INFO,
            METRICS,
            words,
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(transcript.speaker_count, 3);
    }

    #[test]
    fn every_markup_character_is_escaped() {
        assert_eq!(
            escape_xml("a&b<c>d\"e'f"),
            "a&amp;b&lt;c&gt;d&quot;e&apos;f"
        );
    }

    #[test]
    fn captions_without_a_shadow_have_no_shadow_attributes() {
        let settings = Settings {
            shadow: crate::settings::CaptionShadow {
                enabled: false,
                strength: 0.5,
            },
            ..Settings::default()
        };
        let ttml = render_ttml(&[cue(None, "x")], &INFO, &METRICS, &settings);
        assert!(!ttml.contains("textShadow"), "{ttml}");
        let transcript = Transcript::new(
            PathBuf::from("/v/a.mp4"),
            INFO,
            METRICS,
            Vec::new(),
            Vec::new(),
            vec![cue(None, "x")],
        );
        let fcpxml = render_fcpxml(&transcript, &settings);
        assert!(!fcpxml.contains("shadowColor"), "{fcpxml}");
    }

    #[test]
    fn fcpxml_titles_carry_the_shadow() {
        let transcript = Transcript::new(
            PathBuf::from("/v/a.mp4"),
            INFO,
            METRICS,
            Vec::new(),
            Vec::new(),
            vec![cue(None, "x")],
        );
        let out = render_fcpxml(&transcript, &Settings::default());
        assert!(
            out.contains(
                "shadowColor=\"0 0 0 0.80\" shadowOffset=\"9.5 270\" shadowBlurRadius=\"4.7\""
            ),
            "{out}"
        );
    }

    #[test]
    fn a_title_lasts_at_least_one_frame() {
        assert_eq!(
            fcp_span(1.0, 1.0, &INFO),
            ("30030/30000s".to_owned(), "1001/30000s".to_owned())
        );
        assert_eq!(
            fcp_span(2.0, 1.0, &INFO),
            ("60060/30000s".to_owned(), "1001/30000s".to_owned())
        );
    }

    #[test]
    fn titles_side_by_side_share_a_frame_boundary_without_overlap() {
        assert_eq!(
            fcp_span(1.0, 2.0, &INFO),
            ("30030/30000s".to_owned(), "30030/30000s".to_owned())
        );
        assert_eq!(
            fcp_span(2.0, 3.0, &INFO),
            ("60060/30000s".to_owned(), "30030/30000s".to_owned())
        );
    }

    #[test]
    fn a_white_band_stacks_rim_band_and_fill_titles() {
        let settings = Settings::default();
        let out = fcp_title(0, &cue(Some(2), "A&B"), &INFO, &METRICS, &settings);
        assert_eq!(
            out,
            concat!(
                "            <title ref=\"r2\" lane=\"1\" name=\"A&amp;B\" offset=\"45045/30000s\" start=\"0s\" duration=\"52052/30000s\">\n",
                "              <adjust-transform position=\"0.000 -40.741\"/>\n",
                "              <text><text-style ref=\"ts0-0\">A&amp;B</text-style></text>\n",
                "              <text-style-def id=\"ts0-0\"><text-style font=\"Noto Sans JP\" fontSize=\"52\" fontFace=\"Bold\" lineSpacing=\"12.9\" fontColor=\"0.0000 0.0000 0.0000 1\" strokeColor=\"0.0000 0.0000 0.0000 1\" strokeWidth=\"-29.9\" alignment=\"center\" shadowColor=\"0 0 0 0.80\" shadowOffset=\"9.5 270\" shadowBlurRadius=\"4.7\"/></text-style-def>\n",
                "            </title>\n",
                "            <title ref=\"r2\" lane=\"2\" name=\"A&amp;B\" offset=\"45045/30000s\" start=\"0s\" duration=\"52052/30000s\">\n",
                "              <adjust-transform position=\"0.000 -40.741\"/>\n",
                "              <text><text-style ref=\"ts0-1\">A&amp;B</text-style></text>\n",
                "              <text-style-def id=\"ts0-1\"><text-style font=\"Noto Sans JP\" fontSize=\"52\" fontFace=\"Bold\" lineSpacing=\"12.9\" fontColor=\"1.0000 1.0000 1.0000 1\" strokeColor=\"1.0000 1.0000 1.0000 1\" strokeWidth=\"-23.1\" alignment=\"center\"/></text-style-def>\n",
                "            </title>\n",
                "            <title ref=\"r2\" lane=\"3\" name=\"A&amp;B\" offset=\"45045/30000s\" start=\"0s\" duration=\"52052/30000s\">\n",
                "              <adjust-transform position=\"0.000 -40.741\"/>\n",
                "              <text><text-style ref=\"ts0-2\">A&amp;B</text-style></text>\n",
                "              <text-style-def id=\"ts0-2\"><text-style font=\"Noto Sans JP\" fontSize=\"52\" fontFace=\"Bold\" lineSpacing=\"12.9\" fontColor=\"0.0000 0.3529 0.6118 1\" alignment=\"center\"/></text-style-def>\n",
                "            </title>\n",
            )
        );
    }

    #[test]
    fn a_black_outline_or_no_rim_needs_no_rim_title() {
        let settings = Settings::default();
        assert_eq!(
            title_layers(&settings.palette[0], &METRICS, &settings).len(),
            2
        );
        let no_rim = Settings {
            rim_ratio: 0.0,
            ..Settings::default()
        };
        assert_eq!(title_layers(&no_rim.palette[1], &METRICS, &no_rim).len(), 2);
    }

    #[test]
    fn ttml_sets_a_line_pitch_that_keeps_outlines_apart() {
        let out = render_ttml(&[cue(None, "x")], &INFO, &METRICS, &Settings::default());
        assert!(out.contains("tts:lineHeight=\"75.3px\""), "{out}");
    }

    #[test]
    fn the_line_pitch_setting_reaches_both_formats() {
        let settings = Settings {
            line_height: 2.0,
            ..Settings::default()
        };
        let ttml = render_ttml(&[cue(None, "x")], &INFO, &METRICS, &settings);
        assert!(ttml.contains("tts:lineHeight=\"104.0px\""), "{ttml}");
        let title = fcp_title(0, &cue(None, "x"), &INFO, &METRICS, &settings);
        assert!(title.contains("lineSpacing=\"41.6\""), "{title}");
    }
}
