//! Named caption looks the user saves and restores, kept together in one JSON file in the app data folder.

use crate::error::{io_context, AppError, AppResult, UserError};
use crate::settings::{CaptionShadow, Placement, Settings, SpeakerColor};
use serde::{Deserialize, Serialize};
use std::path::Path;

const MAX_NAME_CHARS: usize = 80;

/// What a profile restores: how captions look and where they sit, nothing about models or speech.
/// `default` lets a profile saved by an older version gain newer fields.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CaptionLook {
    pub palette: Vec<SpeakerColor>,
    pub unassigned: SpeakerColor,
    pub font_family: String,
    pub font_ratio: f64,
    pub outline_ratio: f64,
    pub rim_ratio: f64,
    pub line_height: f64,
    pub shadow: CaptionShadow,
    pub width_ratio: f64,
    pub placement: Placement,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub name: String,
    pub look: CaptionLook,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct ProfileFile {
    profiles: Vec<Profile>,
}

impl CaptionLook {
    #[must_use]
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            palette: settings.palette.clone(),
            unassigned: settings.unassigned.clone(),
            font_family: settings.font_family.clone(),
            font_ratio: settings.font_ratio,
            outline_ratio: settings.outline_ratio,
            rim_ratio: settings.rim_ratio,
            line_height: settings.line_height,
            shadow: settings.shadow,
            width_ratio: settings.width_ratio,
            placement: settings.placement,
        }
    }

    /// `settings` wearing this look; everything else in them stays as it is.
    #[must_use]
    pub fn applied_to(self, settings: &Settings) -> Settings {
        Settings {
            palette: self.palette,
            unassigned: self.unassigned,
            font_family: self.font_family,
            font_ratio: self.font_ratio,
            outline_ratio: self.outline_ratio,
            rim_ratio: self.rim_ratio,
            line_height: self.line_height,
            shadow: self.shadow,
            width_ratio: self.width_ratio,
            placement: self.placement,
            ..settings.clone()
        }
    }
}

impl Default for CaptionLook {
    fn default() -> Self {
        Self::from_settings(&Settings::default())
    }
}

pub fn load(path: &Path) -> AppResult<Vec<Profile>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path).map_err(io_context("read profiles"))?;
    let file: ProfileFile = serde_json::from_str(&text)
        .map_err(|error| AppError::Invalid(format!("profiles file: {error}")))?;
    Ok(file.profiles)
}

pub fn save(path: &Path, profiles: Vec<Profile>) -> AppResult<()> {
    let text = serde_json::to_string_pretty(&ProfileFile { profiles })
        .map_err(|error| AppError::Invalid(error.to_string()))?;
    crate::paths::write_atomic(path, text.as_bytes())
}

/// The name as stored: trimmed, and short enough to show in a list.
pub fn clean_name(name: &str) -> AppResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_NAME_CHARS {
        return Err(AppError::Invalid(format!(
            "a profile name needs 1 to {MAX_NAME_CHARS} characters"
        )));
    }
    Ok(trimmed.to_owned())
}

fn missing(name: &str) -> AppError {
    UserError::ProfileMissing {
        name: name.to_owned(),
    }
    .into()
}

/// The list with `profile` added at the end; an existing name is never replaced silently.
pub fn added(mut profiles: Vec<Profile>, profile: Profile) -> AppResult<Vec<Profile>> {
    if profiles.iter().any(|row| row.name == profile.name) {
        return Err(UserError::ProfileExists { name: profile.name }.into());
    }
    profiles.push(profile);
    Ok(profiles)
}

/// The list with the profile of the same name replaced where it stands.
pub fn replaced(mut profiles: Vec<Profile>, profile: Profile) -> AppResult<Vec<Profile>> {
    let slot = profiles
        .iter_mut()
        .find(|row| row.name == profile.name)
        .ok_or_else(|| missing(&profile.name))?;
    *slot = profile;
    Ok(profiles)
}

pub fn removed(mut profiles: Vec<Profile>, name: &str) -> AppResult<Vec<Profile>> {
    let position = profiles
        .iter()
        .position(|row| row.name == name)
        .ok_or_else(|| missing(name))?;
    profiles.remove(position);
    Ok(profiles)
}

pub fn look_named(profiles: &[Profile], name: &str) -> AppResult<CaptionLook> {
    profiles
        .iter()
        .find(|row| row.name == name)
        .map(|row| row.look.clone())
        .ok_or_else(|| missing(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Anchor;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target-test")
            .join(format!("profiles-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn profile(name: &str, font_ratio: f64) -> Profile {
        Profile {
            name: name.to_owned(),
            look: CaptionLook {
                font_ratio,
                ..CaptionLook::default()
            },
        }
    }

    #[test]
    fn a_look_round_trips_through_settings_and_leaves_the_rest_alone() {
        let settings = Settings {
            font_ratio: 0.07,
            font_family: "Hiragino Sans".to_owned(),
            language: "ja".to_owned(),
            speaker_count: 3,
            ..Settings::default()
        };
        let other = Settings {
            language: "de".to_owned(),
            speaker_count: 2,
            ..Settings::default()
        };
        let worn = CaptionLook::from_settings(&settings).applied_to(&other);
        assert_eq!(worn.font_ratio, 0.07);
        assert_eq!(worn.font_family, "Hiragino Sans");
        assert_eq!(worn.language, "de");
        assert_eq!(worn.speaker_count, 2);
    }

    #[test]
    fn a_missing_file_means_no_profiles() {
        assert_eq!(
            load(&scratch("missing").join("profiles.json")).unwrap(),
            Vec::<Profile>::new()
        );
    }

    #[test]
    fn profiles_round_trip_through_the_file() {
        let file = scratch("round-trip").join("profiles.json");
        save(&file, vec![profile("A", 0.05)]).unwrap();
        assert_eq!(load(&file).unwrap(), vec![profile("A", 0.05)]);
    }

    #[test]
    fn a_profile_from_an_older_version_gains_the_new_fields() {
        let file = scratch("older").join("profiles.json");
        std::fs::write(
            &file,
            br#"{"profiles":[{"name":"Old","look":{"font_ratio":0.06}}]}"#,
        )
        .unwrap();
        let rows = load(&file).unwrap();
        assert_eq!(rows[0].look.font_ratio, 0.06);
        assert_eq!(rows[0].look.font_family, "Noto Sans JP");
        assert_eq!(rows[0].look.placement.anchor, Anchor::FaceAbove);
    }

    #[test]
    fn a_file_without_the_list_means_no_profiles() {
        let file = scratch("empty-object").join("profiles.json");
        std::fs::write(&file, b"{}").unwrap();
        assert_eq!(load(&file).unwrap(), Vec::<Profile>::new());
    }

    #[test]
    fn a_broken_file_is_reported_not_replaced() {
        let file = scratch("broken").join("profiles.json");
        std::fs::write(&file, b"{").unwrap();
        assert_eq!(
            load(&file).unwrap_err().to_string(),
            "profiles file: EOF while parsing an object at line 1 column 1"
        );
        assert_eq!(std::fs::read(&file).unwrap(), b"{");
    }

    #[test]
    fn an_unreadable_file_names_the_step() {
        let folder = scratch("unreadable");
        assert!(load(&folder)
            .unwrap_err()
            .to_string()
            .starts_with("read profiles: "));
    }

    #[test]
    fn names_are_trimmed_and_bounded() {
        assert_eq!(clean_name("  配信用  ").unwrap(), "配信用");
        assert_eq!(clean_name(&"あ".repeat(80)).unwrap().chars().count(), 80);
        assert_eq!(
            clean_name("   ").unwrap_err().to_string(),
            "a profile name needs 1 to 80 characters"
        );
        assert_eq!(
            clean_name(&"あ".repeat(81)).unwrap_err().to_string(),
            "a profile name needs 1 to 80 characters"
        );
    }

    #[test]
    fn adding_keeps_order_and_refuses_an_existing_name() {
        let rows = added(vec![profile("A", 0.05)], profile("B", 0.06)).unwrap();
        assert_eq!(rows, vec![profile("A", 0.05), profile("B", 0.06)]);
        assert_eq!(
            serde_json::to_value(added(rows, profile("A", 0.07)).unwrap_err()).unwrap(),
            serde_json::json!({ "code": "profile_exists", "name": "A" })
        );
    }

    #[test]
    fn replacing_keeps_the_position() {
        let rows = vec![profile("A", 0.05), profile("B", 0.06)];
        assert_eq!(
            replaced(rows, profile("A", 0.07)).unwrap(),
            vec![profile("A", 0.07), profile("B", 0.06)]
        );
    }

    #[test]
    fn replacing_an_unknown_name_is_an_error() {
        assert_eq!(
            serde_json::to_value(replaced(Vec::new(), profile("A", 0.07)).unwrap_err()).unwrap(),
            serde_json::json!({ "code": "profile_missing", "name": "A" })
        );
    }

    #[test]
    fn removing_drops_only_that_name() {
        assert_eq!(
            removed(vec![profile("A", 0.05), profile("B", 0.06)], "A").unwrap(),
            vec![profile("B", 0.06)]
        );
        assert_eq!(
            removed(Vec::new(), "A").unwrap_err().to_string(),
            "profile A does not exist"
        );
    }

    #[test]
    fn looking_up_a_name_returns_its_look() {
        let rows = vec![profile("A", 0.05)];
        assert_eq!(look_named(&rows, "A").unwrap().font_ratio, 0.05);
        assert_eq!(
            look_named(&rows, "B").unwrap_err().to_string(),
            "profile B does not exist"
        );
    }
}
