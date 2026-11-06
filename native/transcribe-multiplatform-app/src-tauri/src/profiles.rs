//! Named caption looks the user saves and restores, kept together in one JSON file in the app data folder.

use crate::error::{io_context, AppError, AppResult, UserError};
use crate::settings::{migrate_look, Placement, Settings, SpeakerColor, MAX_SPEAKER_COLORS};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;
use std::path::Path;

const MAX_NAME_CHARS: usize = 80;
/// Far above any real profile: an imported file comes from anywhere, and must not fill the memory.
const MAX_IMPORT_BYTES: u64 = 1 << 20;
const FIRST_COPY_NUMBER: u32 = 2;

/// What a profile restores: how captions look and where they sit, nothing about models or speech.
/// `default` lets a profile saved by an older version gain newer fields.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CaptionLook {
    pub palette: Vec<SpeakerColor>,
    pub font_family: String,
    pub font_ratio: f64,
    pub line_height: f64,
    pub width_ratio: f64,
    pub safe_left_percent: f64,
    pub safe_right_percent: f64,
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
            font_family: settings.font_family.clone(),
            font_ratio: settings.font_ratio,
            line_height: settings.line_height,
            width_ratio: settings.width_ratio,
            safe_left_percent: settings.safe_left_percent,
            safe_right_percent: settings.safe_right_percent,
            placement: settings.placement,
        }
    }

    /// `settings` wearing this look; everything else in them stays as it is.
    #[must_use]
    pub fn applied_to(self, settings: &Settings) -> Settings {
        Settings {
            // A profile saved by an earlier version may hold more colours than can be used.
            palette: self.palette.into_iter().take(MAX_SPEAKER_COLORS).collect(),
            font_family: self.font_family,
            font_ratio: self.font_ratio,
            line_height: self.line_height,
            width_ratio: self.width_ratio,
            safe_left_percent: self.safe_left_percent,
            safe_right_percent: self.safe_right_percent,
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

fn parse_profile_file(text: &str) -> serde_json::Result<ProfileFile> {
    let mut value: Value = serde_json::from_str(text)?;
    let rows = value.get_mut("profiles").and_then(Value::as_array_mut);
    rows.into_iter()
        .flatten()
        .filter_map(|row| row.get_mut("look"))
        .for_each(migrate_look);
    serde_json::from_value(value)
}

fn parse_import(bytes: &[u8]) -> serde_json::Result<Profile> {
    let mut value: Value = serde_json::from_slice(bytes)?;
    if let Some(look) = value.get_mut("look") {
        migrate_look(look);
    }
    serde_json::from_value(value)
}

pub fn load(path: &Path) -> AppResult<Vec<Profile>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path).map_err(io_context("read profiles"))?;
    let file: ProfileFile = parse_profile_file(&text)
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
    let unusable = trimmed.is_empty()
        || trimmed.chars().count() > MAX_NAME_CHARS
        || trimmed.chars().any(char::is_control);
    if unusable {
        return Err(AppError::Invalid(format!(
            "a profile name needs 1 to {MAX_NAME_CHARS} characters and no control characters"
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

/// Writes one profile where the user chose, atomically like the app's own files.
pub fn export(profiles: &[Profile], name: &str, path: &Path) -> AppResult<()> {
    let profile = Profile {
        name: name.to_owned(),
        look: look_named(profiles, name)?,
    };
    let text = serde_json::to_string_pretty(&profile)
        .map_err(|error| AppError::Invalid(error.to_string()))?;
    crate::paths::write_atomic(path, text.as_bytes())
}

/// Reads a profile file from anywhere; only a size-limited file holding a valid profile is accepted.
pub fn read_import(path: &Path) -> AppResult<Profile> {
    let invalid = || -> AppError {
        UserError::ProfileInvalid {
            file: path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
        }
        .into()
    };
    let file = std::fs::File::open(path).map_err(io_context("open profile file"))?;
    let mut bytes = Vec::new();
    file.take(MAX_IMPORT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io_context("read profile file"))?;
    if bytes.len() as u64 > MAX_IMPORT_BYTES {
        return Err(invalid());
    }
    let profile = parse_import(&bytes).map_err(|_| invalid())?;
    let name = clean_name(&profile.name).map_err(|_| invalid())?;
    profile
        .look
        .clone()
        .applied_to(&Settings::default())
        .validate()
        .map_err(|_| invalid())?;
    Ok(Profile { name, ..profile })
}

/// `name`, or `name (2)` and so on when taken, still within the length limit.
#[must_use]
pub fn unused_name(profiles: &[Profile], name: &str) -> String {
    let taken = |candidate: &str| profiles.iter().any(|row| row.name == candidate);
    if !taken(name) {
        return name.to_owned();
    }
    (FIRST_COPY_NUMBER..)
        .map(|number| {
            let suffix = format!(" ({number})");
            let room = MAX_NAME_CHARS - suffix.chars().count();
            let stem: String = name.chars().take(room).collect();
            format!("{}{suffix}", stem.trim_end())
        })
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| name.to_owned())
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
            "a profile name needs 1 to 80 characters and no control characters"
        );
        assert_eq!(
            clean_name(&"あ".repeat(81)).unwrap_err().to_string(),
            "a profile name needs 1 to 80 characters and no control characters"
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

    fn import_scratch(name: &str) -> std::path::PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target-test")
            .join(format!("profile-file-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn named(name: &str) -> Profile {
        Profile {
            name: name.to_owned(),
            look: CaptionLook::default(),
        }
    }

    #[test]
    fn an_exported_profile_imports_as_it_was() {
        let root = import_scratch("round-trip");
        let file = root.join("sub/mine.json");
        let profile = Profile {
            name: "配信用".to_owned(),
            look: CaptionLook {
                font_family: "Hiragino Sans".to_owned(),
                ..CaptionLook::default()
            },
        };
        export(std::slice::from_ref(&profile), "配信用", &file).unwrap();
        assert_eq!(read_import(&file).unwrap(), profile);
        assert!(!root.join("sub/mine.json.partial").exists());
    }

    #[test]
    fn exporting_an_unknown_profile_names_it() {
        let root = import_scratch("export-missing");
        assert_eq!(
            export(&[], "A", &root.join("a.json"))
                .unwrap_err()
                .to_string(),
            "profile A does not exist"
        );
        assert!(!root.join("a.json").exists());
    }

    #[test]
    fn broken_files_are_refused_with_their_name_only() {
        let root = import_scratch("broken");
        let file = root.join("bad.json");
        std::fs::write(&file, b"{").unwrap();
        assert_eq!(
            serde_json::to_value(read_import(&file).unwrap_err()).unwrap(),
            serde_json::json!({ "code": "profile_invalid", "file": "bad.json" })
        );
        std::fs::write(&file, [0xff, 0xfe]).unwrap();
        assert_eq!(
            read_import(&file).unwrap_err().to_string(),
            "bad.json is not a valid profile file"
        );
        std::fs::write(&file, br#"{"name":"A"}"#).unwrap();
        assert_eq!(
            read_import(&file).unwrap_err().to_string(),
            "bad.json is not a valid profile file"
        );
    }

    #[test]
    fn a_file_over_the_limit_is_refused() {
        let root = import_scratch("too-large");
        let file = root.join("big.json");
        std::fs::File::create(&file)
            .unwrap()
            .set_len((1 << 20) + 1)
            .unwrap();
        assert_eq!(
            read_import(&file).unwrap_err().to_string(),
            "big.json is not a valid profile file"
        );
    }

    #[test]
    fn imported_values_pass_the_same_validation_as_settings() {
        let root = import_scratch("values");
        let file = root.join("p.json");
        std::fs::write(&file, br#"{"name":"A","look":{"font_family":"x<y"}}"#).unwrap();
        assert!(read_import(&file).is_err());
        std::fs::write(&file, br#"{"name":"A","look":{"font_ratio":9.0}}"#).unwrap();
        assert!(read_import(&file).is_err());
        std::fs::write(
            &file,
            br##"{"name":"A","look":{"palette":[{"fill":"red","outline":"#FFFFFF"}]}}"##,
        )
        .unwrap();
        assert!(read_import(&file).is_err());
    }

    #[test]
    fn imported_names_are_cleaned_and_checked() {
        let root = import_scratch("names");
        let file = root.join("p.json");
        std::fs::write(&file, br#"{"name":"  A  ","look":{}}"#).unwrap();
        assert_eq!(read_import(&file).unwrap().name, "A");
        std::fs::write(&file, br#"{"name":"   ","look":{}}"#).unwrap();
        assert!(read_import(&file).is_err());
        std::fs::write(&file, b"{\"name\":\"a\\u0007b\",\"look\":{}}").unwrap();
        assert!(read_import(&file).is_err());
    }

    #[test]
    fn a_missing_file_is_not_called_invalid() {
        let root = import_scratch("absent");
        let error = read_import(&root.join("no.json")).unwrap_err().to_string();
        assert!(error.starts_with("open profile file: "), "{error}");
    }

    #[test]
    fn a_taken_name_gets_the_next_free_number() {
        assert_eq!(unused_name(&[], "A"), "A");
        assert_eq!(unused_name(&[named("A")], "A"), "A (2)");
        assert_eq!(
            unused_name(&[named("A"), named("A (2)"), named("A (3)")], "A"),
            "A (4)"
        );
    }

    #[test]
    fn a_numbered_name_stays_within_the_limit() {
        let long = "あ".repeat(80);
        let numbered = unused_name(&[named(&long)], &long);
        assert_eq!(numbered.chars().count(), 80);
        assert!(numbered.ends_with(" (2)"), "{numbered}");
    }

    #[test]
    fn surplus_colours_in_a_profile_are_dropped_when_it_is_applied() {
        let mut look = CaptionLook::default();
        look.palette.extend(CaptionLook::default().palette);
        assert_eq!(look.palette.len(), 16);
        assert_eq!(look.applied_to(&Settings::default()).palette.len(), 8);
    }

    #[test]
    fn the_ui_language_and_theme_are_not_part_of_a_look() {
        let settings = Settings {
            ui_language: "ja".to_owned(),
            ui_theme: crate::settings::UiTheme::Dark,
            ..Settings::default()
        };
        let look = serde_json::to_value(CaptionLook::from_settings(&settings)).unwrap();
        assert_eq!(look.get("ui_language"), None);
        assert_eq!(look.get("ui_theme"), None);
        let applied = CaptionLook::default().applied_to(&settings);
        assert_eq!(applied.ui_language, "ja");
        assert_eq!(applied.ui_theme, crate::settings::UiTheme::Dark);
    }

    #[test]
    fn the_subtitle_format_is_not_part_of_a_look() {
        let settings = Settings {
            subtitle_format: crate::settings::SubtitleFormat::Srt,
            ..Settings::default()
        };
        let look = serde_json::to_value(CaptionLook::from_settings(&settings)).unwrap();
        assert_eq!(look.get("subtitle_format"), None);
        assert_eq!(
            CaptionLook::default().applied_to(&settings).subtitle_format,
            crate::settings::SubtitleFormat::Srt
        );
    }

    #[test]
    fn a_profile_with_a_shadow_from_an_earlier_version_still_reads() {
        let root = import_scratch("old-shadow");
        let saved = root.join("profiles.json");
        std::fs::write(
            &saved,
            br#"{"profiles":[{"name":"A","look":{"font_ratio":0.05,"shadow":{"enabled":true,"strength":0.5}}}]}"#,
        )
        .unwrap();
        let rows = load(&saved).unwrap();
        assert_eq!(rows[0].look.font_ratio, 0.05);
        let file = root.join("exported.json");
        std::fs::write(
            &file,
            br#"{"name":"B","look":{"shadow":{"enabled":true,"strength":9.0}}}"#,
        )
        .unwrap();
        assert_eq!(read_import(&file).unwrap().name, "B");
        assert_eq!(
            serde_json::to_value(CaptionLook::default())
                .unwrap()
                .get("shadow"),
            None
        );
    }

    #[test]
    fn saved_and_imported_profiles_from_before_per_speaker_widths_keep_their_look() {
        let root = import_scratch("old-widths");
        let saved = root.join("profiles.json");
        std::fs::write(
            &saved,
            br##"{"profiles":[{"name":"A","look":{"outline_ratio":0.2,"rim_ratio":0.05,
                "palette":[{"fill":"#005A9C","outline":"#FFFFFF"}]}}]}"##,
        )
        .unwrap();
        let rows = load(&saved).unwrap();
        assert_eq!(rows[0].look.palette[0].outline_width, 0.2);
        assert_eq!(rows[0].look.palette[0].rim, Some("#000000".to_owned()));
        assert_eq!(rows[0].look.palette[0].rim_width, 0.05);
        let file = root.join("old.json");
        std::fs::write(
            &file,
            br##"{"name":"B","look":{"outline_ratio":0.2,
                "palette":[{"fill":"#005A9C","outline":"#FFFFFF"}]}}"##,
        )
        .unwrap();
        assert_eq!(
            read_import(&file).unwrap().look.palette[0].outline_width,
            0.2
        );
    }

    #[test]
    fn a_profile_file_without_a_look_still_fails_clearly() {
        let root = import_scratch("no-look");
        let file = root.join("p.json");
        std::fs::write(&file, br#"{"name":"A"}"#).unwrap();
        assert!(read_import(&file).is_err());
        assert!(parse_profile_file(r#"{"profiles":[{"name":"A"}]}"#).is_err());
    }
}
