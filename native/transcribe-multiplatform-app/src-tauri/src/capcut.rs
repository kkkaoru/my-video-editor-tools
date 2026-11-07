//! A CapCut project of one video and its captions, the only way CapCut keeps where each caption sits
//! and its colour: its subtitle import drops both. The format is CapCut's own and unpublished, so the
//! template is what CapCut itself wrote, without the identifiers of the machine it was written on.

use crate::captions::Cue;
use crate::error::{io_context, AppError, AppResult};
use crate::export::Transcript;
use crate::settings::Settings;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const TEMPLATE: &str = include_str!("capcut-template.json");
const PROJECTS_UNDER_APP_DATA: [&str; 4] =
    ["CapCut", "User Data", "Projects", "com.lveditor.draft"];
const MOVIES_FOLDER: &str = "Movies";
/// Where to write instead of CapCut's folder, like the other `TRANSCRIBE_` overrides.
const PROJECTS_OVERRIDE: &str = "TRANSCRIBE_CAPCUT_PROJECTS";
const DRAFT_FILE: &str = "draft_info.json";
const META_FILE: &str = "draft_meta_info.json";
const PROJECT_FILE: &str = "project.json";
const LAYOUT_FILE: &str = "timeline_layout.json";
const TIMELINES_FOLDER: &str = "Timelines";
const TIMELINE_NAME: &str = "Timeline 01";
const WRITING_SUFFIX: &str = ".writing";
const MICROSECONDS_PER_SECOND: f64 = 1_000_000.0;
/// Measured in CapCut: glyph height over frame height, per unit of its font size.
const FONT_SIZE_PER_FRAME_HEIGHT: f64 = 0.0102;
const OUTLINE_WIDTH: f64 = 0.08;
/// CapCut draws an outline only with this bit set on the text.
const OUTLINE_FLAG: u64 = 8;
const NO_FLAGS: u64 = 7;
const FIRST_RENDER_INDEX: usize = 14_000;
const UNIT_RANGE_CENTER: f64 = 1.0;
const UNIT_RANGE_SPAN: f64 = 2.0;
const COLOR_DIGITS: usize = 6;
const COLOR_MAX: f64 = 255.0;
const HEX_RADIX: u32 = 16;

struct Ids {
    stamp: u128,
    next: u32,
}

impl Ids {
    /// CapCut only needs them unique within the project.
    fn next(&mut self) -> String {
        self.next += 1;
        let low = u64::try_from(self.stamp & 0xFFFF_FFFF_FFFF).unwrap_or(0);
        format!("{:08X}-0000-4000-8000-{low:012X}", self.next)
    }
}

/// CapCut's folder of projects under the given user folders, which it scans when it shows its home page.
#[must_use]
pub fn projects_folder(home: &Path, local_app_data: Option<&Path>) -> PathBuf {
    let root = local_app_data.map_or_else(|| home.join(MOVIES_FOLDER), Path::to_path_buf);
    PROJECTS_UNDER_APP_DATA
        .iter()
        .fold(root, |folder, part| folder.join(part))
}

fn existing_folder(folder: PathBuf) -> AppResult<PathBuf> {
    if folder.is_dir() {
        Ok(folder)
    } else {
        Err(AppError::Invalid(format!(
            "CapCut's project folder was not found: {}",
            folder.display()
        )))
    }
}

fn user_projects_folder() -> AppResult<PathBuf> {
    if let Some(folder) = std::env::var_os(PROJECTS_OVERRIDE) {
        return existing_folder(PathBuf::from(folder));
    }
    let local = cfg!(windows)
        .then(|| std::env::var_os("LOCALAPPDATA"))
        .flatten();
    let home = std::env::var_os("HOME").unwrap_or_default();
    existing_folder(projects_folder(
        Path::new(&home),
        local.as_deref().map(Path::new),
    ))
}

fn rgb(hex: &str) -> [f64; 3] {
    let digits = hex.trim_start_matches('#');
    let channel = |at: usize| {
        digits
            .get(at..at + 2)
            .and_then(|part| u8::from_str_radix(part, HEX_RADIX).ok())
            .map_or(0.0, |value| f64::from(value) / COLOR_MAX)
    };
    if digits.len() == COLOR_DIGITS {
        [channel(0), channel(2), channel(4)]
    } else {
        [0.0, 0.0, 0.0]
    }
}

/// A cue's centre in pixels as CapCut's position: the middle of the frame is 0, up and right are positive.
fn position(x: f64, y: f64, width: f64, height: f64) -> (f64, f64) {
    (
        UNIT_RANGE_SPAN * x / width - UNIT_RANGE_CENTER,
        UNIT_RANGE_CENTER - UNIT_RANGE_SPAN * y / height,
    )
}

fn microseconds(seconds: f64) -> i64 {
    (seconds * MICROSECONDS_PER_SECOND).round() as i64
}

/// A track holds one caption at a time, so voices at once take a track each.
fn lanes_of(spans: &[(i64, i64)]) -> Vec<usize> {
    let mut ends: Vec<i64> = Vec::new();
    spans
        .iter()
        .map(|&(start, end)| {
            let free = ends.iter().position(|&taken| taken <= start);
            let lane = free.unwrap_or(ends.len());
            if free.is_none() {
                ends.push(end);
            } else {
                ends[lane] = end;
            }
            lane
        })
        .collect()
}

fn text_content(line: &str, fill: &str, outline: Option<&str>, size: f64, font: &Value) -> String {
    let solid = |color: &str| {
        json!({
            "alpha": 1.0,
            "content": { "render_type": "solid", "solid": { "alpha": 1.0, "color": rgb(color) } },
        })
    };
    let mut style = json!({
        "fill": solid(fill),
        "font": font,
        "range": [0, line.encode_utf16().count()],
        "size": size,
    });
    if let Some(color) = outline {
        let mut stroke = solid(color);
        stroke["width"] = json!(OUTLINE_WIDTH);
        style["strokes"] = json!([stroke]);
    }
    json!({ "styles": [style], "text": line }).to_string()
}

struct Parts {
    text: Value,
    animation: Value,
    segment: Value,
    track: Value,
}

fn cue_text(cue: &Cue, parts: &Parts, settings: &Settings, base_size: f64) -> Value {
    let color = settings.color_for(cue.speaker);
    let outline = (color.outline_width > 0.0).then_some(color.outline.as_str());
    let size = base_size * cue.scale;
    let line = cue.lines.join("\n");
    let template: Value =
        serde_json::from_str(parts.text["content"].as_str().unwrap_or("{}")).unwrap_or(Value::Null);
    let font = &template["styles"][0]["font"];
    let mut text = parts.text.clone();
    text["content"] = json!(text_content(&line, &color.fill, outline, size, font));
    text["text_color"] = json!(color.fill);
    text["font_size"] = json!(size);
    text["border_color"] = json!(outline.unwrap_or(""));
    text["check_flag"] = json!(if outline.is_some() {
        NO_FLAGS | OUTLINE_FLAG
    } else {
        NO_FLAGS
    });
    text
}

/// The draft as CapCut stores it, and the id of its one timeline.
fn draft_of(
    transcript: &Transcript,
    video: &Path,
    settings: &Settings,
    ids: &mut Ids,
) -> AppResult<(Value, String)> {
    let parse_error = |error: serde_json::Error| AppError::Invalid(error.to_string());
    let template: Value = serde_json::from_str(TEMPLATE).map_err(parse_error)?;
    let mut draft = template["draft"].clone();
    let parts = Parts {
        text: draft["materials"]["texts"][0].clone(),
        animation: draft["materials"]["material_animations"][0].clone(),
        segment: draft["tracks"][1]["segments"][0].clone(),
        track: draft["tracks"][1].clone(),
    };
    let info = &transcript.info;
    let (width, height) = (f64::from(info.width), f64::from(info.height));
    let base_size = transcript.metrics.font_px / height / FONT_SIZE_PER_FRAME_HEIGHT;
    let spans: Vec<(i64, i64)> = transcript
        .cues
        .iter()
        .map(|cue| (microseconds(cue.start), microseconds(cue.end)))
        .collect();
    let lanes = lanes_of(&spans);
    let lane_count = lanes.iter().max().map_or(0, |last| last + 1);
    let mut tracks: Vec<Value> = (0..lane_count)
        .map(|_| {
            let mut track = parts.track.clone();
            track["id"] = json!(ids.next());
            track["segments"] = json!([]);
            track
        })
        .collect();
    let mut texts = Vec::with_capacity(spans.len());
    let mut animations = Vec::with_capacity(spans.len());
    for ((cue, &(start, end)), &lane) in transcript.cues.iter().zip(&spans).zip(&lanes) {
        let mut text = cue_text(cue, &parts, settings, base_size);
        text["id"] = json!(ids.next());
        let mut animation = parts.animation.clone();
        animation["id"] = json!(ids.next());
        let (x, y) = position(cue.x, cue.y, width, height);
        let mut segment = parts.segment.clone();
        segment["id"] = json!(ids.next());
        segment["material_id"] = text["id"].clone();
        segment["extra_material_refs"] = json!([animation["id"]]);
        segment["target_timerange"] = json!({ "start": start, "duration": end - start });
        segment["render_index"] = json!(FIRST_RENDER_INDEX + lane);
        segment["track_render_index"] = json!(lane + 1);
        segment["clip"]["transform"] = json!({ "x": x, "y": y });
        if let Some(list) = tracks[lane]["segments"].as_array_mut() {
            list.push(segment);
        }
        texts.push(text);
        animations.push(animation);
    }
    let duration = microseconds(info.duration);
    let timeline = ids.next();
    draft["id"] = json!(timeline);
    draft["duration"] = json!(duration);
    draft["fps"] = json!(f64::from(info.fps_num) / f64::from(info.fps_den.max(1)));
    draft["canvas_config"]["width"] = json!(info.width);
    draft["canvas_config"]["height"] = json!(info.height);
    let material = &mut draft["materials"]["videos"][0];
    material["path"] = json!(video.to_string_lossy());
    material["material_name"] = json!(video.file_name().map(|name| name.to_string_lossy()));
    material["duration"] = json!(duration);
    material["width"] = json!(info.width);
    material["height"] = json!(info.height);
    let whole = json!({ "start": 0, "duration": duration });
    draft["tracks"][0]["segments"][0]["target_timerange"] = whole.clone();
    draft["tracks"][0]["segments"][0]["source_timerange"] = whole;
    draft["materials"]["texts"] = json!(texts);
    draft["materials"]["material_animations"] = json!(animations);
    let video_track = draft["tracks"][0].clone();
    draft["tracks"] = json!(std::iter::once(video_track)
        .chain(tracks)
        .collect::<Vec<Value>>());
    Ok((draft, timeline))
}

fn free_name(projects: &Path, stem: &str) -> String {
    let taken = |name: &str| projects.join(name).exists();
    if !taken(stem) {
        return stem.to_owned();
    }
    (2_u32..)
        .map(|number| format!("{stem}-{number}"))
        .find(|name| !taken(name))
        .unwrap_or_else(|| stem.to_owned())
}

fn write_json(path: &Path, value: &Value) -> AppResult<()> {
    let text =
        serde_json::to_string(value).map_err(|error| AppError::Invalid(error.to_string()))?;
    std::fs::write(path, text).map_err(io_context("write the CapCut project"))
}

/// Writes the project under `projects` beside any others, never over one, and returns its folder.
/// It appears whole: CapCut lists a folder as soon as it sees one.
pub fn write_project_in(
    projects: &Path,
    video: &Path,
    transcript: &Transcript,
    settings: &Settings,
) -> AppResult<PathBuf> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_micros());
    let mut ids = Ids {
        stamp: now,
        next: 0,
    };
    let (draft, timeline) = draft_of(transcript, video, settings, &mut ids)?;
    let stem = video.file_stem().map_or_else(
        || "captions".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let name = free_name(projects, &stem);
    let folder = projects.join(&name);
    let writing = projects
        .parent()
        .unwrap_or(projects)
        .join(format!("{name}{WRITING_SUFFIX}"));
    let timeline_folder = writing.join(TIMELINES_FOLDER).join(&timeline);
    std::fs::create_dir_all(&timeline_folder).map_err(io_context("write the CapCut project"))?;
    write_json(&writing.join(DRAFT_FILE), &draft)?;
    write_json(&timeline_folder.join(DRAFT_FILE), &draft)?;
    write_json(
        &writing.join(TIMELINES_FOLDER).join(PROJECT_FILE),
        &json!({
            "config": { "color_space": -1, "hdr_vivid": false, "mixed_track_mode_on": false,
                        "render_index_track_mode_on": false, "use_float_render": false },
            "create_time": now,
            "id": ids.next(),
            "main_timeline_id": timeline,
            "timelines": [{ "create_time": now, "id": timeline, "is_marked_delete": false,
                            "name": TIMELINE_NAME, "update_time": now }],
            "update_time": now,
            "version": 0,
        }),
    )?;
    write_json(
        &writing.join(LAYOUT_FILE),
        &json!({
            "dockItems": [{ "dockIndex": 0, "ratio": 1, "timelineIds": [timeline],
                            "timelineNames": [TIMELINE_NAME] }],
            "layoutOrientation": 1,
        }),
    )?;
    let template: Value =
        serde_json::from_str(TEMPLATE).map_err(|error| AppError::Invalid(error.to_string()))?;
    let mut meta = template["meta"].clone();
    meta["draft_id"] = json!(ids.next());
    meta["draft_name"] = json!(name);
    meta["draft_fold_path"] = json!(folder.to_string_lossy());
    meta["draft_root_path"] = json!(projects.to_string_lossy());
    meta["tm_duration"] = draft["duration"].clone();
    meta["tm_draft_create"] = json!(now);
    meta["tm_draft_modified"] = json!(now);
    write_json(&writing.join(META_FILE), &meta)?;
    std::fs::rename(&writing, &folder).map_err(io_context("write the CapCut project"))?;
    Ok(folder)
}

/// [`write_project_in`] in CapCut's own folder of projects on this computer.
pub fn write_project(
    video: &Path,
    transcript: &Transcript,
    settings: &Settings,
) -> AppResult<PathBuf> {
    write_project_in(&user_projects_folder()?, video, transcript, settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transcript() -> Transcript {
        serde_json::from_value(json!({
            "format_version": 1,
            "source": "/v/clip.mp4",
            "info": { "width": 1280, "height": 720, "duration": 10.0, "fps_num": 30, "fps_den": 1, "audio": null },
            "metrics": { "font_px": 36.72, "outline_px": 1.0, "max_line_px": 1000.0, "rendered_scale": 1.0 },
            "speaker_count": 2,
            "words": [],
            "faces": [],
            "cues": [
                { "start": 1.0, "end": 3.0, "speaker": 1, "lines": ["a", "b"], "x": 640.0, "y": 360.0 },
                { "start": 2.0, "end": 4.0, "speaker": 2, "lines": ["c"], "x": 0.0, "y": 0.0 },
            ],
        }))
        .unwrap()
    }

    #[test]
    fn a_project_holds_the_video_and_each_caption_in_its_place_and_track() {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target-test/capcut-project/projects");
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
        std::fs::create_dir_all(root.join("clip")).unwrap();
        let folder = write_project_in(
            &root,
            Path::new("/v/clip.mp4"),
            &transcript(),
            &Settings::default(),
        )
        .unwrap();
        assert_eq!(folder, root.join("clip-2"));
        let draft: Value =
            serde_json::from_slice(&std::fs::read(folder.join(DRAFT_FILE)).unwrap()).unwrap();
        assert_eq!(draft["duration"], 10_000_000);
        assert_eq!(draft["canvas_config"]["width"], 1280);
        assert_eq!(draft["materials"]["videos"][0]["path"], "/v/clip.mp4");
        assert_eq!(draft["materials"]["videos"][0]["material_name"], "clip.mp4");
        assert_eq!(draft["tracks"].as_array().unwrap().len(), 3);
        assert_eq!(draft["tracks"][0]["type"], "video");
        assert_eq!(
            draft["tracks"][0]["segments"][0]["target_timerange"]["duration"],
            10_000_000
        );
        let first = &draft["tracks"][1]["segments"][0];
        assert_eq!(
            first["target_timerange"],
            json!({ "start": 1_000_000, "duration": 2_000_000 })
        );
        assert_eq!(first["clip"]["transform"], json!({ "x": 0.0, "y": 0.0 }));
        assert_eq!(
            draft["tracks"][2]["segments"][0]["clip"]["transform"],
            json!({ "x": -1.0, "y": 1.0 })
        );
        let text = &draft["materials"]["texts"][0];
        assert_eq!(first["material_id"], text["id"]);
        assert!(text["content"].as_str().unwrap().contains("a\\nb"));
        assert_eq!(draft["materials"]["texts"].as_array().unwrap().len(), 2);
        let meta: Value =
            serde_json::from_slice(&std::fs::read(folder.join(META_FILE)).unwrap()).unwrap();
        assert_eq!(meta["draft_name"], "clip-2");
        let timeline = draft["id"].as_str().unwrap();
        assert!(folder
            .join(TIMELINES_FOLDER)
            .join(timeline)
            .join(DRAFT_FILE)
            .is_file());
        assert!(folder.join(TIMELINES_FOLDER).join(PROJECT_FILE).is_file());
        assert!(folder.join(LAYOUT_FILE).is_file());
    }

    #[test]
    fn the_template_holds_no_trace_of_the_machine_it_came_from() {
        let template: Value = serde_json::from_str(TEMPLATE).unwrap();
        assert_eq!(template["draft"]["platform"]["device_id"], "");
        assert_eq!(template["draft"]["platform"]["mac_address"], "");
        assert_eq!(template["draft"]["platform"]["hard_disk_id"], "");
        assert_eq!(template["draft"]["last_modified_platform"]["device_id"], "");
        assert_eq!(
            template["draft"]["last_modified_platform"]["mac_address"],
            ""
        );
        assert_eq!(
            template["draft"]["last_modified_platform"]["hard_disk_id"],
            ""
        );
        assert_eq!(template["draft"]["materials"]["videos"][0]["path"], "");
        assert!(!TEMPLATE.contains("/Users/"));
    }

    #[test]
    fn projects_sit_under_movies_on_a_mac_and_app_data_on_windows() {
        assert_eq!(
            projects_folder(Path::new("/u"), None),
            PathBuf::from("/u/Movies/CapCut/User Data/Projects/com.lveditor.draft")
        );
        assert_eq!(
            projects_folder(Path::new("/u"), Some(Path::new("/l"))),
            PathBuf::from("/l/CapCut/User Data/Projects/com.lveditor.draft")
        );
    }

    #[test]
    fn the_override_says_where_a_project_goes() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target-test/capcut-override");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::env::set_var(PROJECTS_OVERRIDE, &root);
        let folder = write_project(
            Path::new("/v/clip.mp4"),
            &transcript(),
            &Settings::default(),
        )
        .unwrap();
        std::env::remove_var(PROJECTS_OVERRIDE);
        assert_eq!(folder, root.join("clip"));
    }

    #[test]
    fn a_missing_project_folder_is_an_error_that_names_it() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target-test/capcut-missing");
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(
            existing_folder(root.join("none")).unwrap_err().to_string(),
            format!(
                "CapCut's project folder was not found: {}",
                root.join("none").display()
            )
        );
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(existing_folder(root.clone()).unwrap(), root);
    }

    #[test]
    fn a_caption_without_an_outline_has_none_and_no_outline_flag() {
        let mut settings = Settings::default();
        settings.palette[0].outline_width = 0.0;
        let template: Value = serde_json::from_str(TEMPLATE).unwrap();
        let parts = Parts {
            text: template["draft"]["materials"]["texts"][0].clone(),
            animation: Value::Null,
            segment: Value::Null,
            track: Value::Null,
        };
        let text = cue_text(&transcript().cues[0], &parts, &settings, 5.0);
        assert_eq!(text["check_flag"], 7);
        assert_eq!(text["border_color"], "");
    }

    #[test]
    fn a_video_without_a_name_makes_a_project_called_captions() {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target-test/capcut-unnamed/projects");
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
        std::fs::create_dir_all(&root).unwrap();
        let folder =
            write_project_in(&root, Path::new("/"), &transcript(), &Settings::default()).unwrap();
        assert_eq!(folder, root.join("captions"));
    }

    #[test]
    fn colors_become_fractions_and_a_bad_one_black() {
        assert_eq!(rgb("#FF0033"), [1.0, 0.0, 0.2]);
        assert_eq!(rgb("nope"), [0.0, 0.0, 0.0]);
        assert_eq!(rgb("#GG0033"), [0.0, 0.0, 0.2]);
    }

    #[test]
    fn the_middle_of_the_frame_is_zero_and_up_is_positive() {
        assert_eq!(position(640.0, 360.0, 1280.0, 720.0), (0.0, 0.0));
        assert_eq!(position(1280.0, 0.0, 1280.0, 720.0), (1.0, 1.0));
        assert_eq!(position(0.0, 720.0, 1280.0, 720.0), (-1.0, -1.0));
    }

    #[test]
    fn captions_at_once_take_a_track_each_and_share_when_apart() {
        assert_eq!(lanes_of(&[]), Vec::<usize>::new());
        assert_eq!(
            lanes_of(&[(0, 10), (5, 15), (10, 20), (16, 30)]),
            vec![0, 1, 0, 1]
        );
    }

    #[test]
    fn a_taken_name_moves_to_the_next_number() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target-test/capcut-free-name");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("clip")).unwrap();
        assert_eq!(free_name(&root, "other"), "other");
        assert_eq!(free_name(&root, "clip"), "clip-2");
    }

    #[test]
    fn text_counts_in_the_units_capcut_counts_and_outlines_only_when_asked() {
        let font = json!({ "path": "f" });
        let plain: Value =
            serde_json::from_str(&text_content("a😀", "#FFFFFF", None, 5.0, &font)).unwrap();
        assert_eq!(plain["text"], "a😀");
        assert_eq!(plain["styles"][0]["range"], json!([0, 3]));
        assert_eq!(plain["styles"][0]["strokes"], Value::Null);
        assert_eq!(plain["styles"][0]["font"], json!({ "path": "f" }));
        let edged: Value =
            serde_json::from_str(&text_content("a", "#FFFFFF", Some("#000000"), 5.0, &font))
                .unwrap();
        assert_eq!(edged["styles"][0]["strokes"][0]["width"], 0.08);
    }
}
