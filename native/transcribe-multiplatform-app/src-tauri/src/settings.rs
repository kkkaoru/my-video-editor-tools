//! User-editable preferences, stored as JSON in the app data directory.

use crate::error::{io_context, AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const DEFAULT_WHITE: &str = "#FFFFFF";
/// Speaker 1 is white on a black outline. The others follow the repository's `SINGLE_FILL` rule: a dark
/// fill at 7:1 or better against the white band, with the black rim keeping it readable on any video.
const FIRST_SPEAKER_FILL: &str = "#FFFFFF";
const FIRST_SPEAKER_OUTLINE: &str = "#000000";
const OTHER_FILLS: [&str; 9] = [
    "#005A9C", "#6A1FA8", "#0B672C", "#A3123A", "#8A4500", "#7A1D7A", "#006262", "#3A3AA8",
    "#5C5C00",
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
/// Whisper detects the language itself unless the user picks one.
const DEFAULT_LANGUAGE: &str = "auto";
const MAX_OFFSET_PERCENT: f64 = 50.0;
/// `EM_ADVANCE`: the line advance measured for the repository's captions (`caption_layout_file`).
const DEFAULT_LINE_HEIGHT: f64 = 1.448;
/// The speaker models separate at most eight voices; more would only be guesswork.
const MAX_SPEAKER_COUNT: u32 = 8;
/// Style 20: a shadow past the rim that is clearly visible.
const DEFAULT_SHADOW_STRENGTH: f64 = 0.8;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SpeakerColor {
    pub fill: String,
    pub outline: String,
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

/// Drop shadow behind caption text; `strength` is its opacity, 0 to 1.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct CaptionShadow {
    pub enabled: bool,
    pub strength: f64,
}

/// `default` lets settings saved by an older version gain newer fields.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// One entry per speaker slot; ten by default and editable in the app.
    pub palette: Vec<SpeakerColor>,
    pub unassigned: SpeakerColor,
    pub font_family: String,
    /// Font size as a fraction of the frame's short edge.
    pub font_ratio: f64,
    /// Outline width as a fraction of the font size.
    pub outline_ratio: f64,
    /// Black rim outside the outline, as a fraction of the font size; zero turns it off.
    pub rim_ratio: f64,
    /// Distance between caption lines as a multiple of the font size.
    pub line_height: f64,
    pub shadow: CaptionShadow,
    /// Longest caption line as a fraction of the frame width.
    pub width_ratio: f64,
    pub placement: Placement,
    pub language: String,
    pub diarize_model: String,
    /// How many people speak: 0 lets the model decide, 1 skips speaker separation altogether.
    pub speaker_count: u32,
    pub face_model: String,
    /// Extra folders searched for already-downloaded models.
    pub model_dirs: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            palette: std::iter::once(SpeakerColor {
                fill: FIRST_SPEAKER_FILL.to_owned(),
                outline: FIRST_SPEAKER_OUTLINE.to_owned(),
            })
            .chain(OTHER_FILLS.iter().map(|fill| SpeakerColor {
                fill: (*fill).to_owned(),
                outline: DEFAULT_WHITE.to_owned(),
            }))
            .collect(),
            unassigned: SpeakerColor {
                fill: UNASSIGNED_FILL.to_owned(),
                outline: DEFAULT_WHITE.to_owned(),
            },
            font_family: FONT_FAMILY.to_owned(),
            font_ratio: DEFAULT_FONT_RATIO,
            outline_ratio: DEFAULT_OUTLINE_RATIO,
            rim_ratio: DEFAULT_RIM_RATIO,
            line_height: DEFAULT_LINE_HEIGHT,
            shadow: CaptionShadow {
                enabled: true,
                strength: DEFAULT_SHADOW_STRENGTH,
            },
            width_ratio: DEFAULT_WIDTH_RATIO,
            placement: Placement {
                anchor: Anchor::VideoBottom,
                offset_x_percent: 0.0,
                offset_y_percent: 0.0,
            },
            language: DEFAULT_LANGUAGE.to_owned(),
            diarize_model: String::new(),
            speaker_count: 0,
            face_model: "yunet".to_owned(),
            model_dirs: Vec::new(),
        }
    }
}

fn is_hex_color(value: &str) -> bool {
    let digits = value.strip_prefix('#').unwrap_or("");
    digits.len() == 6 && digits.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_color(color: &SpeakerColor) -> AppResult<()> {
    if is_hex_color(&color.fill) && is_hex_color(&color.outline) {
        return Ok(());
    }
    Err(AppError::Invalid(format!(
        "colours must be #RRGGBB: {} / {}",
        color.fill, color.outline
    )))
}

fn validate_ratio(name: &str, value: f64, low: f64, high: f64) -> AppResult<()> {
    if value.is_finite() && (low..=high).contains(&value) {
        return Ok(());
    }
    Err(AppError::Invalid(format!(
        "{name} must be between {low} and {high}, got {value}"
    )))
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
        validate_ratio("outline_ratio", self.outline_ratio, 0.01, 0.5)?;
        validate_ratio("rim_ratio", self.rim_ratio, 0.0, 0.2)?;
        validate_ratio("line_height", self.line_height, 1.0, 3.0)?;
        validate_ratio("shadow.strength", self.shadow.strength, 0.0, 1.0)?;
        validate_ratio("width_ratio", self.width_ratio, 0.2, 1.0)?;
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
        if self.font_family.trim().is_empty() || self.font_family.contains(['"', '<', '&']) {
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

    /// The colour for a 1-based speaker; extra speakers reuse slots instead of failing a finished run.
    #[must_use]
    pub fn color_for(&self, speaker: Option<u32>) -> &SpeakerColor {
        let Some(number) = speaker.filter(|number| *number > 0) else {
            return &self.unassigned;
        };
        let slot = (number as usize - 1) % self.palette.len();
        &self.palette[slot]
    }

    pub fn load(path: &Path) -> AppResult<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path).map_err(io_context("read settings"))?;
        let settings: Self = serde_json::from_str(&text)
            .map_err(|error| AppError::Invalid(format!("settings file: {error}")))?;
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
    fn defaults_have_ten_distinct_speaker_colours() {
        let settings = Settings::default();
        assert_eq!(settings.palette.len(), 10);
        assert_eq!(settings.palette[0].fill, "#FFFFFF");
        assert_eq!(settings.palette[0].outline, "#000000");
        assert_eq!(settings.palette[1].fill, "#005A9C");
        assert_eq!(settings.palette[1].outline, "#FFFFFF");
        assert_eq!(settings.palette[9].fill, "#5C5C00");
        settings.validate().unwrap();
    }

    #[test]
    fn speakers_map_to_slots_and_missing_speakers_stay_grey() {
        let settings = Settings::default();
        assert_eq!(settings.color_for(Some(1)).fill, "#FFFFFF");
        assert_eq!(settings.color_for(Some(2)).fill, "#005A9C");
        assert_eq!(settings.color_for(Some(11)).fill, "#FFFFFF");
        assert_eq!(settings.color_for(None).fill, "#595959");
        assert_eq!(settings.color_for(Some(0)).fill, "#595959");
    }

    #[test]
    fn bad_colours_are_rejected() {
        let mut settings = Settings::default();
        settings.palette[0].fill = "red".to_owned();
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "colours must be #RRGGBB: red / #000000"
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
            "colours must be #RRGGBB: #595959 / #12345"
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
    fn a_shadow_stronger_than_opaque_is_rejected() {
        let mut settings = Settings::default();
        settings.shadow.strength = 1.5;
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "shadow.strength must be between 0 and 1, got 1.5"
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
        assert_eq!(
            settings.shadow,
            CaptionShadow {
                enabled: true,
                strength: 0.8
            }
        );
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
    fn a_rim_wider_than_the_limit_is_rejected() {
        let settings = Settings {
            rim_ratio: 0.5,
            ..Settings::default()
        };
        assert_eq!(
            settings.validate().unwrap_err().to_string(),
            "rim_ratio must be between 0 and 0.2, got 0.5"
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
}
