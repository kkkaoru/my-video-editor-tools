//! User-editable preferences, stored as JSON in the app data directory.

use crate::error::{io_context, AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const DEFAULT_WHITE: &str = "#FFFFFF";
/// The first three continue the repository's speaker lane colours (purple, green, light blue).
const DEFAULT_FILLS: [&str; 10] = [
    "#C48CFF", "#7CDC8C", "#6CD4FF", "#FF9E5E", "#FF7FA8", "#F2D64B", "#5EE0C8", "#8FA8FF", "#E08CE0", "#B5E05E",
];
/// Speaker-less words stay grey instead of borrowing someone's identity.
const UNASSIGNED_FILL: &str = "#C8C8C8";
const FONT_FAMILY: &str = "Noto Sans JP";
/// 52 px at 1080p: the repository's single-speaker caption size.
const DEFAULT_FONT_RATIO: f64 = 0.048;
/// Style 20's white band is about 11% of the font size on each side.
const DEFAULT_OUTLINE_RATIO: f64 = 0.11;
const DEFAULT_WIDTH_RATIO: f64 = 0.9;
/// Whisper detects the language itself unless the user picks one.
const DEFAULT_LANGUAGE: &str = "auto";
const MAX_OFFSET_PERCENT: f64 = 50.0;

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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    /// One entry per speaker slot; ten by default and editable in the app.
    pub palette: Vec<SpeakerColor>,
    pub unassigned: SpeakerColor,
    pub font_family: String,
    /// Font size as a fraction of the frame's short edge.
    pub font_ratio: f64,
    /// Outline width as a fraction of the font size.
    pub outline_ratio: f64,
    /// Longest caption line as a fraction of the frame width.
    pub width_ratio: f64,
    pub placement: Placement,
    pub language: String,
    pub transcribe_model: String,
    pub diarize_model: String,
    pub face_model: String,
    /// Extra folders searched for already-downloaded models.
    pub model_dirs: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            palette: DEFAULT_FILLS
                .iter()
                .map(|fill| SpeakerColor { fill: (*fill).to_owned(), outline: DEFAULT_WHITE.to_owned() })
                .collect(),
            unassigned: SpeakerColor { fill: UNASSIGNED_FILL.to_owned(), outline: DEFAULT_WHITE.to_owned() },
            font_family: FONT_FAMILY.to_owned(),
            font_ratio: DEFAULT_FONT_RATIO,
            outline_ratio: DEFAULT_OUTLINE_RATIO,
            width_ratio: DEFAULT_WIDTH_RATIO,
            placement: Placement { anchor: Anchor::VideoBottom, offset_x_percent: 0.0, offset_y_percent: 0.0 },
            language: DEFAULT_LANGUAGE.to_owned(),
            transcribe_model: String::new(),
            diarize_model: String::new(),
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
    Err(AppError::Invalid(format!("colours must be #RRGGBB: {} / {}", color.fill, color.outline)))
}

fn validate_ratio(name: &str, value: f64, low: f64, high: f64) -> AppResult<()> {
    if value.is_finite() && (low..=high).contains(&value) {
        return Ok(());
    }
    Err(AppError::Invalid(format!("{name} must be between {low} and {high}, got {value}")))
}

impl Settings {
    /// Rejects values that would produce broken subtitle files; the UI and JSON both pass here.
    pub fn validate(&self) -> AppResult<()> {
        if self.palette.is_empty() {
            return Err(AppError::Invalid("the speaker palette needs at least one colour".into()));
        }
        self.palette.iter().try_for_each(validate_color)?;
        validate_color(&self.unassigned)?;
        validate_ratio("font_ratio", self.font_ratio, 0.01, 0.3)?;
        validate_ratio("outline_ratio", self.outline_ratio, 0.01, 0.5)?;
        validate_ratio("width_ratio", self.width_ratio, 0.2, 1.0)?;
        validate_ratio("offset_x_percent", self.placement.offset_x_percent, -MAX_OFFSET_PERCENT, MAX_OFFSET_PERCENT)?;
        validate_ratio("offset_y_percent", self.placement.offset_y_percent, -MAX_OFFSET_PERCENT, MAX_OFFSET_PERCENT)?;
        if self.font_family.trim().is_empty() || self.font_family.contains(['"', '<', '&']) {
            return Err(AppError::Invalid("font family is empty or has markup characters".into()));
        }
        if self.language.is_empty() || !self.language.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            return Err(AppError::Invalid(format!("language must be a code such as ja, got {:?}", self.language)));
        }
        Ok(())
    }

    /// The colour for a 1-based speaker; extra speakers reuse slots instead of failing a finished run.
    #[must_use]
    pub fn color_for(&self, speaker: Option<u32>) -> &SpeakerColor {
        let Some(number) = speaker.filter(|number| *number > 0) else { return &self.unassigned };
        let slot = (number as usize - 1) % self.palette.len();
        &self.palette[slot]
    }

    pub fn load(path: &Path) -> AppResult<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path).map_err(io_context("read settings"))?;
        let settings: Self =
            serde_json::from_str(&text).map_err(|error| AppError::Invalid(format!("settings file: {error}")))?;
        settings.validate()?;
        Ok(settings)
    }

    pub fn save(&self, path: &Path) -> AppResult<()> {
        self.validate()?;
        let text = serde_json::to_string_pretty(self).map_err(|error| AppError::Invalid(error.to_string()))?;
        crate::paths::write_atomic(path, text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_have_ten_distinct_speaker_colours() {
        let settings = Settings::default();
        assert_eq!(settings.palette.len(), 10);
        assert_eq!(settings.palette[0].fill, "#C48CFF");
        assert_eq!(settings.palette[9].fill, "#B5E05E");
        settings.validate().unwrap();
    }

    #[test]
    fn speakers_map_to_slots_and_missing_speakers_stay_grey() {
        let settings = Settings::default();
        assert_eq!(settings.color_for(Some(1)).fill, "#C48CFF");
        assert_eq!(settings.color_for(Some(2)).fill, "#7CDC8C");
        assert_eq!(settings.color_for(Some(11)).fill, "#C48CFF");
        assert_eq!(settings.color_for(None).fill, "#C8C8C8");
        assert_eq!(settings.color_for(Some(0)).fill, "#C8C8C8");
    }

    #[test]
    fn bad_colours_are_rejected() {
        let mut settings = Settings::default();
        settings.palette[0].fill = "red".to_owned();
        assert_eq!(settings.validate().unwrap_err().to_string(), "colours must be #RRGGBB: red / #FFFFFF");
    }

    #[test]
    fn empty_palette_is_rejected() {
        let settings = Settings { palette: Vec::new(), ..Settings::default() };
        assert_eq!(settings.validate().unwrap_err().to_string(), "the speaker palette needs at least one colour");
    }

    #[test]
    fn out_of_range_offset_is_rejected() {
        let mut settings = Settings::default();
        settings.placement.offset_x_percent = 51.0;
        assert_eq!(settings.validate().unwrap_err().to_string(), "offset_x_percent must be between -50 and 50, got 51");
    }

    #[test]
    fn markup_in_font_family_is_rejected() {
        let settings = Settings { font_family: "A<B".to_owned(), ..Settings::default() };
        assert_eq!(settings.validate().unwrap_err().to_string(), "font family is empty or has markup characters");
    }

    #[test]
    fn language_must_be_a_code() {
        let settings = Settings { language: "j a".to_owned(), ..Settings::default() };
        assert_eq!(settings.validate().unwrap_err().to_string(), "language must be a code such as ja, got \"j a\"");
    }

    #[test]
    fn anchor_serializes_in_snake_case() {
        assert_eq!(serde_json::to_string(&Anchor::FaceAbove).unwrap(), "\"face_above\"");
    }
}
