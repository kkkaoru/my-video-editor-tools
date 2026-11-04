//! Turns timed words and speaker turns into positioned caption cues.
//!
//! Speaker rule from `replay.diarized_words`: a word gets a speaker only when one voice clearly
//! dominates its time span. Overlapping voices stay unassigned rather than guessed.

use crate::engines::VideoInfo;
use crate::face::FaceSample;
use crate::settings::{Anchor, Settings};
use serde::{Deserialize, Serialize};

/// Fraction of a word one speaker must cover, and the lead it needs over the next (diarized_words).
const DOMINANT_SHARE: f64 = 0.6;
const DOMINANT_MARGIN: f64 = 0.2;
/// Timing noise: a word just outside a turn belongs to the nearest speaker within this gap.
const SILENT_WORD_REACH_SECONDS: f64 = 1.0;
const CUE_BREAK_PAUSE_SECONDS: f64 = 0.7;
const MAX_CUE_SECONDS: f64 = 7.0;
const MIN_CUE_SECONDS: f64 = 0.5;
const MAX_LINES: usize = 2;
const LINE_HEIGHT: f64 = 1.25;
/// Width of Latin and digits relative to the em; CJK takes a full em.
const NARROW_GLYPH_EM: f64 = 0.55;
/// Space between face and caption: `HEAD_CAPTION_GAP_PX` 24 against the 52 px base font.
const FACE_GAP_EM: f64 = 0.46;
const VIDEO_MARGIN_RATIO: f64 = 0.05;
/// `HEAD_CAPTION_Y_DEADBAND_PX` 6 at a 1920 reference: small face drift must not jitter captions.
const DEADBAND_RATIO: f64 = 6.0 / 1920.0;
const SENTENCE_ENDS: [char; 6] = ['。', '！', '？', '!', '?', '．'];
/// Kinsoku rules shared with `replay.speaker_subtitles`.
const NO_LINE_START: &str = "、。！？!?.,;:…」』）)]｝ー〜～ぁぃぅぇぉっゃゅょァィゥェォッャュョ";
const NO_LINE_END: &str = "「『（([｛";
/// A last line this short ("かな?") gets rebalanced with the line above.
const WRAP_ORPHAN_MAXIMUM: usize = 4;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Word {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub speaker: Option<u32>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct SpeakerTurn {
    pub start: f64,
    pub end: f64,
    pub speaker: u32,
}

/// A caption ready for export; `x`/`y` are the block's centre in video pixels.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub speaker: Option<u32>,
    pub lines: Vec<String>,
    pub x: f64,
    pub y: f64,
}

/// Font and width for one video, derived from its resolution.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Metrics {
    pub font_px: f64,
    pub outline_px: f64,
    pub max_line_px: f64,
}

/// What a word's overlap with the turns says about its speaker.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Voice {
    Speaker(u32),
    Overlapped,
    Silent,
}

/// Font and caption width scale with the video, so vertical and 4K need no extra settings.
#[must_use]
pub fn metrics_for(info: &VideoInfo, settings: &Settings) -> Metrics {
    let short_edge = f64::from(info.width.min(info.height));
    let font_px = (short_edge * settings.font_ratio).round().max(1.0);
    let outline_px = (font_px * settings.outline_ratio).max(1.0);
    let max_line_px = (f64::from(info.width) * settings.width_ratio - 2.0 * outline_px).max(font_px);
    Metrics { font_px, outline_px, max_line_px }
}

fn overlap(word: &Word, turn: &SpeakerTurn) -> f64 {
    (word.end.min(turn.end) - word.start.max(turn.start)).max(0.0)
}

fn descending_share(left: &(u32, f64), right: &(u32, f64)) -> std::cmp::Ordering {
    right.1.total_cmp(&left.1)
}

fn voice_of(word: &Word, turns: &[SpeakerTurn]) -> Voice {
    let mut amounts: Vec<(u32, f64)> = Vec::new();
    for turn in turns {
        let share = overlap(word, turn);
        if share <= 0.0 {
            continue;
        }
        match amounts.iter_mut().find(|(speaker, _)| *speaker == turn.speaker) {
            Some((_, total)) => *total += share,
            None => amounts.push((turn.speaker, share)),
        }
    }
    amounts.sort_by(descending_share);
    let Some(&(top, top_share)) = amounts.first() else { return Voice::Silent };
    let duration = (word.end - word.start).max(f64::EPSILON);
    let runner_up = amounts.get(1).map_or(0.0, |entry| entry.1);
    if amounts.len() > 1 && top_share - runner_up < DOMINANT_MARGIN * duration {
        return Voice::Overlapped;
    }
    if amounts.len() > 1 && top_share < DOMINANT_SHARE * duration {
        return Voice::Overlapped;
    }
    Voice::Speaker(top)
}

fn distance_to_turn(word: &Word, turn: &SpeakerTurn) -> f64 {
    (turn.start - word.end).max(word.start - turn.end).max(0.0)
}

fn nearest_speaker(word: &Word, turns: &[SpeakerTurn]) -> Option<u32> {
    turns
        .iter()
        .map(|turn| (distance_to_turn(word, turn), turn.speaker))
        .filter(|(distance, _)| *distance <= SILENT_WORD_REACH_SECONDS)
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, speaker)| speaker)
}

/// Sets each word's speaker. Overlapped voices stay `None`; silent gaps are timing noise.
pub fn assign_speakers(words: &mut [Word], turns: &[SpeakerTurn]) {
    for word in words.iter_mut() {
        word.speaker = match voice_of(word, turns) {
            Voice::Speaker(speaker) => Some(speaker),
            Voice::Overlapped => None,
            Voice::Silent => nearest_speaker(word, turns),
        };
    }
}

fn glyph_width(c: char, font_px: f64) -> f64 {
    if c.is_ascii() {
        font_px * NARROW_GLYPH_EM
    } else {
        font_px
    }
}

fn measure(text: &str, font_px: f64) -> f64 {
    text.chars().map(|c| glyph_width(c, font_px)).sum()
}

/// Greedy wrap; breaks move to keep closing marks off line starts and opening marks off line ends.
fn wrap(text: &str, metrics: &Metrics) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut lines: Vec<Vec<char>> = Vec::new();
    let mut current: Vec<char> = Vec::new();
    let mut width = 0.0;
    for c in &chars {
        let glyph = glyph_width(*c, metrics.font_px);
        if width + glyph > metrics.max_line_px && !current.is_empty() {
            let mut carry = Vec::new();
            while current.len() > 1 && break_is_forbidden(&current, *c, &carry) {
                if let Some(moved) = current.pop() {
                    carry.insert(0, moved);
                }
            }
            lines.push(std::mem::take(&mut current));
            width = carry.iter().map(|moved| glyph_width(*moved, metrics.font_px)).sum();
            current = carry;
        }
        current.push(*c);
        width += glyph;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    rebalance_orphan(&mut lines, metrics);
    lines.into_iter().map(|line| line.into_iter().collect()).collect()
}

fn break_is_forbidden(current: &[char], next: char, carry: &[char]) -> bool {
    let first_of_next = carry.first().copied().unwrap_or(next);
    let last = current.last().copied().unwrap_or(' ');
    NO_LINE_START.contains(first_of_next) || NO_LINE_END.contains(last)
}

fn rebalance_orphan(lines: &mut [Vec<char>], metrics: &Metrics) {
    let Some((last, earlier)) = lines.split_last_mut() else { return };
    let Some(previous) = earlier.last_mut() else { return };
    if last.len() > WRAP_ORPHAN_MAXIMUM {
        return;
    }
    let total = previous.len() + last.len();
    while previous.len() > total.div_ceil(2) {
        let Some(moved) = previous.pop() else { return };
        last.insert(0, moved);
        let text: String = last.iter().collect();
        let next_start = last.first().copied().unwrap_or(' ');
        if measure(&text, metrics.font_px) > metrics.max_line_px || NO_LINE_START.contains(next_start) {
            last.remove(0);
            previous.push(moved);
            return;
        }
    }
}

fn normalized_text(words: &[Word]) -> String {
    words.iter().map(|word| word.text.as_str()).collect::<String>().trim().to_owned()
}

fn fits(words: &[Word], metrics: &Metrics) -> bool {
    wrap(&normalized_text(words), metrics).len() <= MAX_LINES
}

fn starts_new_cue(group: &[Word], next: &Word, metrics: &Metrics) -> bool {
    let Some(last) = group.last() else { return false };
    let first_start = group.first().map_or(next.start, |word| word.start);
    let is_punctuation_only = next.text.trim().chars().all(|c| !c.is_alphanumeric());
    if is_punctuation_only {
        return false;
    }
    let mut extended = group.to_vec();
    extended.push(next.clone());
    last.speaker != next.speaker
        || next.start - last.end > CUE_BREAK_PAUSE_SECONDS
        || next.end - first_start > MAX_CUE_SECONDS
        || last.text.trim_end().ends_with(SENTENCE_ENDS)
        || !fits(&extended, metrics)
}

/// Groups words into cues at speaker changes, pauses, sentence ends and the two-line limit.
fn group_words(words: &[Word], metrics: &Metrics) -> Vec<Vec<Word>> {
    let mut groups: Vec<Vec<Word>> = Vec::new();
    for word in words {
        match groups.last_mut() {
            Some(group) if !starts_new_cue(group, word, metrics) => group.push(word.clone()),
            _ => groups.push(vec![word.clone()]),
        }
    }
    groups
}

fn block_height(line_count: usize, metrics: &Metrics) -> f64 {
    line_count as f64 * metrics.font_px * LINE_HEIGHT + 2.0 * metrics.outline_px
}

fn block_width(lines: &[String], metrics: &Metrics) -> f64 {
    lines.iter().map(|line| measure(line, metrics.font_px)).fold(0.0, f64::max) + 2.0 * metrics.outline_px
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied()
}

fn distance_to_span(sample: &FaceSample, start: f64, end: f64) -> f64 {
    (start - sample.time).max(sample.time - end).max(0.0)
}

/// The face during a cue: the median over its span, else the nearest sample in time.
fn face_for_cue(faces: &[FaceSample], start: f64, end: f64) -> Option<FaceSample> {
    let during: Vec<&FaceSample> = faces.iter().filter(|face| (start..=end).contains(&face.time)).collect();
    if during.is_empty() {
        return faces.iter().min_by(|left, right| distance_to_span(left, start, end).total_cmp(&distance_to_span(right, start, end))).copied();
    }
    Some(FaceSample {
        time: start,
        top: median(during.iter().map(|face| face.top).collect())?,
        bottom: median(during.iter().map(|face| face.bottom).collect())?,
        center_x: median(during.iter().map(|face| face.center_x).collect())?,
    })
}

fn clamp_center(center: f64, block: f64, frame: f64) -> f64 {
    let half = block / 2.0;
    if block >= frame {
        return frame / 2.0;
    }
    center.clamp(half, frame - half)
}

fn anchored_center(anchor: Anchor, face: Option<&FaceSample>, height: f64, info: &VideoInfo, metrics: &Metrics) -> (f64, f64) {
    let (frame_w, frame_h) = (f64::from(info.width), f64::from(info.height));
    let margin = frame_h * VIDEO_MARGIN_RATIO;
    let bottom_of_frame = (frame_w / 2.0, frame_h - margin - height / 2.0);
    let gap = metrics.font_px * FACE_GAP_EM;
    match (anchor, face) {
        (Anchor::VideoTop, _) => (frame_w / 2.0, margin + height / 2.0),
        (Anchor::VideoBottom, _) | (Anchor::FaceAbove | Anchor::FaceBelow, None) => bottom_of_frame,
        (Anchor::FaceAbove, Some(face)) => (face.center_x, face.top - gap - height / 2.0),
        (Anchor::FaceBelow, Some(face)) => (face.center_x, face.bottom + gap + height / 2.0),
    }
}

/// Builds positioned cues. The returned flag reports a face anchor that found no face.
#[must_use]
pub fn build_cues(words: &[Word], faces: &[FaceSample], info: &VideoInfo, settings: &Settings) -> (Vec<Cue>, bool) {
    let metrics = metrics_for(info, settings);
    let placement = settings.placement;
    let (frame_w, frame_h) = (f64::from(info.width), f64::from(info.height));
    let deadband = f64::from(info.width.max(info.height)) * DEADBAND_RATIO;
    let wants_face = matches!(placement.anchor, Anchor::FaceAbove | Anchor::FaceBelow);
    let groups = group_words(words, &metrics);
    let mut cues: Vec<Cue> = Vec::with_capacity(groups.len());
    let mut faceless = false;
    for (index, group) in groups.iter().enumerate() {
        let (Some(first), Some(last)) = (group.first(), group.last()) else { continue };
        let text = normalized_text(group);
        if text.is_empty() {
            continue;
        }
        let lines = wrap(&text, &metrics);
        let next_start = groups.get(index + 1).and_then(|next| next.first()).map_or(f64::INFINITY, |word| word.start);
        let end = last.end.max(first.start + MIN_CUE_SECONDS).min(next_start.max(first.start + f64::EPSILON));
        let face = wants_face.then(|| face_for_cue(faces, first.start, last.end)).flatten();
        faceless |= wants_face && face.is_none();
        let height = block_height(lines.len(), &metrics);
        let (center_x, center_y) = anchored_center(placement.anchor, face.as_ref(), height, info, &metrics);
        let mut x = clamp_center(center_x + placement.offset_x_percent / 100.0 * frame_w, block_width(&lines, &metrics), frame_w);
        let mut y = clamp_center(center_y + placement.offset_y_percent / 100.0 * frame_h, height, frame_h);
        if let Some(previous) = cues.last() {
            if (previous.x - x).abs() <= deadband {
                x = previous.x;
            }
            if (previous.y - y).abs() <= deadband {
                y = previous.y;
            }
        }
        cues.push(Cue { start: first.start, end, speaker: first.speaker, lines, x, y });
    }
    (cues, faceless)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Placement;

    fn word(start: f64, end: f64, text: &str) -> Word {
        Word { start, end, text: text.to_owned(), speaker: None }
    }

    fn spoken(start: f64, end: f64, text: &str, speaker: u32) -> Word {
        Word { start, end, text: text.to_owned(), speaker: Some(speaker) }
    }

    const FULL_HD: VideoInfo = VideoInfo { width: 1920, height: 1080, duration: 60.0, fps_num: 30, fps_den: 1 };
    const VERTICAL: VideoInfo = VideoInfo { width: 1080, height: 1920, duration: 60.0, fps_num: 30, fps_den: 1 };
    const UHD: VideoInfo = VideoInfo { width: 3840, height: 2160, duration: 60.0, fps_num: 30, fps_den: 1 };

    fn at(anchor: Anchor, x: f64, y: f64) -> Settings {
        Settings { placement: Placement { anchor, offset_x_percent: x, offset_y_percent: y }, ..Settings::default() }
    }

    #[test]
    fn caption_size_follows_the_resolution() {
        let settings = Settings::default();
        assert_eq!(metrics_for(&FULL_HD, &settings).font_px, 52.0);
        assert_eq!(metrics_for(&UHD, &settings).font_px, 104.0);
        assert_eq!(metrics_for(&VERTICAL, &settings).font_px, 52.0);
        assert!(metrics_for(&FULL_HD, &settings).max_line_px > 1700.0);
        assert!(metrics_for(&VERTICAL, &settings).max_line_px < 961.0);
    }

    #[test]
    fn vertical_video_wraps_narrower_than_landscape() {
        let text = "あ".repeat(30);
        let settings = Settings::default();
        assert_eq!(wrap(&text, &metrics_for(&FULL_HD, &settings)).len(), 1);
        assert_eq!(wrap(&text, &metrics_for(&VERTICAL, &settings)).len(), 2);
    }

    #[test]
    fn dominant_voice_wins() {
        let mut words = [word(0.0, 1.0, "は")];
        assign_speakers(&mut words, &[SpeakerTurn { start: 0.0, end: 5.0, speaker: 2 }]);
        assert_eq!(words[0].speaker, Some(2));
    }

    #[test]
    fn overlapping_voices_are_not_guessed() {
        let mut words = [word(0.0, 1.0, "は")];
        assign_speakers(&mut words, &[
            SpeakerTurn { start: 0.0, end: 0.5, speaker: 1 },
            SpeakerTurn { start: 0.5, end: 1.0, speaker: 2 },
        ]);
        assert_eq!(words[0].speaker, None);
    }

    #[test]
    fn a_word_in_a_small_silent_gap_takes_the_nearest_speaker() {
        let mut words = [word(5.2, 5.3, "ね")];
        assign_speakers(&mut words, &[SpeakerTurn { start: 0.0, end: 5.0, speaker: 3 }]);
        assert_eq!(words[0].speaker, Some(3));
    }

    #[test]
    fn a_word_far_from_any_speech_stays_unassigned() {
        let mut words = [word(9.0, 9.1, "ね")];
        assign_speakers(&mut words, &[SpeakerTurn { start: 0.0, end: 5.0, speaker: 3 }]);
        assert_eq!(words[0].speaker, None);
    }

    #[test]
    fn closing_marks_never_start_a_line() {
        let metrics = Metrics { font_px: 10.0, outline_px: 1.0, max_line_px: 50.0 };
        assert_eq!(wrap("あいうえお。かきくけこさ", &metrics), vec!["あいうえ", "お。かき", "くけこさ"]);
    }

    #[test]
    fn opening_marks_never_end_a_line() {
        let metrics = Metrics { font_px: 10.0, outline_px: 1.0, max_line_px: 50.0 };
        assert_eq!(wrap("あいうえ「おかきくけこさしすせ", &metrics), vec!["あいうえ", "「おかきく", "けこさ", "しすせ"]);
    }

    #[test]
    fn a_short_last_line_is_rebalanced() {
        let metrics = Metrics { font_px: 10.0, outline_px: 1.0, max_line_px: 100.0 };
        assert_eq!(wrap("あいうえおかきくけこさし", &metrics), vec!["あいうえおか", "きくけこさし"]);
    }

    #[test]
    fn speaker_changes_start_a_new_cue() {
        let words = [spoken(0.0, 1.0, "はい", 1), spoken(1.1, 2.0, "いいえ", 2)];
        let (cues, _) = build_cues(&words, &[], &FULL_HD, &Settings::default());
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].speaker, Some(1));
        assert_eq!(cues[1].speaker, Some(2));
    }

    #[test]
    fn pauses_and_sentence_ends_split_cues() {
        let words = [spoken(0.0, 1.0, "はい", 1), spoken(2.0, 3.0, "そう。", 1), spoken(3.1, 4.0, "次", 1)];
        let (cues, _) = build_cues(&words, &[], &FULL_HD, &Settings::default());
        assert_eq!(cues.iter().map(|cue| cue.lines.join("")).collect::<Vec<_>>(), vec!["はい", "そう。", "次"]);
    }

    #[test]
    fn punctuation_stays_with_its_sentence() {
        let words = [spoken(0.0, 1.0, "はい", 1), spoken(5.0, 5.1, "。", 2)];
        let (cues, _) = build_cues(&words, &[], &FULL_HD, &Settings::default());
        assert_eq!(cues[0].lines, vec!["はい。"]);
    }

    #[test]
    fn short_cues_last_long_enough_but_never_overlap_the_next() {
        let words = [spoken(0.0, 0.1, "あ", 1), spoken(0.3, 2.0, "い", 2), spoken(2.0, 2.1, "う", 1)];
        let (cues, _) = build_cues(&words, &[], &FULL_HD, &Settings::default());
        assert_eq!(cues[0].end, 0.3);
        assert_eq!(cues[1].end, 2.0);
        assert_eq!(cues[2].end, 2.5);
    }

    #[test]
    fn video_bottom_and_top_sit_inside_the_margin() {
        let words = [spoken(0.0, 1.0, "はい", 1)];
        let (bottom, _) = build_cues(&words, &[], &FULL_HD, &at(Anchor::VideoBottom, 0.0, 0.0));
        let (top, _) = build_cues(&words, &[], &FULL_HD, &at(Anchor::VideoTop, 0.0, 0.0));
        assert_eq!((bottom[0].x, bottom[0].y.round()), (960.0, 988.0));
        assert_eq!((top[0].x, top[0].y.round()), (960.0, 92.0));
    }

    #[test]
    fn offsets_move_captions_in_percent_of_the_frame() {
        let words = [spoken(0.0, 1.0, "はい", 1)];
        let (cues, _) = build_cues(&words, &[], &FULL_HD, &at(Anchor::VideoBottom, 10.0, -10.0));
        assert_eq!((cues[0].x, cues[0].y.round()), (1152.0, 880.0));
    }

    #[test]
    fn offsets_cannot_push_captions_off_the_frame() {
        let words = [spoken(0.0, 1.0, "はい", 1)];
        let (cues, _) = build_cues(&words, &[], &FULL_HD, &at(Anchor::VideoBottom, 50.0, 50.0));
        assert_eq!(cues[0].y.round(), 1042.0);
        assert!(cues[0].x < 1920.0);
    }

    #[test]
    fn captions_can_sit_above_or_below_a_face() {
        let words = [spoken(0.0, 1.0, "はい", 1)];
        let faces = [FaceSample { time: 0.5, top: 400.0, bottom: 600.0, center_x: 700.0 }];
        let (above, _) = build_cues(&words, &faces, &FULL_HD, &at(Anchor::FaceAbove, 0.0, 0.0));
        let (below, _) = build_cues(&words, &faces, &FULL_HD, &at(Anchor::FaceBelow, 0.0, 2.0));
        assert_eq!((above[0].x, above[0].y.round()), (700.0, 338.0));
        assert_eq!((below[0].x, below[0].y.round()), (700.0, 684.0));
    }

    #[test]
    fn a_missing_face_falls_back_to_the_bottom_and_reports_it() {
        let words = [spoken(0.0, 1.0, "はい", 1)];
        let (cues, faceless) = build_cues(&words, &[], &FULL_HD, &at(Anchor::FaceAbove, 0.0, 0.0));
        assert!(faceless);
        assert_eq!(cues[0].y.round(), 988.0);
    }

    #[test]
    fn the_face_nearest_in_time_covers_gaps() {
        let faces = [
            FaceSample { time: 0.0, top: 1.0, bottom: 2.0, center_x: 3.0 },
            FaceSample { time: 10.0, top: 4.0, bottom: 5.0, center_x: 6.0 },
        ];
        assert_eq!(face_for_cue(&faces, 8.0, 9.0).unwrap().top, 4.0);
    }

    #[test]
    fn small_face_drift_keeps_the_caption_still() {
        let words = [spoken(0.0, 1.0, "はい。", 1), spoken(2.0, 3.0, "そう。", 1)];
        let faces = [
            FaceSample { time: 0.5, top: 400.0, bottom: 600.0, center_x: 700.0 },
            FaceSample { time: 2.5, top: 404.0, bottom: 604.0, center_x: 702.0 },
        ];
        let (cues, _) = build_cues(&words, &faces, &FULL_HD, &at(Anchor::FaceAbove, 0.0, 0.0));
        assert_eq!(cues[0].y, cues[1].y);
        assert_eq!(cues[0].x, cues[1].x);
    }
}
