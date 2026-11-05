//! A DaVinci Resolve timeline holding the video and one Text+ title per caption, the one format where
//! Resolve keeps every colour, thickness and shadow. The templates come from a Resolve export with its
//! machine id, paths and thumbnail removed; `{{...}}` slots take this video's values.

use crate::captions::{Cue, Metrics};
use crate::engines::{AudioInfo, VideoInfo};
use crate::error::{AppError, AppResult};
use crate::export::{escape_xml, frame_span, title_layers, LayerKind, Transcript};
use crate::settings::{Settings, SpeakerColor};
use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::io::{Cursor, Write};
use std::path::Path;
use std::time::UNIX_EPOCH;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

const PROJECT_TEMPLATE: &str = include_str!("drt_template/project.xml");
const FOLDER_TEMPLATE: &str = include_str!("drt_template/folder.xml");
const SEQUENCE_TEMPLATE: &str = include_str!("drt_template/sequence.xml");
const TITLE_TEMPLATE: &str = include_str!("drt_template/title.xml");
/// The template's id slot of the sequence container, which also names its file.
const SEQUENCE_SLOT: &str = "13";
/// Measured in Resolve 21.1.2: a Text+ em is `Size` x this x the frame width.
const EM_PER_SIZE: f64 = 0.5387;
/// Measured the same way: one side of an outline is `Thickness` x this x the em.
const STROKE_PER_THICKNESS: f64 = 1.1;
/// FCPXML draws a stroke of ratio `r` at `r` x this in frame pixels, whatever the font size; a timeline
/// matches that measured width.
const STROKE_PX_PER_RATIO: f64 = 100.0;
/// Text+ `LineSpacing` 1 keeps the font's own pitch, about this many ems for CJK fonts.
const NATURAL_LINE_HEIGHT: f64 = 1.2;
const CENTERED: u32 = 3;
const FONT_STYLE: &str = "Bold";
const RESOLVE_VERSION: &str = "DaVinci Resolve 21.1.2.0010";
/// The rest of `FrameSize` beyond one 4:2:0 frame, as Resolve wrote it.
const FRAME_SIZE_PADDING: u64 = 16384;
const VIDEO_CODEC: &str = "avc1";
const CLIP_TAIL_FIELDS: [(u32, u64); 3] = [(15, 2), (16, 100), (18, 16384)];
const VIDEO_CLIP_FIELD_14: u64 = 228;
const FIELDS_TRAILER: &str = "12100000000000000006ffffffffffffffff";
const IDENTITY_MATRIX: &str =
    "0000803f0000000000000000000000000000803f0000000000000000000000000000803f";
const VERSION_STAMP: &str = "e0d8b1bb0d";
const RAW_BLOB: u8 = 0x80;
const SECONDS_PER_DAY: u64 = 86_400;
const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// One Text+ input.
enum LuaValue {
    Number(f64),
    Text(String),
    Point(f64, f64),
}

/// Ids stay the same within one export and differ between exports.
struct Ids<'a> {
    seed: &'a str,
}

impl Ids<'_> {
    fn uuid(&self, label: &str) -> String {
        let hash = Sha256::digest(format!("{}/{label}", self.seed));
        let hex = hex(&hash[..16]);
        format!(
            "{}-{}-4{}-a{}-{}",
            &hex[..8],
            &hex[8..12],
            &hex[13..16],
            &hex[17..20],
            &hex[20..32]
        )
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_be_bytes).collect()
}

fn varint(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}

fn proto_bytes(field: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = varint(u64::from(field << 3 | 2));
    out.extend(varint(payload.len() as u64));
    out.extend_from_slice(payload);
    out
}

fn proto_number(field: u32, value: u64) -> Vec<u8> {
    let mut out = varint(u64::from(field << 3));
    out.extend(varint(value));
    out
}

/// Resolve's blob wrapper: version 2, the length, then a raw protobuf after its marker byte.
fn raw_blob(proto: &[u8]) -> String {
    let mut out = 2u32.to_be_bytes().to_vec();
    out.extend_from_slice(&(proto.len() as u32 + 1).to_be_bytes());
    out.push(RAW_BLOB);
    out.extend_from_slice(proto);
    hex(&out)
}

fn zlib(bytes: &[u8]) -> AppResult<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder
        .write_all(bytes)
        .and_then(|()| encoder.finish())
        .map_err(|error| AppError::Invalid(format!("compress timeline: {error}")))
}

fn lua_text(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

fn channel(hex_color: &str, start: usize) -> f64 {
    hex_color
        .get(start..start + 2)
        .and_then(|digits| u8::from_str_radix(digits, 16).ok())
        .map_or(0.0, |value| f64::from(value) / 255.0)
}

fn color_inputs(element: u32, hex_color: &str) -> Vec<(String, LuaValue)> {
    vec![
        (
            format!("Red{element}"),
            LuaValue::Number(channel(hex_color, 1)),
        ),
        (
            format!("Green{element}"),
            LuaValue::Number(channel(hex_color, 3)),
        ),
        (
            format!("Blue{element}"),
            LuaValue::Number(channel(hex_color, 5)),
        ),
    ]
}

/// One outlined element of a Text+: its colour and its reach past the glyph.
fn stroke_inputs(
    element: u32,
    shaped: bool,
    hex_color: &str,
    thickness: f64,
) -> Vec<(String, LuaValue)> {
    let mut inputs = vec![
        (format!("Enabled{element}"), LuaValue::Number(1.0)),
        (format!("Softness{element}"), LuaValue::Number(1.0)),
    ];
    if shaped {
        inputs.push((format!("ElementShape{element}"), LuaValue::Number(1.0)));
    }
    inputs.extend(color_inputs(element, hex_color));
    inputs.push((format!("Thickness{element}"), LuaValue::Number(thickness)));
    inputs
}

/// The speaker's values as Text+ elements: 1 fill, 2 outline, 3 shadow, 4 rim, 5 thickened fill.
fn look_inputs(color: &SpeakerColor, metrics: &Metrics) -> Vec<(String, LuaValue)> {
    let font_px = metrics.font_px;
    let em = font_px * metrics.rendered_scale;
    let mut inputs = color_inputs(1, &color.fill);
    let mut outermost = 0.0_f64;
    for layer in title_layers(color, font_px) {
        let Some((stroke_color, per_side)) = &layer.stroke else {
            continue;
        };
        // `title_layers` gives ratio x font size per side.
        let drawn_px = per_side / font_px * STROKE_PX_PER_RATIO;
        let thickness = drawn_px / em / STROKE_PER_THICKNESS;
        outermost = outermost.max(thickness);
        inputs.extend(match layer.kind {
            LayerKind::Rim => stroke_inputs(4, true, stroke_color, thickness),
            LayerKind::Outline => stroke_inputs(2, false, stroke_color, thickness),
            LayerKind::Fill => stroke_inputs(5, true, stroke_color, thickness),
        });
    }
    if let Some(shadow) = &color.shadow {
        // A glyph-shaped shadow would hide under the edges, which reach further than its offset.
        if outermost > 0.0 {
            inputs.extend(stroke_inputs(3, true, shadow, outermost));
        } else {
            inputs.push(("Enabled3".to_owned(), LuaValue::Number(1.0)));
            inputs.extend(color_inputs(3, shadow));
        }
        inputs.push((
            "Offset3".to_owned(),
            LuaValue::Point(color.shadow_x, color.shadow_y),
        ));
        inputs.push(("SoftnessX3".to_owned(), LuaValue::Number(color.shadow_blur)));
        inputs.push(("SoftnessY3".to_owned(), LuaValue::Number(color.shadow_blur)));
    }
    inputs
}

fn cue_inputs(
    cue: &Cue,
    transcript: &Transcript,
    settings: &Settings,
    frames: u64,
) -> Vec<(String, LuaValue)> {
    let info = &transcript.info;
    let metrics = &transcript.metrics;
    let (width, height) = (f64::from(info.width), f64::from(info.height));
    let mut inputs = vec![
        (
            "GlobalOut".to_owned(),
            LuaValue::Number(frames.saturating_sub(1) as f64),
        ),
        ("Width".to_owned(), LuaValue::Number(width)),
        ("Height".to_owned(), LuaValue::Number(height)),
        ("UseFrameFormatSettings".to_owned(), LuaValue::Number(1.0)),
        (
            "StyledText".to_owned(),
            LuaValue::Text(cue.lines.join("\n")),
        ),
        (
            "Font".to_owned(),
            LuaValue::Text(settings.font_family.clone()),
        ),
        ("Style".to_owned(), LuaValue::Text(FONT_STYLE.to_owned())),
        (
            "Size".to_owned(),
            LuaValue::Number(metrics.font_px * metrics.rendered_scale / (EM_PER_SIZE * width)),
        ),
        (
            "LineSpacing".to_owned(),
            LuaValue::Number(settings.line_height / NATURAL_LINE_HEIGHT),
        ),
        (
            "VerticalJustificationNew".to_owned(),
            LuaValue::Number(f64::from(CENTERED)),
        ),
        (
            "HorizontalJustificationNew".to_owned(),
            LuaValue::Number(f64::from(CENTERED)),
        ),
        // Text+ measures y from the bottom of the frame.
        (
            "Center".to_owned(),
            LuaValue::Point(cue.x / width, 1.0 - cue.y / height),
        ),
    ];
    inputs.extend(look_inputs(settings.color_for(cue.speaker), metrics));
    inputs
}

fn lua_inputs(inputs: &[(String, LuaValue)]) -> String {
    inputs
        .iter()
        .map(|(name, value)| {
            let value = match value {
                LuaValue::Number(number) => number.to_string(),
                LuaValue::Text(text) => lua_text(text),
                LuaValue::Point(x, y) => format!("{{ {x}, {y} }}"),
            };
            format!("{name} = Input {{ Value = {value}, }}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn composition_key(name: &str) -> Vec<u8> {
    let key = utf16(name);
    let mut out = (key.len() as u32).to_be_bytes().to_vec();
    out.extend(key);
    out
}

fn composition_value(kind: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = kind.to_be_bytes().to_vec();
    out.push(0);
    out.extend_from_slice(payload);
    out
}

/// A one-tool Fusion composition: a Text+ with `inputs` feeding the title's output.
fn composition(inputs: &[(String, LuaValue)], frames: u64, info: &VideoInfo) -> AppResult<String> {
    let last = frames.saturating_sub(1);
    let tools = format!(
        "{{ Template = TextPlus {{ CtrlWZoom = false, Inputs = {{ {} }}, ViewInfo = OperatorInfo {{ Pos = {{ 220, 49.5 }} }}, Version = 1 }}, MediaOut1 = MediaOut {{ Inputs = {{ Index = Input {{ Value = \"0\", }}, Input = Input {{ SourceOp = \"Template\", Source = \"Output\", }} }}, ViewInfo = OperatorInfo {{ Pos = {{ 440, 49.5 }} }}, }} }} ",
        lua_inputs(inputs)
    );
    let head = format!(
        "Composition {{ CurrentTime = 0, RenderRange = {{ 0, {last} }}, GlobalRange = {{ 0, {last} }}, CurrentID = 3, HiQ = true, PlaybackUpdateMode = 0, StereoMode = false, Version = \"{RESOLVE_VERSION}\", SavedOutputs = 0, HeldTools = 0, DisabledTools = 0, LockedTools = 0, AudioOffset = 0, Resumable = true, OutputClips = {{ }}, CustomData = {{ TEMPLATE_ID = \"Text+\" }}, Frames = {{ }}, Prefs = {{ Comp = {{ FrameFormat = {{ Width = {width}, Height = {height}, DepthFull = 3, DepthPreview = 3, DepthInteractive = 3, }}, Unsorted = {{ GlobalEnd = {last} }}, }} }}, Compressed = true, }}",
        width = info.width,
        height = info.height,
    );
    let mut data = head.into_bytes();
    data.push(0);
    data.extend_from_slice(&(tools.len() as u32).to_le_bytes());
    data.extend(zlib(tools.as_bytes())?);

    let name = utf16("Composition 1");
    let mut name_value = (name.len() as u32).to_be_bytes().to_vec();
    name_value.extend(name);
    let mut data_value = (data.len() as u32).to_be_bytes().to_vec();
    data_value.extend(data);
    let mut table = [1u32.to_be_bytes(), 4u32.to_be_bytes()].concat();
    let entries: [(&str, Vec<u8>); 4] = [
        ("count", composition_value(2, &1u32.to_be_bytes())),
        ("0_name", composition_value(10, &name_value)),
        ("0_mediaIdList", composition_value(11, &0u32.to_be_bytes())),
        ("0_data", composition_value(12, &data_value)),
    ];
    entries.iter().for_each(|(key, value)| {
        table.extend(composition_key(key));
        table.extend_from_slice(value);
    });
    let mut out = (table.len() as u32).to_be_bytes().to_vec();
    out.extend(zlib(&table)?);
    Ok(hex(&out))
}

fn title_fields(start: u64) -> String {
    let mut clip = proto_bytes(4, b"Text+");
    clip.extend(proto_bytes(9, b"Text+"));
    clip.extend(proto_number(21, start));
    let mut item = proto_bytes(1, &clip);
    item.extend(proto_number(4, 1));
    let mut proto = proto_bytes(1, &item);
    proto.extend(decode_hex(FIELDS_TRAILER));
    raw_blob(&proto)
}

/// The timeline item's fields when it has no audio clip to link to.
fn unlinked_fields() -> String {
    let mut item = proto_bytes(1, &proto_number(21, 0));
    item.extend(proto_number(4, 1));
    let mut proto = proto_bytes(1, &item);
    proto.extend(decode_hex(FIELDS_TRAILER));
    raw_blob(&proto)
}

fn decode_hex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .filter_map(|index| u8::from_str_radix(text.get(index * 2..index * 2 + 2)?, 16).ok())
        .collect()
}

/// `C:\videos\a.mp4` keeps its backslashes wherever the app runs.
fn split_path(path: &str) -> (&str, &str) {
    path.rfind(['/', '\\'])
        .map_or(("", path), |at| (&path[..at], &path[at + 1..]))
}

fn days_to_civil(days: u64) -> (u64, usize, u64) {
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month as usize - 1, day)
}

/// C's `ctime` layout, which Resolve stores beside the same moment in microseconds.
fn ctime(seconds: u64) -> String {
    let days = seconds / SECONDS_PER_DAY;
    let (year, month, day) = days_to_civil(days);
    let rest = seconds % SECONDS_PER_DAY;
    format!(
        "{} {} {day:2} {:02}:{:02}:{:02} {year}",
        WEEKDAYS[(days % 7) as usize],
        MONTHS[month],
        rest / 3600,
        rest / 60 % 60,
        rest % 60,
    )
}

fn modified_micros(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_micros() as u64)
}

fn clip_blob(source: &str, micros: u64, video_id: Option<&str>) -> String {
    let (folder, file) = split_path(source);
    let mut proto = proto_bytes(1, folder.as_bytes());
    proto.extend(proto_bytes(2, file.as_bytes()));
    proto.extend(proto_bytes(3, ctime(micros / 1_000_000).as_bytes()));
    match video_id {
        Some(id) => {
            proto.extend(proto_bytes(5, VIDEO_CODEC.as_bytes()));
            proto.extend(proto_bytes(6, file.as_bytes()));
            proto.extend(proto_bytes(7, id.as_bytes()));
        }
        None => proto.extend(proto_bytes(5, b"AAC")),
    }
    proto.extend(proto_number(13, micros));
    if video_id.is_some() {
        proto.extend(proto_number(14, VIDEO_CLIP_FIELD_14));
    }
    CLIP_TAIL_FIELDS
        .iter()
        .for_each(|(field, value)| proto.extend(proto_number(*field, *value)));
    raw_blob(&proto)
}

/// The timeline's picture settings: frame size repeated for input, output and the working area.
fn version_body(info: &VideoInfo) -> String {
    let (width, height) = (u64::from(info.width), u64::from(info.height));
    let one = 1.0f32.to_le_bytes();
    let mut frame = proto_number(1, width);
    frame.extend(proto_number(2, height));
    frame.push(0x1d);
    frame.extend_from_slice(&one);
    frame.extend(proto_number(4, width));
    frame.extend(proto_number(5, height));
    frame.push(0x35);
    frame.extend_from_slice(&one);
    frame.extend(proto_number(7, width));
    frame.extend(proto_number(8, height));
    frame.extend(proto_number(9, u64::from(u32::MAX)));
    frame.extend(proto_bytes(15, &decode_hex(IDENTITY_MATRIX)));
    let mut body = proto_number(2, 1);
    body.extend(proto_bytes(3, &frame));
    body.extend(decode_hex("60"));
    body.extend(decode_hex(VERSION_STAMP));
    let mut proto = vec![RAW_BLOB];
    proto.extend(proto_bytes(1, &body));
    proto.extend(proto_number(2, 1));
    proto.extend(decode_hex("20"));
    proto.extend(decode_hex(VERSION_STAMP));
    hex(&proto)
}

fn fps(info: &VideoInfo) -> f64 {
    f64::from(info.fps_num) / f64::from(info.fps_den)
}

/// From the duration, as Resolve counts: a stream's own `nb_frames` can be far off (1074 for 901 frames).
fn frame_total(info: &VideoInfo) -> u64 {
    (info.duration * fps(info)).round() as u64
}

fn frame_rate_hex(info: &VideoInfo) -> String {
    format!("{}{}", hex(&fps(info).to_le_bytes()), "0".repeat(16))
}

fn extents_hex(seconds: f64) -> String {
    format!("{}{}", "0".repeat(16), hex(&seconds.to_le_bytes()))
}

fn timemap_hex(rate: f64) -> String {
    format!("02{}", hex(&rate.to_be_bytes()))
}

/// Keeps or drops every `{{#name}}...{{/name}}` part.
fn section(template: &str, name: &str, keep: bool) -> String {
    let (open, close) = (format!("{{{{#{name}}}}}"), format!("{{{{/{name}}}}}"));
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(at) = rest.find(&open) {
        out.push_str(&rest[..at]);
        let inner = &rest[at + open.len()..];
        let end = inner.find(&close).unwrap_or(inner.len());
        if keep {
            out.push_str(&inner[..end]);
        }
        rest = inner.get(end + close.len()..).unwrap_or("");
    }
    out.push_str(rest);
    out
}

/// Replaces every `{{name}}`; `{{id:N}}` and `{{idw:N}}` take the export's id, plain and as UTF-16 hex.
fn fill(template: &str, ids: &Ids<'_>, values: &[(&str, String)]) -> AppResult<String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(at) = rest.find("{{") {
        out.push_str(&rest[..at]);
        let inner = &rest[at + 2..];
        let end = inner
            .find("}}")
            .ok_or_else(|| AppError::Invalid("broken timeline template".into()))?;
        let name = &inner[..end];
        if let Some(slot) = name.strip_prefix("id:") {
            out.push_str(&ids.uuid(slot));
        } else if let Some(slot) = name.strip_prefix("idw:") {
            out.push_str(&hex(&utf16(&ids.uuid(slot))));
        } else {
            let value = values
                .iter()
                .find(|(key, _)| *key == name)
                .ok_or_else(|| AppError::Invalid(format!("timeline template slot {name}")))?;
            out.push_str(&value.1);
        }
        rest = &inner[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

fn title_xml(
    index: usize,
    cue: &Cue,
    transcript: &Transcript,
    settings: &Settings,
    ids: &Ids<'_>,
) -> AppResult<String> {
    let info = &transcript.info;
    let (start, frames) = frame_span(cue.start, cue.end, info);
    let inputs = cue_inputs(cue, transcript, settings, frames);
    let values = [
        ("title_id", ids.uuid(&format!("title-{index}"))),
        ("comp_id", ids.uuid(&format!("comp-{index}"))),
        ("thumb_id", ids.uuid(&format!("thumb-{index}"))),
        ("title_fields", title_fields(start)),
        ("start", start.to_string()),
        ("duration", frames.to_string()),
        ("composition", composition(&inputs, frames, info)?),
        (
            "title_timemap",
            timemap_hex(frames.saturating_sub(1) as f64 / fps(info)),
        ),
        ("frame_rate", frame_rate_hex(info)),
        ("zero4", "0".repeat(8)),
        ("zero8", "0".repeat(16)),
    ];
    fill(TITLE_TEMPLATE, ids, &values)
}

fn audio_values(
    audio: Option<AudioInfo>,
    source: &str,
    micros: u64,
) -> Vec<(&'static str, String)> {
    audio.map_or_else(Vec::new, |audio| {
        vec![
            ("audio_clip", clip_blob(source, micros, None)),
            ("sample_rate", hex(&audio.sample_rate.to_be_bytes())),
            ("channels", hex(&audio.channels.to_be_bytes())),
            (
                "audio_samples",
                hex(
                    &((audio.duration * f64::from(audio.sample_rate)).round() as u64).to_be_bytes(),
                ),
            ),
            ("audio_extents", extents_hex(audio.duration)),
        ]
    })
}

/// The `.drt`: a ZIP of the project, the media pool with the video, and the timeline. `seed` makes its ids.
pub fn render(transcript: &Transcript, settings: &Settings, seed: &str) -> AppResult<Vec<u8>> {
    let info = &transcript.info;
    let ids = Ids { seed };
    let source = transcript.source.to_string_lossy();
    let micros = modified_micros(&transcript.source);
    let (_, file_name) = split_path(&source);
    let stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |parts| parts.0);
    let frames = frame_total(info);
    let titles = transcript
        .cues
        .iter()
        .enumerate()
        .map(|(index, cue)| title_xml(index, cue, transcript, settings, &ids))
        .collect::<AppResult<String>>()?;
    let mut values = vec![
        ("name", escape_xml(stem)),
        ("file_name", escape_xml(file_name)),
        ("file_path", escape_xml(&source)),
        (
            "video_clip",
            clip_blob(&source, micros, Some(&ids.uuid("video-file"))),
        ),
        ("frame_rate", frame_rate_hex(info)),
        ("media_timemap", timemap_hex(fps(info))),
        (
            "resolution",
            format!(
                "{}{}",
                hex(&u64::from(info.width).to_be_bytes()),
                hex(&u64::from(info.height).to_be_bytes())
            ),
        ),
        ("fps_int", hex(&(fps(info).round() as u64).to_be_bytes())),
        ("extents", extents_hex(frames as f64 / fps(info))),
        ("num_frames", hex(&(frames as u32).to_be_bytes())),
        ("num_frames_dec", frames.to_string()),
        (
            "frame_size",
            hex(
                &((u64::from(info.width) * u64::from(info.height) * 3 / 2 + FRAME_SIZE_PADDING)
                    as u32)
                    .to_be_bytes(),
            ),
        ),
        ("version_body", version_body(info)),
        ("unlinked_fields", unlinked_fields()),
        ("titles", titles),
    ];
    values.extend(audio_values(info.audio, &source, micros));
    let has_audio = info.audio.is_some();
    let build = |template: &str| -> AppResult<String> {
        let kept = section(
            &section(template, "audio", has_audio),
            "no_audio",
            !has_audio,
        );
        fill(&kept, &ids, &values)
    };
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let files = [
        ("project.xml".to_owned(), build(PROJECT_TEMPLATE)?),
        (
            "MediaPool/Master/MpFolder.xml".to_owned(),
            build(FOLDER_TEMPLATE)?,
        ),
        (
            format!("SeqContainer/{}.xml", ids.uuid(SEQUENCE_SLOT)),
            build(SEQUENCE_TEMPLATE)?,
        ),
    ];
    for (name, xml) in files {
        zip.start_file(name, SimpleFileOptions::default())
            .and_then(|()| zip.write_all(xml.as_bytes()).map_err(Into::into))
            .map_err(|error| AppError::Invalid(format!("write timeline: {error}")))?;
    }
    zip.finish()
        .map(Cursor::into_inner)
        .map_err(|error| AppError::Invalid(format!("write timeline: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::ZlibDecoder;
    use std::io::Read;
    use std::path::PathBuf;
    use zip::ZipArchive;

    const VERTICAL: VideoInfo = VideoInfo {
        width: 720,
        height: 1280,
        duration: 30.033333,
        fps_num: 30,
        fps_den: 1,
        audio: Some(AudioInfo {
            sample_rate: 44_100,
            channels: 2,
            duration: 30.00381,
        }),
    };
    const NTSC: VideoInfo = VideoInfo {
        width: 1920,
        height: 1080,
        duration: 10.0,
        fps_num: 30_000,
        fps_den: 1001,
        audio: None,
    };
    const METRICS: Metrics = Metrics {
        font_px: 50.0,
        outline_px: 6.0,
        max_line_px: 600.0,
        glyphs: None,
        rendered_scale: 2.0,
    };

    fn cue(start: f64, end: f64, speaker: Option<u32>, lines: &[&str]) -> Cue {
        Cue {
            start,
            end,
            speaker,
            lines: lines.iter().map(|line| (*line).to_owned()).collect(),
            x: 360.0,
            y: 960.0,
        }
    }

    fn transcript(info: VideoInfo, cues: Vec<Cue>) -> Transcript {
        Transcript::new(
            PathBuf::from("/videos/a b&c.mp4"),
            info,
            METRICS,
            Vec::new(),
            Vec::new(),
            cues,
        )
    }

    fn opened(bytes: &[u8]) -> (String, String, String, String) {
        let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut read = |name: String| {
            let mut text = String::new();
            zip.by_name(&name)
                .unwrap()
                .read_to_string(&mut text)
                .unwrap();
            text
        };
        let project = read("project.xml".to_owned());
        let folder = read("MediaPool/Master/MpFolder.xml".to_owned());
        let names: Vec<String> = ZipArchive::new(Cursor::new(bytes))
            .unwrap()
            .file_names()
            .map(str::to_owned)
            .collect();
        let sequence_name = names
            .iter()
            .find(|name| name.starts_with("SeqContainer/"))
            .unwrap()
            .clone();
        let sequence = read(sequence_name.clone());
        (project, folder, sequence, sequence_name)
    }

    /// The Lua text of the title at `index`, through both of its compression layers.
    fn title_lua(sequence: &str, index: usize) -> String {
        let blob = sequence
            .split("<CompositionBA>")
            .nth(index + 1)
            .unwrap()
            .split("</CompositionBA>")
            .next()
            .unwrap();
        let bytes = decode_hex(blob);
        let mut table = Vec::new();
        ZlibDecoder::new(&bytes[4..])
            .read_to_end(&mut table)
            .unwrap();
        let at = table
            .windows(9)
            .position(|window| window == b"Composit\x69")
            .unwrap();
        let nul = at + table[at..].iter().position(|byte| *byte == 0).unwrap();
        let length = u32::from_le_bytes(table[nul + 1..nul + 5].try_into().unwrap()) as usize;
        let mut lua = String::new();
        ZlibDecoder::new(&table[nul + 5..])
            .read_to_string(&mut lua)
            .unwrap();
        assert_eq!(lua.len(), length);
        lua
    }

    fn inner(text: &str, tag: &str) -> Vec<String> {
        text.split(&format!("<{tag}>"))
            .skip(1)
            .map(|part| part.split(&format!("</{tag}>")).next().unwrap().to_owned())
            .collect()
    }

    fn default_render(info: VideoInfo, cues: Vec<Cue>) -> Vec<u8> {
        render(&transcript(info, cues), &Settings::default(), "seed").unwrap()
    }

    #[test]
    fn every_slot_is_filled_and_no_user_data_comes_along() {
        let bytes = default_render(VERTICAL, vec![cue(1.0, 2.0, Some(1), &["はい"])]);
        let (project, folder, sequence, _) = opened(&bytes);
        let all = format!("{project}{folder}{sequence}");
        assert!(!all.contains("{{"));
        assert!(!all.contains("kkk4oru"));
        assert!(!all.contains("single-test"));
        assert!(!all.contains("shisyeru"));
        // A JPEG thumbnail starts with this, in hex.
        assert!(!all.contains("ffd8ffe0"));
    }

    #[test]
    fn the_shipped_templates_hold_no_user_data() {
        let all = format!("{PROJECT_TEMPLATE}{FOLDER_TEMPLATE}{SEQUENCE_TEMPLATE}{TITLE_TEMPLATE}");
        assert!(!all.contains("kkk4oru"));
        assert!(!all.contains("/Users/"));
        assert!(!all.contains("vertical-clip"));
        assert!(!all.contains(&hex(&utf16("FCB214AA7092"))));
        assert!(!all.contains("FCB214AA7092"));
        assert!(!all.contains("ffd8ffe0"));
        assert!(!all.contains(&hex(b"/Users/")));
    }

    #[test]
    fn the_video_fills_the_whole_timeline() {
        let bytes = default_render(VERTICAL, vec![cue(1.0, 2.0, Some(1), &["はい"])]);
        let (_, folder, sequence, _) = opened(&bytes);
        // Video, then the title, then audio: the media clips last the whole 901 frames.
        assert_eq!(inner(&sequence, "Duration"), vec!["901", "30", "901"]);
        assert_eq!(inner(&sequence, "Start"), vec!["0", "30", "0"]);
        // 901 frames at 30 fps, as a little-endian double.
        assert_eq!(
            inner(&folder, "MediaExtents"),
            vec!["0000000000000000".to_owned() + &hex(&(901.0f64 / 30.0).to_le_bytes())]
        );
        assert_eq!(
            inner(&folder, "Resolution"),
            vec!["00000000000002d00000000000000500"]
        );
        assert_eq!(
            inner(&folder, "FrameRate"),
            vec!["0000000000003e400000000000000000"]
        );
    }

    #[test]
    fn the_title_track_holds_one_text_plus_per_cue_in_frames() {
        let bytes = default_render(
            VERTICAL,
            vec![
                cue(0.0, 1.0, Some(1), &["はい"]),
                cue(1.0, 2.5, Some(2), &["どうも"]),
            ],
        );
        let (_, _, sequence, _) = opened(&bytes);
        assert_eq!(inner(&sequence, "Start"), vec!["0", "0", "30", "0"]);
        assert_eq!(inner(&sequence, "Duration"), vec!["901", "30", "45", "901"]);
        assert_eq!(
            sequence
                .matches("<PrettyType>Fusion Title</PrettyType>")
                .count(),
            2
        );
        assert!(title_lua(&sequence, 0).contains("StyledText = Input { Value = \"はい\", }"));
        assert!(title_lua(&sequence, 1).contains("StyledText = Input { Value = \"どうも\", }"));
    }

    #[test]
    fn a_title_field_carries_its_start_and_the_text_plus_name() {
        // 367, the sample Resolve wrote for a title at frame 367.
        assert_eq!(
            title_fields(367),
            "000000020000002b800a160a122205546578742b4a05546578742ba801ef02200112100000000000000006ffffffffffffffff"
        );
    }

    #[test]
    fn the_picture_settings_match_what_resolve_wrote_for_this_size() {
        assert_eq!(
            version_body(&VERTICAL),
            "800a5210011a4808d00510800a1d0000803f20d00528800a350000803f38d00540800a48ffffffff0f7a240000803f0000000000000000000000000000803f0000000000000000000000000000803f60e0d8b1bb0d100120e0d8b1bb0d"
        );
    }

    #[test]
    fn the_first_speaker_is_white_on_a_black_outline_with_no_rim() {
        let bytes = default_render(VERTICAL, vec![cue(1.0, 2.0, Some(1), &["はい"])]);
        let (_, _, sequence, _) = opened(&bytes);
        let lua = title_lua(&sequence, 0);
        assert!(lua.contains("Red1 = Input { Value = 1, }"), "{lua}");
        assert!(lua.contains("Enabled2 = Input { Value = 1, }"), "{lua}");
        assert!(lua.contains("Softness2 = Input { Value = 1, }"), "{lua}");
        assert!(lua.contains("Red2 = Input { Value = 0, }"), "{lua}");
        // 0.115 is 11.5 frame pixels, on an em of 100 px, over the measured stroke scale.
        let thickness: f64 = lua
            .split("Thickness2 = Input { Value = ")
            .nth(1)
            .unwrap()
            .split(',')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!((thickness * 10_000.0).round(), 1045.0);
        assert!(!lua.contains("Enabled4"), "{lua}");
        assert!(!lua.contains("Enabled5"), "{lua}");
        assert!(!lua.contains("Enabled3"), "{lua}");
    }

    #[test]
    fn three_colours_a_thick_fill_and_a_shadow_become_their_elements() {
        let mut settings = Settings::default();
        settings.palette[0] = SpeakerColor {
            fill: "#FF0000".to_owned(),
            fill_width: 0.011,
            outline: "#00FF00".to_owned(),
            outline_width: 0.11,
            rim: Some("#0000FF".to_owned()),
            rim_width: 0.11,
            shadow: Some("#FFFFFF".to_owned()),
            shadow_x: 0.02,
            shadow_y: -0.03,
            shadow_blur: 5.0,
        };
        let bytes = render(
            &transcript(VERTICAL, vec![cue(1.0, 2.0, Some(1), &["はい"])]),
            &settings,
            "seed",
        )
        .unwrap();
        let (_, _, sequence, _) = opened(&bytes);
        let lua = title_lua(&sequence, 0);
        assert!(lua.contains("Red1 = Input { Value = 1, }"), "{lua}");
        assert!(lua.contains("Green2 = Input { Value = 1, }"), "{lua}");
        assert!(lua.contains("Blue4 = Input { Value = 1, }"), "{lua}");
        assert!(
            lua.contains("ElementShape4 = Input { Value = 1, }"),
            "{lua}"
        );
        assert!(
            lua.contains("ElementShape5 = Input { Value = 1, }"),
            "{lua}"
        );
        assert!(lua.contains("Red5 = Input { Value = 1, }"), "{lua}");
        assert!(lua.contains("Enabled3 = Input { Value = 1, }"), "{lua}");
        assert!(
            lua.contains("Offset3 = Input { Value = { 0.02, -0.03 }, }"),
            "{lua}"
        );
        assert!(lua.contains("SoftnessX3 = Input { Value = 5, }"), "{lua}");
        assert!(lua.contains("SoftnessY3 = Input { Value = 5, }"), "{lua}");
        // Each element reaches from the glyph, so each includes the ones inside it.
        let thickness = |name: &str| -> f64 {
            lua.split(&format!("{name} = Input {{ Value = "))
                .nth(1)
                .unwrap()
                .split(',')
                .next()
                .unwrap()
                .parse()
                .unwrap()
        };
        assert_eq!((thickness("Thickness5") * 1000.0).round(), 10.0);
        assert_eq!((thickness("Thickness2") * 1000.0).round(), 110.0);
        assert_eq!((thickness("Thickness4") * 1000.0).round(), 210.0);
        // The shadow takes the outermost edge's shape, or the edges would cover it.
        assert!(
            lua.contains("ElementShape3 = Input { Value = 1, }"),
            "{lua}"
        );
        assert_eq!((thickness("Thickness3") * 1000.0).round(), 210.0);
    }

    #[test]
    fn a_shadow_follows_the_outline_when_there_is_no_rim() {
        let mut settings = Settings::default();
        settings.palette[0].shadow = Some("#FF8800".to_owned());
        let bytes = render(
            &transcript(VERTICAL, vec![cue(1.0, 2.0, Some(1), &["はい"])]),
            &settings,
            "seed",
        )
        .unwrap();
        let (_, _, sequence, _) = opened(&bytes);
        let lua = title_lua(&sequence, 0);
        assert!(
            lua.contains("ElementShape3 = Input { Value = 1, }"),
            "{lua}"
        );
        let thickness: f64 = lua
            .split("Thickness3 = Input { Value = ")
            .nth(1)
            .unwrap()
            .split(',')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!((thickness * 10_000.0).round(), 1045.0);
    }

    #[test]
    fn a_shadow_keeps_the_glyph_shape_without_any_edge() {
        let mut settings = Settings::default();
        settings.palette[0].outline_width = 0.0;
        settings.palette[0].shadow = Some("#FF8800".to_owned());
        let bytes = render(
            &transcript(VERTICAL, vec![cue(1.0, 2.0, Some(1), &["はい"])]),
            &settings,
            "seed",
        )
        .unwrap();
        let (_, _, sequence, _) = opened(&bytes);
        let lua = title_lua(&sequence, 0);
        assert!(lua.contains("Enabled3 = Input { Value = 1, }"), "{lua}");
        assert!(lua.contains("Green3 = Input"), "{lua}");
        assert!(!lua.contains("ElementShape3"), "{lua}");
        assert!(!lua.contains("Thickness3"), "{lua}");
    }

    #[test]
    fn a_transparent_rim_and_no_shadow_write_no_elements() {
        let bytes = default_render(VERTICAL, vec![cue(1.0, 2.0, Some(2), &["はい"])]);
        let (_, _, sequence, _) = opened(&bytes);
        let lua = title_lua(&sequence, 0);
        assert!(!lua.contains("Enabled4"), "{lua}");
        assert!(!lua.contains("Thickness4"), "{lua}");
        assert!(!lua.contains("Enabled3"), "{lua}");
    }

    #[test]
    fn the_position_and_size_follow_the_measured_scale() {
        let bytes = default_render(VERTICAL, vec![cue(1.0, 2.0, Some(1), &["はい"])]);
        let (_, _, sequence, _) = opened(&bytes);
        let lua = title_lua(&sequence, 0);
        // x 360 of 720; y 960 of 1280 measured from the bottom.
        assert!(
            lua.contains("Center = Input { Value = { 0.5, 0.25 }, }"),
            "{lua}"
        );
        assert!(
            lua.contains(&format!(
                "Size = Input {{ Value = {}, }}",
                50.0 * 2.0 / (0.5387 * 720.0)
            )),
            "{lua}"
        );
        assert!(lua.contains("Width = Input { Value = 720, }"), "{lua}");
        assert!(lua.contains("Height = Input { Value = 1280, }"), "{lua}");
        assert!(
            lua.contains("Font = Input { Value = \"Noto Sans JP\", }"),
            "{lua}"
        );
    }

    #[test]
    fn line_breaks_and_quotes_survive_lua() {
        assert_eq!(lua_text("a\"b\\c\nd"), "\"a\\\"b\\\\c\\nd\"");
        let bytes = default_render(VERTICAL, vec![cue(1.0, 2.0, Some(1), &["一", "二"])]);
        let (_, _, sequence, _) = opened(&bytes);
        assert!(title_lua(&sequence, 0).contains("Value = \"一\\n二\""));
    }

    #[test]
    fn a_fractional_frame_rate_stays_exact() {
        let bytes = default_render(NTSC, vec![cue(0.0, 1.0, Some(1), &["はい"])]);
        let (_, folder, sequence, _) = opened(&bytes);
        // 10 s at 29.97 is 300 frames (299.7 rounded).
        assert_eq!(inner(&sequence, "Duration")[0], "300");
        assert_eq!(
            inner(&folder, "FrameRate"),
            vec![hex(&(30_000.0f64 / 1001.0).to_le_bytes()) + "0000000000000000"]
        );
        assert_eq!(
            inner(&sequence, "MediaTimemapBA")[0],
            format!("02{}", hex(&(30_000.0f64 / 1001.0).to_be_bytes()))
        );
    }

    #[test]
    fn a_video_without_audio_has_no_audio_track() {
        let bytes = default_render(NTSC, vec![cue(0.0, 1.0, Some(1), &["はい"])]);
        let (_, folder, sequence, _) = opened(&bytes);
        assert!(sequence.contains("<AudioTrackVec/>"));
        assert!(!sequence.contains("Sm2TiAudioClip"));
        assert!(folder.contains("<EmbeddedAudioVec/>"));
        assert!(!folder.contains("BtAudioInfo"));
    }

    #[test]
    fn audio_details_reach_the_media_pool() {
        let bytes = default_render(VERTICAL, vec![cue(0.0, 1.0, Some(1), &["はい"])]);
        let (_, folder, sequence, _) = opened(&bytes);
        assert_eq!(sequence.matches("<Sm2TiAudioClip ").count(), 1);
        let tracks = &inner(&folder, "TracksBA")[0];
        // 44100, then 2 channels, then the samples Resolve itself counted for this audio.
        assert!(tracks.contains("0000ac44"), "{tracks}");
        assert!(
            tracks.contains(&hex(&1_323_168u64.to_be_bytes())),
            "{tracks}"
        );
    }

    #[test]
    fn the_source_path_is_escaped_in_xml_and_exact_in_the_clip() {
        let bytes = default_render(VERTICAL, vec![cue(0.0, 1.0, Some(1), &["はい"])]);
        let (_, folder, sequence, _) = opened(&bytes);
        assert_eq!(
            inner(&sequence, "MediaFilePath")[0],
            "/videos/a b&amp;c.mp4"
        );
        assert!(folder.contains("<Name>a b&amp;c.mp4</Name>"));
        assert!(folder.contains("<Name>a b&amp;c</Name>"));
        let clip = &inner(&folder, "Clip")[0];
        assert!(clip.contains(&hex(b"/videos")), "{clip}");
        assert!(clip.contains(&hex(b"a b&c.mp4")), "{clip}");
    }

    #[test]
    fn windows_paths_keep_their_folder_and_name() {
        assert_eq!(split_path("C:\\動画\\a.mp4"), ("C:\\動画", "a.mp4"));
        assert_eq!(split_path("/v/a.mp4"), ("/v", "a.mp4"));
        assert_eq!(split_path("a.mp4"), ("", "a.mp4"));
        let clip = clip_blob("C:\\v\\a.mp4", 0, Some("id"));
        assert!(clip.contains(&hex(b"C:\\v")), "{clip}");
    }

    #[test]
    fn a_new_export_gets_new_ids_so_resolve_does_not_reuse_a_pool_clip() {
        let one = Ids { seed: "one" }.uuid("7");
        let two = Ids { seed: "two" }.uuid("7");
        assert_ne!(one, two);
        assert_eq!(one, Ids { seed: "one" }.uuid("7"));
        assert_eq!(one.len(), 36);
        assert_eq!(&one[14..15], "4");
        let bytes = default_render(VERTICAL, vec![cue(0.0, 1.0, Some(1), &["はい"])]);
        let (_, _, sequence, name) = opened(&bytes);
        assert!(sequence.contains(&format!(
            "<Sm2SequenceContainer DbId=\"{}\">",
            name.trim_start_matches("SeqContainer/")
                .trim_end_matches(".xml")
        )));
    }

    #[test]
    fn ctime_matches_resolves_layout() {
        // The sample's file: 04:41:21 in Japan on 5 November 2026.
        assert_eq!(ctime(1_793_821_281), "Wed Nov  4 19:41:21 2026");
        assert_eq!(ctime(0), "Thu Jan  1 00:00:00 1970");
        assert_eq!(ctime(1_709_164_800), "Thu Feb 29 00:00:00 2024");
    }

    #[test]
    fn a_missing_source_has_no_modification_time() {
        assert_eq!(modified_micros(Path::new("/nonexistent/video.mp4")), 0);
        let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert!(modified_micros(&here) > 0);
    }

    #[test]
    fn a_broken_template_or_slot_is_an_error() {
        let ids = Ids { seed: "s" };
        assert_eq!(
            fill("{{a", &ids, &[]).unwrap_err().to_string(),
            "broken timeline template"
        );
        assert_eq!(
            fill("{{a}}", &ids, &[]).unwrap_err().to_string(),
            "timeline template slot a"
        );
    }

    #[test]
    fn sections_are_kept_or_dropped() {
        assert_eq!(section("a{{#x}}b{{/x}}c", "x", true), "abc");
        assert_eq!(section("a{{#x}}b{{/x}}c", "x", false), "ac");
        assert_eq!(section("abc", "x", true), "abc");
    }

    #[test]
    fn a_colour_with_no_channels_reads_as_zero() {
        assert_eq!(channel("#FF", 1), 1.0);
        assert_eq!(channel("#FF", 3), 0.0);
    }

    #[test]
    fn a_title_sits_where_fcpxml_puts_it() {
        let placed = Cue {
            x: 180.0,
            ..cue(1.0, 2.0, Some(1), &["はい"])
        };
        let doc = transcript(VERTICAL, vec![placed]);
        let fcpxml = crate::export::render_fcpxml(&doc, &Settings::default());
        // FCPXML: percent of the frame height from the centre, y up.
        assert!(
            fcpxml.contains("<adjust-transform position=\"-14.062 -25.000\"/>"),
            "{fcpxml}"
        );
        let bytes = render(&doc, &Settings::default(), "seed").unwrap();
        let (_, _, sequence, _) = opened(&bytes);
        // Text+: fractions of the frame, y from the bottom. Both name pixel (180, 960).
        assert!(
            title_lua(&sequence, 0).contains("Center = Input { Value = { 0.25, 0.25 }, }"),
            "{}",
            title_lua(&sequence, 0)
        );
    }

    #[test]
    fn the_text_is_as_large_as_fcpxml_draws_it() {
        let metrics = Metrics {
            font_px: 35.0,
            rendered_scale: 1.1540740740740742,
            ..METRICS
        };
        let doc = Transcript::new(
            PathBuf::from("/v/a.mp4"),
            VERTICAL,
            metrics,
            Vec::new(),
            Vec::new(),
            vec![cue(1.0, 2.0, Some(1), &["はい"])],
        );
        let (_, _, sequence, _) = opened(&render(&doc, &Settings::default(), "seed").unwrap());
        let lua = title_lua(&sequence, 0);
        let size: f64 = lua
            .split("Size = Input { Value = ")
            .nth(1)
            .unwrap()
            .split(',')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        // About 40 px on a 720 px wide frame.
        assert_eq!((size * 0.5387 * 720.0 * 10.0).round(), 404.0);
    }
}
