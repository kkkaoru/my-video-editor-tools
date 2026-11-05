//! User-editable preferences, stored as JSON in the app data directory.

use crate::error::{io_context, AppError, AppResult};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const DEFAULT_WHITE: &str = "#FFFFFF";
const BLACK: &str = "#000000";
const MAX_FILL_WIDTH: f64 = 0.2;
const MAX_OUTLINE_WIDTH: f64 = 0.5;
const MAX_RIM_WIDTH: f64 = 0.2;
const MAX_SHADOW_OFFSET: f64 = 0.2;
const MAX_SHADOW_BLUR: f64 = 50.0;
const DEFAULT_SHADOW_OFFSET: f64 = 0.03;
const DEFAULT_SHADOW_BLUR: f64 = 5.0;
/// 1: `.drt` became the default subtitle format, for people who already had settings too.
/// 2: a rim of no width became transparent, as it is by default.
const SETTINGS_VERSION: u64 = 2;
const UI_LANGUAGES: [&str; 8] = ["en", "ja", "zh-Hans", "zh-Hant", "ko", "de", "fr", "es"];
/// Speaker 1 is white on a black outline. The others follow the repository's `SINGLE_FILL` rule: a dark
/// fill at 7:1 or better against the white band, with the black rim keeping it readable on any video.
const FIRST_SPEAKER_FILL: &str = "#FFFFFF";
const FIRST_SPEAKER_OUTLINE: &str = "#000000";
const OTHER_FILLS: [&str; MAX_SPEAKER_COLORS - 1] = [
    "#005A9C", "#6A1FA8", "#0B672C", "#A3123A", "#8A4500", "#7A1D7A", "#006262",
];
/// Speaker-less words stay grey instead of borrowing someone's identity.
const UNASSIGNED_FILL: &str = "#595959";
const FONT_FAMILY: &str = "Noto Sans JP";
/// 52 px at 1080p: the repository's single-speaker caption size.
const DEFAULT_FONT_RATIO: f64 = 0.048;
/// Style 20's white band is about 11% of the font size on each side.
const DEFAULT_OUTLINE_RATIO: f64 = 0.115;
/// Style 20's rim: 3 units per side at font size 88, outside the band.
const DEFAULT_RIM_RATIO: f64 = 3.0 / 88.0;
const DEFAULT_WIDTH_RATIO: f64 = 0.9;
const DEFAULT_SAFE_PERCENT: f64 = 5.0;
const MAX_SAFE_PERCENT: f64 = 40.0;
const MAX_SAFE_TOTAL_PERCENT: f64 = 80.0;
/// Whisper detects the language itself unless the user picks one.
const DEFAULT_LANGUAGE: &str = "auto";
const MAX_OFFSET_PERCENT: f64 = 50.0;
/// A font family goes into XML attributes, so a name holding these cannot be used.
pub const FONT_FAMILY_FORBIDDEN: [char; 3] = ['"', '<', '&'];
/// `EM_ADVANCE`: the line advance measured for the repository's captions (`caption_layout_file`).
const DEFAULT_LINE_HEIGHT: f64 = 1.448;
/// A speaker model the catalog no longer has: a saved selection of it reads as no selection.
const RETIRED_DIARIZE_MODEL: &str = "nemotron-3-diarization";
/// The speaker models separate at most eight voices; more would only be guesswork.
const MAX_SPEAKER_COUNT: u32 = 8;
/// One colour per speaker the models can tell apart.
pub const MAX_SPEAKER_COLORS: usize = MAX_SPEAKER_COUNT as usize;
/// Most recordings have one voice; 0 (automatic) stays available and is kept once saved.
const DEFAULT_SPEAKER_COUNT: u32 = 1;

/// One speaker's three colours, each with its thickness as a fraction of the font size. Outward from the
/// glyph: the fill thickens the letters, then the outline, then the rim.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SpeakerColor {
    pub fill: String,
    #[serde(default)]
    pub fill_width: f64,
    pub outline: String,
    #[serde(default = "default_outline_width")]
    pub outline_width: f64,
    /// `None` is transparent: no rim at all, whatever `rim_width` says.
    #[serde(default)]
    pub rim: Option<String>,
    #[serde(default)]
    pub rim_width: f64,
    /// `None` is no shadow. Only `.drt` draws one; Resolve ignores them in FCPXML and TTML.
    #[serde(default)]
    pub shadow: Option<String>,
    /// Text+ offsets: drawn about offset x 2 x em pixels away; a negative `shadow_y` falls downward.
    #[serde(default = "default_shadow_x")]
    pub shadow_x: f64,
    #[serde(default = "default_shadow_y")]
    pub shadow_y: f64,
    #[serde(default = "default_shadow_blur")]
    pub shadow_blur: f64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    FaceAbove,
    FaceBelow,
    VideoTop,
    VideoBottom,
}

/// Where captions sit; offsets are percent of the frame so they survive any resolution.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Placement {
    pub anchor: Anchor,
    pub offset_x_percent: f64,
    pub offset_y_percent: f64,
}

/// Which subtitle file a job writes, beside the transcript that is always written.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SubtitleFormat {
    /// A DaVinci Resolve timeline of Text+ titles, the only format that keeps every colour and shadow.
    #[default]
    Drt,
    Fcpxml,
    Ttml,
    Srt,
}

/// How the app's own window looks.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UiTheme {
    #[default]
    System,
    Light,
    Dark,
}

/// An unknown theme, from a newer version or a hand-edited file, must not stop the app from opening.
fn known_ui_theme<'de, D: Deserializer<'de>>(values: D) -> Result<UiTheme, D::Error> {
    Ok(serde_json::from_value(Value::deserialize(values)?).unwrap_or_default())
}

/// As `known_ui_theme`: anything but a language the app has follows the system.
fn known_ui_language<'de, D: Deserializer<'de>>(values: D) -> Result<String, D::Error> {
    let tag = Value::deserialize(values)?;
    let known = tag.as_str().filter(|tag| UI_LANGUAGES.contains(tag));
    Ok(known.unwrap_or_default().to_owned())
}

/// `default` lets settings saved by an older version gain newer fields.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// One entry per speaker slot, editable in the app; only the first `MAX_SPEAKER_COLORS` are used.
    pub palette: Vec<SpeakerColor>,
    pub unassigned: SpeakerColor,
    pub font_family: String,
    /// Font size as a fraction of the frame's short edge.
    pub font_ratio: f64,
    /// Distance between caption lines as a multiple of the font size.
    pub line_height: f64,
    /// Longest caption line as a fraction of the frame width.
    pub width_ratio: f64,
    /// Left and right margins captions keep clear of, as percent of the frame width.
    pub safe_left_percent: f64,
    pub safe_right_percent: f64,
    pub placement: Placement,
    pub subtitle_format: SubtitleFormat,
    pub language: String,
    pub diarize_model: String,
    /// How many people speak: 0 lets the model decide, 1 skips speaker separation altogether.
    pub speaker_count: u32,
    pub face_model: String,
    /// Extra folders searched for already-downloaded models.
    pub model_dirs: Vec<PathBuf>,
    /// The saved profile dropped subtitle files follow; empty means the settings above.
    pub active_profile: String,
    /// One of `UI_LANGUAGES`; empty follows the operating system.
    #[serde(deserialize_with = "known_ui_language")]
    pub ui_language: String,
    #[serde(deserialize_with = "known_ui_theme")]
    pub ui_theme: UiTheme,
    /// Always this build's version: what a file holds is read by `parse_settings`.
    #[serde(skip_deserializing)]
    pub settings_version: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            palette: std::iter::once(SpeakerColor {
                fill: FIRST_SPEAKER_FILL.to_owned(),
                fill_width: 0.0,
                outline: FIRST_SPEAKER_OUTLINE.to_owned(),
                outline_width: DEFAULT_OUTLINE_RATIO,
                rim: None,
                rim_width: DEFAULT_RIM_RATIO,
                shadow: None,
                shadow_x: DEFAULT_SHADOW_OFFSET,
                shadow_y: -DEFAULT_SHADOW_OFFSET,
                shadow_blur: DEFAULT_SHADOW_BLUR,
            })
            .chain(OTHER_FILLS.iter().map(|fill| white_outlined(fill)))
            .collect(),
            unassigned: white_outlined(UNASSIGNED_FILL),
            font_family: FONT_FAMILY.to_owned(),
            font_ratio: DEFAULT_FONT_RATIO,
            line_height: DEFAULT_LINE_HEIGHT,
            width_ratio: DEFAULT_WIDTH_RATIO,
            safe_left_percent: DEFAULT_SAFE_PERCENT,
            safe_right_percent: DEFAULT_SAFE_PERCENT,
            placement: Placement {
                anchor: Anchor::FaceAbove,
                offset_x_percent: 0.0,
                offset_y_percent: 0.0,
            },
            subtitle_format: SubtitleFormat::default(),
            language: DEFAULT_LANGUAGE.to_owned(),
            diarize_model: String::new(),
            speaker_count: DEFAULT_SPEAKER_COUNT,
            face_model: "yunet".to_owned(),
            model_dirs: Vec::new(),
            active_profile: String::new(),
            ui_language: String::new(),
            ui_theme: UiTheme::default(),
            settings_version: SETTINGS_VERSION,
        }
    }
}

const fn default_shadow_x() -> f64 {
    DEFAULT_SHADOW_OFFSET
}

const fn default_shadow_y() -> f64 {
    -DEFAULT_SHADOW_OFFSET
}

const fn default_shadow_blur() -> f64 {
    DEFAULT_SHADOW_BLUR
}

const fn default_outline_width() -> f64 {
    DEFAULT_OUTLINE_RATIO
}

/// Every speaker after the first: a white band, and no rim until one is chosen.
fn white_outlined(fill: &str) -> SpeakerColor {
    SpeakerColor {
        fill: fill.to_owned(),
        fill_width: 0.0,
        outline: DEFAULT_WHITE.to_owned(),
        outline_width: DEFAULT_OUTLINE_RATIO,
        rim: None,
        rim_width: DEFAULT_RIM_RATIO,
        shadow: None,
        shadow_x: DEFAULT_SHADOW_OFFSET,
        shadow_y: -DEFAULT_SHADOW_OFFSET,
        shadow_blur: DEFAULT_SHADOW_BLUR,
    }
}

fn is_hex_color(value: &str) -> bool {
    let digits = value.strip_prefix('#').unwrap_or("");
    digits.len() == 6 && digits.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_ratio(name: &str, value: f64, low: f64, high: f64) -> AppResult<()> {
    if value.is_finite() && (low..=high).contains(&value) {
        return Ok(());
    }
    Err(AppError::Invalid(format!(
        "{name} must be between {low} and {high}, got {value}"
    )))
}

fn validate_color(color: &SpeakerColor) -> AppResult<()> {
    let rim_is_usable = color.rim.as_deref().is_none_or(is_hex_color);
    let shadow_is_usable = color.shadow.as_deref().is_none_or(is_hex_color);
    if !is_hex_color(&color.fill)
        || !is_hex_color(&color.outline)
        || !rim_is_usable
        || !shadow_is_usable
    {
        return Err(AppError::Invalid(format!(
            "colours must be #RRGGBB: {} / {} / {}",
            color.fill,
            color.outline,
            color.rim.as_deref().unwrap_or("transparent")
        )));
    }
    validate_ratio("fill_width", color.fill_width, 0.0, MAX_FILL_WIDTH)?;
    validate_ratio("outline_width", color.outline_width, 0.0, MAX_OUTLINE_WIDTH)?;
    if color.rim.is_some() {
        validate_ratio("rim_width", color.rim_width, 0.0, MAX_RIM_WIDTH)?;
    }
    if color.shadow.is_some() {
        validate_ratio(
            "shadow_x",
            color.shadow_x,
            -MAX_SHADOW_OFFSET,
            MAX_SHADOW_OFFSET,
        )?;
        validate_ratio(
            "shadow_y",
            color.shadow_y,
            -MAX_SHADOW_OFFSET,
            MAX_SHADOW_OFFSET,
        )?;
        validate_ratio("shadow_blur", color.shadow_blur, 0.0, MAX_SHADOW_BLUR)?;
    }
    Ok(())
}

/// Moves `outline_ratio` and `rim_ratio`, which applied to every speaker in earlier versions, into each
/// speaker of a settings object or a profile's look, so old files keep the look they had.
pub(crate) fn migrate_look(look: &mut Value) {
    let outline = look
        .get("outline_ratio")
        .and_then(Value::as_f64)
        .unwrap_or(DEFAULT_OUTLINE_RATIO);
    let rim = look
        .get("rim_ratio")
        .and_then(Value::as_f64)
        .unwrap_or(DEFAULT_RIM_RATIO);
    let palette = look.get_mut("palette").and_then(Value::as_array_mut);
    palette
        .into_iter()
        .flatten()
        .for_each(|color| migrate_color(color, outline, rim));
    if let Some(color) = look.get_mut("unassigned") {
        migrate_color(color, outline, rim);
    }
}

fn migrate_color(color: &mut Value, outline: f64, rim: f64) {
    let Some(fields) = color.as_object_mut() else {
        return;
    };
    if fields.contains_key("outline_width") || fields.contains_key("rim") {
        return;
    }
    fields.insert("outline_width".to_owned(), json!(outline));
    // The rim was always black, and added nothing around a black outline.
    let on_black = fields
        .get("outline")
        .and_then(Value::as_str)
        .is_some_and(|hex| hex.eq_ignore_ascii_case(BLACK));
    if rim > 0.0 && !on_black {
        fields.insert("rim".to_owned(), json!(BLACK));
        fields.insert("rim_width".to_owned(), json!(rim));
    }
}

fn clear_widthless_rim(color: &mut Value) {
    let Some(fields) = color.as_object_mut() else {
        return;
    };
    let width = fields.get("rim_width").and_then(Value::as_f64);
    if width.unwrap_or(0.0) == 0.0 {
        fields.insert("rim".to_owned(), Value::Null);
    }
}

/// Reads a settings file's text through the migration of earlier versions' fields.
fn parse_settings(text: &str) -> serde_json::Result<Settings> {
    let mut value: Value = serde_json::from_str(text)?;
    migrate_look(&mut value);
    let saved_version = value.get("settings_version").and_then(Value::as_u64);
    if saved_version.is_none() {
        if let Some(fields) = value.as_object_mut() {
            fields.insert("subtitle_format".to_owned(), json!("drt"));
        }
    }
    if saved_version.unwrap_or(0) < SETTINGS_VERSION {
        let palette = value.get_mut("palette").and_then(Value::as_array_mut);
        palette.into_iter().flatten().for_each(clear_widthless_rim);
        if let Some(color) = value.get_mut("unassigned") {
            clear_widthless_rim(color);
        }
    }
    serde_json::from_value(value)
}

impl SpeakerColor {
    /// How far this speaker's caption reaches past the glyph, as a fraction of the font size.
    #[must_use]
    pub fn edge_ratio(&self) -> f64 {
        let rim = if self.rim.is_some() {
            self.rim_width
        } else {
            0.0
        };
        self.fill_width + self.outline_width + rim
    }
}

impl Settings {
    /// Rejects values that would produce broken subtitle files; the UI and JSON both pass here.
    pub fn validate(&self) -> AppResult<()> {
        if self.speaker_count > MAX_SPEAKER_COUNT {
            return Err(AppError::Invalid(format!(
                "speaker_count must be at most {MAX_SPEAKER_COUNT}, got {}",
                self.speaker_count
            )));
        }
        if self.palette.is_empty() {
            return Err(AppError::Invalid(
                "the speaker palette needs at least one colour".into(),
            ));
        }
        self.palette.iter().try_for_each(validate_color)?;
        validate_color(&self.unassigned)?;
        validate_ratio("font_ratio", self.font_ratio, 0.01, 0.3)?;
        validate_ratio("line_height", self.line_height, 1.0, 3.0)?;
        validate_ratio("width_ratio", self.width_ratio, 0.2, 1.0)?;
        validate_ratio(
            "safe_left_percent",
            self.safe_left_percent,
            0.0,
            MAX_SAFE_PERCENT,
        )?;
        validate_ratio(
            "safe_right_percent",
            self.safe_right_percent,
            0.0,
            MAX_SAFE_PERCENT,
        )?;
        // Two values at their limit pass only when their sum still leaves a line to draw on.
        validate_ratio(
            "safe_left_percent + safe_right_percent",
            self.safe_left_percent + self.safe_right_percent,
            0.0,
            MAX_SAFE_TOTAL_PERCENT,
        )?;
        validate_ratio(
            "offset_x_percent",
            self.placement.offset_x_percent,
            -MAX_OFFSET_PERCENT,
            MAX_OFFSET_PERCENT,
        )?;
        validate_ratio(
            "offset_y_percent",
            self.placement.offset_y_percent,
            -MAX_OFFSET_PERCENT,
            MAX_OFFSET_PERCENT,
        )?;
        if self.font_family.trim().is_empty() || self.font_family.contains(FONT_FAMILY_FORBIDDEN) {
            return Err(AppError::Invalid(
                "font family is empty or has markup characters".into(),
            ));
        }
        if self.language.is_empty() || !self.language.bytes().all(|byte| byte.is_ascii_alphabetic())
        {
            return Err(AppError::Invalid(format!(
                "language must be a code such as ja, got {:?}",
                self.language
            )));
        }
        Ok(())
    }

    /// The furthest any speaker's caption reaches past the glyph, as a fraction of the font size; a line
    /// must fit whoever ends up speaking it.
    #[must_use]
    pub fn widest_edge_ratio(&self) -> f64 {
        self.palette
            .iter()
            .chain(std::iter::once(&self.unassigned))
            .map(SpeakerColor::edge_ratio)
            .fold(0.0, f64::max)
    }

    /// The colour for a 1-based speaker; extra speakers reuse slots instead of failing a finished run.
    #[must_use]
    pub fn color_for(&self, speaker: Option<u32>) -> &SpeakerColor {
        let Some(number) = speaker.filter(|number| *number > 0) else {
            return &self.unassigned;
        };
        let slot = (number as usize - 1) % self.palette.len().min(MAX_SPEAKER_COLORS);
        &self.palette[slot]
    }

    pub fn load(path: &Path) -> AppResult<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path).map_err(io_context("read settings"))?;
        let mut settings: Self = parse_settings(&text)
            .map_err(|error| AppError::Invalid(format!("settings file: {error}")))?;
        // Earlier versions saved ten colours; a surplus one must not stop the app from opening.
        settings.palette.truncate(MAX_SPEAKER_COLORS);
        if settings.diarize_model == RETIRED_DIARIZE_MODEL {
            settings.diarize_model.clear();
        }
        settings.validate()?;
        Ok(settings)
    }

    pub fn save(&self, path: &Path) -> AppResult<()> {
        self.validate()?;
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| AppError::Invalid(error.to_string()))?;
        crate::paths::write_atomic(path, text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG relative luminance of `#RRGGBB`.
    fn luminance(hex: &str) -> f64 {
        let linear = |range: std::ops::Range<usize>| {
            let value = f64::from(u8::from_str_radix(&hex[range], 16).unwrap()) / 255.0;
            if value <= 0.03928 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(1..3) + 0.7152 * linear(3..5) + 0.0722 * linear(5..7)
    }

    fn contrast(left: &str, right: &str) -> f64 {
        let (left, right) = (luminance(left), luminance(right));
        (left.max(right) + 0.05) / (left.min(right) + 0.05)
    }

    #[test]
    fn defaults_have_one_colour_per_possible_speaker() {
        let settings = Settings::default();
        assert_eq!(settings.palette.len(), 8);
        assert_eq!(settings.palette[0].fill, "#FFFFFF");
        assert_eq!(settings.palette[0].outline, "#000000");
        assert_eq!(settings.palette[1].fill, "#005A9C");
        assert_eq!(settings.palette[1].outline, "#FFFFFF");
        assert_eq!(settings.palette[7].fill, "#006262");
        settings.validate().unwrap();
    }

    #[test]
    fn speakers_map_to_slots_and_missing_speakers_stay_grey() {
        let settings = Settings::default();
        assert_eq!(settings.color_for(Some(1)).fill, "#FFFFFF");
        assert_eq!(settings.color_for(Some(2)).fill, "#005A9C");
        assert_eq!(settings.color_for(Some(9)).fill, "#FFFFFF");
        assert_eq!(settings.color_for(None).fill, "#595959");
        assert_eq!(settings.color_for(Some(0)).fill, "#595959");
    }

    #[test]
    fn bad_colours_are_rejected() {
        let mut settings = Settings::default();
        settings.palette[0].fill = "red".to_owned();
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "colours must be #RRGGBB: red / #000000 / transparent"
        );
    }

    #[test]
    fn empty_palette_is_rejected() {
        let settings = Settings {
            palette: Vec::new(),
            ..Settings::default()
        };
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "the speaker palette needs at least one colour"
        );
    }

    #[test]
    fn out_of_range_offset_is_rejected() {
        let mut settings = Settings::default();
        settings.placement.offset_x_percent = 51.0;
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "offset_x_percent must be between -50 and 50, got 51"
        );
    }

    #[test]
    fn markup_in_font_family_is_rejected() {
        let settings = Settings {
            font_family: "A<B".to_owned(),
            ..Settings::default()
        };
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "font family is empty or has markup characters"
        );
    }

    #[test]
    fn language_must_be_a_code() {
        let settings = Settings {
            language: "j a".to_owned(),
            ..Settings::default()
        };
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "language must be a code such as ja, got \"j a\""
        );
    }

    #[test]
    fn anchor_serializes_in_snake_case() {
        assert_eq!(
            serde_json::to_string(&Anchor::FaceAbove).unwrap(),
            "\"face_above\""
        );
    }

    #[test]
    fn out_of_range_ratios_and_vertical_offsets_are_rejected() {
        let settings = Settings {
            font_ratio: f64::NAN,
            ..Settings::default()
        };
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "font_ratio must be between 0.01 and 0.3, got NaN"
        );
        let mut settings = Settings::default();
        settings.placement.offset_y_percent = -51.0;
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "offset_y_percent must be between -50 and 50, got -51"
        );
    }

    #[test]
    fn a_bad_unassigned_colour_is_rejected() {
        let mut settings = Settings::default();
        settings.unassigned.outline = "#12345".to_owned();
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "colours must be #RRGGBB: #595959 / #12345 / transparent"
        );
    }

    #[test]
    fn settings_round_trip_and_a_missing_file_means_defaults() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/settings-round-trip");
        let _ = std::fs::remove_dir_all(&root);
        let file = root.join("settings.json");
        assert_eq!(Settings::load(&file).unwrap(), Settings::default());
        let settings = Settings {
            font_ratio: 0.05,
            ..Settings::default()
        };
        settings.save(&file).unwrap();
        assert_eq!(Settings::load(&file).unwrap(), settings);
    }

    #[test]
    fn invalid_settings_are_neither_saved_nor_loaded() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/settings-invalid");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("settings.json");
        let invalid = Settings {
            language: String::new(),
            ..Settings::default()
        };
        assert_eq!(
            invalid.save(&file).unwrap_err().to_string(),
            "language must be a code such as ja, got \"\""
        );
        assert!(!file.exists());
        std::fs::write(&file, serde_json::to_string(&invalid).unwrap()).unwrap();
        assert_eq!(
            Settings::load(&file).unwrap_err().to_string(),
            "language must be a code such as ja, got \"\""
        );
    }

    #[test]
    fn settings_from_an_older_version_gain_the_new_fields() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/settings-older");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("settings.json");
        std::fs::write(&file, br#"{"language":"ja","transcribe_model":"old"}"#).unwrap();
        let settings = Settings::load(&file).unwrap();
        assert_eq!(settings.language, "ja");
        assert_eq!(settings.active_profile, "");
    }

    #[test]
    fn the_contrast_measure_spans_one_to_twenty_one() {
        assert_eq!(contrast("#000000", "#FFFFFF").round(), 21.0);
        assert_eq!(contrast("#123456", "#123456"), 1.0);
    }

    #[test]
    fn every_default_colour_reads_at_seven_to_one_against_its_outline() {
        let settings = Settings::default();
        let too_faint: Vec<&str> = settings
            .palette
            .iter()
            .chain(std::iter::once(&settings.unassigned))
            .filter(|color| contrast(&color.fill, &color.outline) < 7.0)
            .map(|color| color.fill.as_str())
            .collect();
        assert_eq!(too_faint, Vec::<&str>::new());
    }

    #[test]
    fn only_speaker_one_sits_on_a_black_outline() {
        let settings = Settings::default();
        let not_white: Vec<&str> = settings.palette[1..]
            .iter()
            .chain(std::iter::once(&settings.unassigned))
            .filter(|color| color.outline != "#FFFFFF")
            .map(|color| color.fill.as_str())
            .collect();
        assert_eq!(not_white, Vec::<&str>::new());
        assert_eq!(settings.palette[0].outline, "#000000");
    }

    #[test]
    fn the_speaker_count_stops_at_eight() {
        let settings = Settings {
            speaker_count: 8,
            ..Settings::default()
        };
        assert!(settings.validate().is_ok());
        let settings = Settings {
            speaker_count: 9,
            ..Settings::default()
        };
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "speaker_count must be at most 8, got 9"
        );
    }

    #[test]
    fn a_line_pitch_outside_the_range_is_rejected() {
        let settings = Settings {
            line_height: 0.5,
            ..Settings::default()
        };
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "line_height must be between 1 and 3, got 0.5"
        );
    }

    #[test]
    fn one_speaker_is_the_default_and_a_saved_automatic_choice_stays() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/settings-speakers");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("settings.json");
        assert_eq!(Settings::default().speaker_count, 1);
        std::fs::write(&file, br#"{"language":"ja"}"#).unwrap();
        assert_eq!(Settings::load(&file).unwrap().speaker_count, 1);
        std::fs::write(&file, br#"{"speaker_count":0}"#).unwrap();
        assert_eq!(Settings::load(&file).unwrap().speaker_count, 0);
    }

    #[test]
    fn colours_stop_at_the_most_speakers_a_model_can_separate() {
        let settings = Settings::default();
        let distinct: std::collections::HashSet<&str> = settings
            .palette
            .iter()
            .map(|color| color.fill.as_str())
            .collect();
        assert_eq!(distinct.len(), 8);
        assert_eq!(MAX_SPEAKER_COUNT as usize, settings.palette.len());
    }

    #[test]
    fn colours_saved_by_an_earlier_version_load_up_to_the_limit() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/settings-ten-colours");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("settings.json");
        let first_eight: Vec<String> = (1..=8)
            .map(|n| format!(r##"{{"fill":"#00000{n}","outline":"#FFFFFF"}}"##))
            .collect();
        // The ninth colour is not even valid: it is dropped before anything reads it.
        std::fs::write(
            &file,
            format!(
                r##"{{"palette":[{},{{"fill":"red","outline":"red"}},{{"fill":"#111111","outline":"#FFFFFF"}}]}}"##,
                first_eight.join(",")
            ),
        )
        .unwrap();
        let settings = Settings::load(&file).unwrap();
        assert_eq!(settings.palette.len(), 8);
        assert_eq!(settings.palette[7].fill, "#000008");
    }

    #[test]
    fn a_longer_palette_in_memory_still_uses_only_the_first_colours() {
        let mut settings = Settings::default();
        settings.palette.push(white_outlined("#123456"));
        assert_eq!(settings.color_for(Some(8)).fill, "#006262");
        assert_eq!(settings.color_for(Some(9)).fill, "#FFFFFF");
    }

    #[test]
    fn the_subtitle_format_defaults_to_drt_and_reads_its_lowercase_names() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/settings-format");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("settings.json");
        std::fs::write(&file, br#"{"language":"ja"}"#).unwrap();
        assert_eq!(
            Settings::load(&file).unwrap().subtitle_format,
            SubtitleFormat::Drt
        );
        std::fs::write(&file, br#"{"settings_version":1,"subtitle_format":"ttml"}"#).unwrap();
        assert_eq!(
            Settings::load(&file).unwrap().subtitle_format,
            SubtitleFormat::Ttml
        );
        std::fs::write(&file, br#"{"settings_version":1,"subtitle_format":"srt"}"#).unwrap();
        assert_eq!(
            Settings::load(&file).unwrap().subtitle_format,
            SubtitleFormat::Srt
        );
        std::fs::write(&file, br#"{"settings_version":1,"subtitle_format":"vtt"}"#).unwrap();
        assert!(Settings::load(&file).is_err());
    }

    #[test]
    fn settings_from_before_versions_move_to_drt_once_and_a_later_choice_stays() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/settings-version");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("settings.json");
        std::fs::write(&file, br#"{"subtitle_format":"fcpxml","language":"en"}"#).unwrap();
        let mut migrated = Settings::load(&file).unwrap();
        assert_eq!(migrated.subtitle_format, SubtitleFormat::Drt);
        assert_eq!(migrated.language, "en");

        migrated.subtitle_format = SubtitleFormat::Fcpxml;
        migrated.save(&file).unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(saved["settings_version"], 2);
        assert_eq!(saved["subtitle_format"], "fcpxml");
        let reloaded = Settings::load(&file).unwrap();
        assert_eq!(reloaded.subtitle_format, SubtitleFormat::Fcpxml);
        assert_eq!(reloaded.settings_version, 2);
    }

    #[test]
    fn a_file_saved_by_the_current_version_keeps_fcpxml() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/settings-version-kept");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("settings.json");
        std::fs::write(
            &file,
            br#"{"settings_version":1,"subtitle_format":"fcpxml"}"#,
        )
        .unwrap();
        assert_eq!(
            Settings::load(&file).unwrap().subtitle_format,
            SubtitleFormat::Fcpxml
        );
    }

    #[test]
    fn a_rim_of_no_width_saved_before_version_two_becomes_transparent() {
        let settings = parse_settings(
            r##"{"settings_version":1,"subtitle_format":"fcpxml",
                "palette":[
                    {"fill":"#111111","outline":"#222222","outline_width":0.1,"rim":"#000000","rim_width":0.0},
                    {"fill":"#111111","outline":"#222222","outline_width":0.1,"rim":"#000000"},
                    {"fill":"#111111","outline":"#222222","outline_width":0.1,"rim":"#000000","rim_width":0.03}
                ],
                "unassigned":{"fill":"#111111","outline":"#222222","outline_width":0.1,"rim":"#000000","rim_width":0.0}}"##,
        )
        .unwrap();
        assert_eq!(settings.subtitle_format, SubtitleFormat::Fcpxml);
        assert_eq!(settings.palette[0].rim, None);
        assert_eq!(settings.palette[1].rim, None);
        assert_eq!(settings.palette[2].rim.as_deref(), Some("#000000"));
        assert_eq!(settings.unassigned.rim, None);
    }

    #[test]
    fn settings_without_a_version_get_both_migrations() {
        let settings = parse_settings(
            r##"{"subtitle_format":"fcpxml",
                "palette":[{"fill":"#111111","outline":"#222222","outline_width":0.1,"rim":"#000000","rim_width":0.0}]}"##,
        )
        .unwrap();
        assert_eq!(settings.subtitle_format, SubtitleFormat::Drt);
        assert_eq!(settings.palette[0].rim, None);
    }

    #[test]
    fn a_colour_that_is_not_an_object_is_still_an_error() {
        assert!(parse_settings(r#"{"settings_version":1,"palette":["red"]}"#).is_err());
    }

    #[test]
    fn a_rim_of_no_width_saved_by_version_two_stays() {
        let settings = parse_settings(
            r##"{"settings_version":2,
                "palette":[{"fill":"#111111","outline":"#222222","outline_width":0.1,"rim":"#000000","rim_width":0.0}],
                "unassigned":{"fill":"#111111","outline":"#222222","outline_width":0.1,"rim":"#000000","rim_width":0.0}}"##,
        )
        .unwrap();
        assert_eq!(settings.palette[0].rim.as_deref(), Some("#000000"));
        assert_eq!(settings.unassigned.rim.as_deref(), Some("#000000"));
    }

    #[test]
    fn the_ui_language_and_theme_default_to_the_system_for_old_files() {
        let old = parse_settings("{}").unwrap();
        assert_eq!(old.ui_language, "");
        assert_eq!(old.ui_theme, UiTheme::System);
    }

    #[test]
    fn every_ui_language_and_theme_is_read_back() {
        let languages = ["en", "ja", "zh-Hans", "zh-Hant", "ko", "de", "fr", "es"].map(|tag| {
            let settings = Settings {
                ui_language: tag.to_owned(),
                ..Settings::default()
            };
            parse_settings(&serde_json::to_string(&settings).unwrap())
                .unwrap()
                .ui_language
        });
        assert_eq!(
            languages,
            ["en", "ja", "zh-Hans", "zh-Hant", "ko", "de", "fr", "es"]
        );
        let themes = [UiTheme::System, UiTheme::Light, UiTheme::Dark].map(|ui_theme| {
            let settings = Settings {
                ui_theme,
                ..Settings::default()
            };
            serde_json::to_value(&settings).unwrap()["ui_theme"].clone()
        });
        assert_eq!(themes, ["system", "light", "dark"]);
        assert_eq!(
            parse_settings(r#"{"ui_theme":"dark"}"#).unwrap().ui_theme,
            UiTheme::Dark
        );
    }

    #[test]
    fn an_unknown_ui_language_or_theme_goes_back_to_the_system() {
        let unknown = parse_settings(r#"{"ui_language":"tlh","ui_theme":"neon"}"#).unwrap();
        assert_eq!(unknown.ui_language, "");
        assert_eq!(unknown.ui_theme, UiTheme::System);
        let wrong_type = parse_settings(r#"{"ui_language":5,"ui_theme":null}"#).unwrap();
        assert_eq!(wrong_type.ui_language, "");
        assert_eq!(wrong_type.ui_theme, UiTheme::System);
    }

    #[test]
    fn a_settings_file_that_is_not_an_object_is_an_error() {
        assert!(parse_settings("5").is_err());
    }

    #[test]
    fn safe_zones_default_to_five_percent_and_old_settings_gain_them() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!((old.safe_left_percent, old.safe_right_percent), (5.0, 5.0));
    }

    #[test]
    fn a_safe_zone_out_of_range_is_refused() {
        let wide = Settings {
            safe_left_percent: 41.0,
            ..Settings::default()
        };
        assert_eq!(
            wide.validate().unwrap_err().to_string(),
            "safe_left_percent must be between 0 and 40, got 41"
        );
        let negative = Settings {
            safe_right_percent: -1.0,
            ..Settings::default()
        };
        assert_eq!(
            negative.validate().unwrap_err().to_string(),
            "safe_right_percent must be between 0 and 40, got -1"
        );
    }

    #[test]
    fn safe_zones_together_must_leave_room() {
        let settings = Settings {
            safe_left_percent: 40.0,
            safe_right_percent: 40.5,
            ..Settings::default()
        };
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "safe_right_percent must be between 0 and 40, got 40.5"
        );
        let exact = Settings {
            safe_left_percent: 40.0,
            safe_right_percent: 40.0,
            ..Settings::default()
        };
        assert!(exact.validate().is_ok());
    }

    #[test]
    fn a_shadow_saved_by_an_earlier_version_is_ignored() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/settings-old-shadow");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("settings.json");
        // Even a value the old validation refused must not stop the app from opening.
        std::fs::write(
            &file,
            br#"{"language":"ja","shadow":{"enabled":true,"strength":9.0}}"#,
        )
        .unwrap();
        let settings = Settings::load(&file).unwrap();
        assert_eq!(settings.language, "ja");
        let saved = serde_json::to_value(&settings).unwrap();
        assert_eq!(saved.get("shadow"), None);
    }

    #[test]
    fn a_saved_choice_of_the_retired_speaker_model_reads_as_no_choice() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/settings-retired-model");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("settings.json");
        std::fs::write(&file, br#"{"diarize_model":"nemotron-3-diarization"}"#).unwrap();
        assert_eq!(Settings::load(&file).unwrap().diarize_model, "");
        std::fs::write(&file, br#"{"diarize_model":"nemotron-3.5-transcribe-q2l"}"#).unwrap();
        assert_eq!(
            Settings::load(&file).unwrap().diarize_model,
            "nemotron-3.5-transcribe-q2l"
        );
    }

    #[test]
    fn the_first_speaker_keeps_a_black_outline_and_the_others_start_without_a_rim() {
        let settings = Settings::default();
        assert_eq!(
            settings.palette[0],
            SpeakerColor {
                fill: "#FFFFFF".to_owned(),
                fill_width: 0.0,
                outline: "#000000".to_owned(),
                outline_width: 0.115,
                rim: None,
                rim_width: 3.0 / 88.0,
                shadow: None,
                shadow_x: DEFAULT_SHADOW_OFFSET,
                shadow_y: -DEFAULT_SHADOW_OFFSET,
                shadow_blur: DEFAULT_SHADOW_BLUR,
            }
        );
        assert_eq!(settings.palette[1].outline, "#FFFFFF");
        assert_eq!(settings.palette[1].rim, None);
        assert_eq!(settings.unassigned.rim, None);
        assert_eq!(settings.widest_edge_ratio(), 0.115);
    }

    #[test]
    fn a_transparent_rim_adds_no_edge_and_its_width_is_not_checked() {
        let mut settings = Settings::default();
        settings.palette[1].rim_width = 9.0;
        assert_eq!(settings.palette[1].edge_ratio(), 0.115);
        assert!(settings.validate().is_ok());
        settings.palette[1].rim = Some("#112233".to_owned());
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "rim_width must be between 0 and 0.2, got 9"
        );
    }

    #[test]
    fn every_width_and_the_rim_colour_are_validated() {
        let mut settings = Settings::default();
        settings.palette[0].fill_width = 0.3;
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "fill_width must be between 0 and 0.2, got 0.3"
        );
        let mut settings = Settings::default();
        settings.palette[0].outline_width = 0.6;
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "outline_width must be between 0 and 0.5, got 0.6"
        );
        let mut settings = Settings::default();
        settings.palette[0].rim = Some("black".to_owned());
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "colours must be #RRGGBB: #FFFFFF / #000000 / black"
        );
    }

    #[test]
    fn the_widest_edge_counts_every_layer_of_every_speaker() {
        let mut settings = Settings::default();
        settings.palette[3].fill_width = 0.02;
        settings.palette[3].rim = Some("#000000".to_owned());
        settings.palette[3].rim_width = 0.03;
        assert_eq!(settings.widest_edge_ratio(), 0.165);
    }

    #[test]
    fn settings_from_before_per_speaker_widths_keep_their_look() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/settings-migrate");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("settings.json");
        std::fs::write(
            &file,
            br##"{"outline_ratio":0.2,"rim_ratio":0.05,
                "palette":[{"fill":"#FFFFFF","outline":"#000000"},{"fill":"#005A9C","outline":"#FFFFFF"}],
                "unassigned":{"fill":"#595959","outline":"#FFFFFF"}}"##,
        )
        .unwrap();
        let settings = Settings::load(&file).unwrap();
        assert_eq!(
            settings.palette[0],
            SpeakerColor {
                fill: "#FFFFFF".to_owned(),
                fill_width: 0.0,
                outline: "#000000".to_owned(),
                outline_width: 0.2,
                rim: None,
                rim_width: 0.0,
                shadow: None,
                shadow_x: DEFAULT_SHADOW_OFFSET,
                shadow_y: -DEFAULT_SHADOW_OFFSET,
                shadow_blur: DEFAULT_SHADOW_BLUR,
            }
        );
        assert_eq!(
            settings.palette[1],
            SpeakerColor {
                fill: "#005A9C".to_owned(),
                fill_width: 0.0,
                outline: "#FFFFFF".to_owned(),
                outline_width: 0.2,
                rim: Some("#000000".to_owned()),
                rim_width: 0.05,
                shadow: None,
                shadow_x: DEFAULT_SHADOW_OFFSET,
                shadow_y: -DEFAULT_SHADOW_OFFSET,
                shadow_blur: DEFAULT_SHADOW_BLUR,
            }
        );
        assert_eq!(settings.unassigned.rim_width, 0.05);
    }

    #[test]
    fn an_old_file_with_the_rim_off_stays_without_one() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test/settings-migrate-off");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("settings.json");
        std::fs::write(
            &file,
            br##"{"rim_ratio":0.0,"palette":[{"fill":"#005A9C","outline":"#FFFFFF"}]}"##,
        )
        .unwrap();
        let settings = Settings::load(&file).unwrap();
        assert_eq!(settings.palette[0].rim, None);
        assert_eq!(settings.palette[0].outline_width, 0.115);
    }

    #[test]
    fn an_old_file_without_ratios_gets_the_old_defaults() {
        let mut look =
            serde_json::json!({ "palette": [{ "fill": "#005A9C", "outline": "#FFFFFF" }] });
        migrate_look(&mut look);
        assert_eq!(
            look,
            serde_json::json!({ "palette": [{
                "fill": "#005A9C",
                "outline": "#FFFFFF",
                "outline_width": 0.115,
                "rim": "#000000",
                "rim_width": 3.0 / 88.0
            }] })
        );
    }

    #[test]
    fn colours_already_in_the_new_form_are_left_alone() {
        let new_form = serde_json::json!({
            "outline_ratio": 0.3,
            "palette": [{ "fill": "#111111", "outline": "#222222", "outline_width": 0.1, "rim": null }],
            "unassigned": "not an object"
        });
        let mut look = new_form.clone();
        migrate_look(&mut look);
        assert_eq!(look, new_form);
    }

    #[test]
    fn a_colour_may_leave_out_its_widths_and_rim() {
        let color: SpeakerColor =
            serde_json::from_str(r##"{"fill":"#111111","outline":"#222222"}"##).unwrap();
        assert_eq!(
            color,
            SpeakerColor {
                fill: "#111111".to_owned(),
                fill_width: 0.0,
                outline: "#222222".to_owned(),
                outline_width: 0.115,
                rim: None,
                rim_width: 0.0,
                shadow: None,
                shadow_x: DEFAULT_SHADOW_OFFSET,
                shadow_y: -DEFAULT_SHADOW_OFFSET,
                shadow_blur: DEFAULT_SHADOW_BLUR,
            }
        );
    }

    #[test]
    fn a_colour_from_before_shadows_reads_without_one() {
        let color: SpeakerColor =
            serde_json::from_str(r##"{"fill":"#111111","outline":"#222222"}"##).unwrap();
        assert_eq!(color.shadow, None);
        assert_eq!(color.shadow_x, 0.03);
        assert_eq!(color.shadow_y, -0.03);
        assert_eq!(color.shadow_blur, 5.0);
    }

    #[test]
    fn a_shadow_is_validated_only_when_there_is_one() {
        let mut settings = Settings::default();
        settings.palette[0].shadow_blur = 99.0;
        assert!(settings.validate().is_ok());
        settings.palette[0].shadow = Some("#000000".to_owned());
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "shadow_blur must be between 0 and 50, got 99"
        );
        let mut settings = Settings::default();
        settings.palette[0].shadow = Some("#000000".to_owned());
        settings.palette[0].shadow_x = 0.3;
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "shadow_x must be between -0.2 and 0.2, got 0.3"
        );
        let mut settings = Settings::default();
        settings.palette[0].shadow = Some("#000000".to_owned());
        settings.palette[0].shadow_y = -0.3;
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "shadow_y must be between -0.2 and 0.2, got -0.3"
        );
        let mut settings = Settings::default();
        settings.palette[0].shadow = Some("black".to_owned());
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "colours must be #RRGGBB: #FFFFFF / #000000 / transparent"
        );
    }

    #[test]
    fn drt_has_a_lowercase_name() {
        assert_eq!(
            serde_json::to_string(&SubtitleFormat::Drt).unwrap(),
            "\"drt\""
        );
    }
}
