//! Turns timed words and speaker turns into positioned caption cues.
//!
//! Every word gets a speaker, so no caption is left without one's colours: the voice covering it
//! longest, else the nearest in time.
//!
//! Paging is the repo's static whole-phrase display (`caption_phrase_ass.phrase_cues`) over the
//! morphology of `replay.caption_linguistics`, with the word filters of `caption_timing` and the
//! hold of `caption_display_hold`. Its growing reveal and carried words need a renderer that shows
//! words one by one, so they have no place in a cue shown whole from its start.

use crate::engines::VideoInfo;
use crate::error::AppResult;
use crate::face::FaceSample;
use crate::fonts::GlyphWidths;
use crate::morphology::{
    grapheme_ends, is_japanese, is_kana, is_katakana_run, is_latin_or_number, Boundaries,
    BreakClass, PhraseEnd, Segmenter, DEPENDENT_KANA,
};
use crate::settings::{Anchor, Settings, SubtitleFormat};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

const FIRST_SPEAKER: u32 = 1;
/// A speaker talking for less than this, right beside other speech, is not told apart inside a caption.
const MIN_TURN_SECONDS: f64 = 1.0;
/// Two speakers talk at once when their words coincide for this long; less is timing error (0.22 s).
const SIMULTANEOUS_SECONDS: f64 = 0.25;
/// A turn takes whoever speaks most this far around it. Measured against delivered captions, this halves
/// the changes of speaker and keeps the share of the right colour; 8 s starts to lose it.
const STEADY_WINDOW_SECONDS: f64 = 4.0;
/// Turns at once with another speaker's for this long are two voices, not a wavering number.
const TRULY_TOGETHER_SECONDS: f64 = 1.0;
/// Enough of one voice around a change of speaker to read the phrase it falls in.
const PHRASE_CONTEXT_WORDS: usize = 24;
/// `caption_display_hold.LINGER`: a pause this long ends a page, and nothing is shown over it.
const CUE_BREAK_PAUSE_SECONDS: f64 = 0.5;
/// Nobody pauses this long inside one morpheme: a longer gap there is a real break, a shorter one
/// a timing error.
const SPLIT_WORD_REACH_SECONDS: f64 = 2.0;
/// A longer hole than this between a morpheme's parts is more likely a misread than a stretched word.
const STRETCHED_WORD_REACH_SECONDS: f64 = 3.0;
/// How long one character takes at most when a morpheme's characters have to be closed up.
const LONGEST_SPOKEN_CHARACTER_SECONDS: f64 = 0.25;
/// A particle after a longer pause is new speech: the caption before it does not wait.
const DEPENDENT_WORD_REACH_SECONDS: f64 = 1.0;
/// Characters on each side of a gap that decide whether a word spans it.
const JUNCTION_CONTEXT_CHARACTERS: [usize; 3] = [4, 8, 16];
/// `caption_display_hold`: Netflix minimum and maximum event duration, and its reading rates.
const MAX_CUE_SECONDS: f64 = 7.0;
/// Further than this from the words' own time, a voice's start belongs to some other words.
const VOICE_ONSET_REACH_SECONDS: f64 = 0.6;
const MIN_DISPLAY: f64 = 5.0 / 6.0;
/// `caption_phrase_ass.HOLD_SECONDS`: a caption leaves soon after its speech unless it needs reading.
const LINGER: f64 = 0.12;
const MIN_GAP_FRAMES: f64 = 2.0;
const READING_CPS: f64 = 4.0;
const FAST_READING_CPS: f64 = 12.0;
/// `caption_phrase_ass`: page length to aim for and a real gap.
const PAGE_TARGET_SECONDS: f64 = 3.0;
const PHRASE_GAP_SECONDS: f64 = 0.25;
/// A hard break needs room for `MIN_DISPLAY` plus the two-frame gap at the slowest common rates.
const SHORTEST_GROUP_SECONDS: f64 = 0.92;
/// Zero-length recognized spans still divide safely.
const SHORTEST_SPAN_SECONDS: f64 = 1e-9;
const MAX_LINES: usize = 2;
/// `caption_layout_file`: one line's native measurement, at the style-20 calibration.
const EM_LINE_HEIGHT: f64 = 150.0 / 82.955_801_104_972_38;
/// `caption_layout_file` defaults, in pixels of a frame whose short edge is `REFERENCE_SHORT_EDGE`.
const REFERENCE_SHORT_EDGE: f64 = 1080.0;
const PERCENT: f64 = 100.0;
const MARGIN_PX: f64 = 120.0;
const FACE_GAP_PX: f64 = 60.0;
const MAX_DRIFT_PX: f64 = 150.0;
const PADDING_PX: f64 = 24.0;
/// `caption_style_defaults.HEAD_EXTENSION_FACE_HEIGHT_RATIO`, used as in `head_reference`.
const HEAD_EXTENSION_FACE_HEIGHT_RATIO: f64 = 1.25;
/// A face is compared with its neighbours this close in time: a false detection never moves a
/// caption, and a real shot at least this long keeps its own zone.
const OUTLIER_WINDOW_SECONDS: f64 = 5.0;
/// Fewer samples than this say nothing about which of them is wrong.
const OUTLIER_MINIMUM_SAMPLES: usize = 3;
/// `caption_safe_wrap`: a conservative estimate, so real glyphs fit; everything else takes a full em.
const NARROW_GLYPH_EM: f64 = 0.6;
const HALF_WIDTH_GLYPH_EM: f64 = 0.5;
/// Resolve sizes an FCPXML title by the frame's height: one kanji's advance, measured over 720x1280,
/// 1280x720, 1920x1080, 1080x1920 and 3840x2160 renders, is `fontSize` times height / 1080 times this.
const FCPXML_ADVANCE_RATIO: f64 = 0.974;
const FCPXML_REFERENCE_HEIGHT: f64 = 1080.0;
const SENTENCE_ENDS: [char; 6] = ['。', '！', '？', '!', '?', '．'];
/// Kinsoku rules shared with `replay.speaker_subtitles` and `caption_linguistics`.
const NO_LINE_START: &str = "、。！？!?.,;:…」』）)]｝〜～";
const NO_LINE_END: &str = "「『（([｛";
/// A sokuon belongs to the kana after it, so なくっ|て splits one sound even at a morpheme end.
const GEMINATES: &str = "っッ";
/// A page this short ("かな?") reads as a stray fragment.
const WRAP_ORPHAN_MAXIMUM: usize = 4;
/// `TRAILING_PARTICLE_MAXIMUM`: a word this short is a tail or a stray, not a caption of its own.
const STRAY_CHARACTERS: usize = 2;
/// `caption_timing` word filters.
const PUNCTUATION: &str = "、。！？!?.,;:…「」『』（）()\"' ";
const NON_LEXICAL_MARKS: &str = "ー〜～";
const ELONGATION_VOWELS: &str = "ぁぃぅぇぉァィゥェォ";
const ELONGATION_MAXIMUM: usize = 24;
const DEGENERATE_UNIT_MAXIMUM: usize = 3;
const DEGENERATE_MINIMUM_REPEATS: usize = 8;
const LOOP_UNITS_PER_SECOND: f64 = 15.0;
/// `asr_hallucination`: longest first, so a longer phrase claims its tokens.
pub(crate) const STOCK_PHRASES: [&str; 6] = [
    "最後までご視聴いただきありがとうございました",
    "ご覧いただきありがとうございました",
    "チャンネル登録よろしくお願いします",
    "ご視聴ありがとうございました",
    "ご視聴ありがとうございます",
    "チャンネル登録お願いします",
];
const STOCK_PHRASE_MARKS: &str = "。、，,．.！!？?…・ 　";
const CLOSING_WINDOW_SECONDS: f64 = 90.0;
/// Page cost weights, tuned by reading real transcripts. Only their order matters much: a page
/// ends at a clause rather than a particle, at a particle rather than mid-phrase, and it breaks a
/// word only when nothing else fits.
const PAGE_COST: f64 = 1.0;
const CLOSING_BREAK_COST: f64 = 0.3;
const CONNECTIVE_BREAK_COST: f64 = 0.6;
const PARTICLE_BREAK_COST: f64 = 1.2;
const ARGUMENT_BREAK_COST: f64 = 4.0;
const PLAIN_BREAK_COST: f64 = 2.0;
const INSIDE_TOKEN_COST: f64 = 0.5;
const ATOMIC_BREAK_COST: f64 = 6.0;
const MORPHEME_BREAK_COST: f64 = 15.0;
const OVERFLOW_BREAK_COST: f64 = 25.0;
const CHARACTER_BREAK_COST: f64 = 40.0;
const MORPHEME_LINE_COST: f64 = 6.0;
const CHARACTER_LINE_COST: f64 = 25.0;
const TINY_PAGE_COST: f64 = 6.0;
const SHORT_PAGE_COST: f64 = 2.0;
/// A page under `MIN_DISPLAY` is the last thing to accept, short of breaking a word.
const FLASH_COST: f64 = 30.0;
const FAST_PAGE_COST: f64 = 3.0;
/// Words with no time of their own read infinitely fast; that must not outweigh leaving the frame.
const FASTEST_PAGE_COST: f64 = 20.0;
/// A page filling its line reads worse than two natural shorter ones.
const WIDTH_PAGE_COST: f64 = 1.5;
/// A line wider than this share of the limit splits at a good phrase end if it has one.
const COMFORTABLE_LINE_SHARE: f64 = 0.66;
const FORCED_PAGE_COST: f64 = 1000.0;
const OVERLONG_PAGE_COST: f64 = 100.0;
/// Full-width characters of imbalance a line break accepts per unit of break cost.
const LINE_BREAK_EMS_PER_COST: f64 = 4.0;

/// One speaker's continuous speech, by character, with everything paging needs.
struct Run<'a> {
    chars: Vec<char>,
    times: Vec<(f64, f64)>,
    token_edges: HashSet<usize>,
    widths: Vec<f64>,
    visible: Vec<usize>,
    boundaries: Boundaries,
    graphemes: Vec<usize>,
    kept_runs: Vec<(usize, usize)>,
    metrics: &'a Metrics,
    speaker: Option<u32>,
    /// Silence after the run, which keeps its last page readable without more speech.
    following_gap: f64,
}

/// Where a shot's faces stay, as the union of their clean samples.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FaceZone {
    start: f64,
    end: f64,
    head_top: f64,
    top: f64,
    bottom: f64,
    left: f64,
    right: f64,
}

/// One fixed vertical position: `Above` holds a block's bottom edge, `Below` its top edge, so a
/// one-line and a two-line caption share it (`caption_fixed_y`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Spot {
    Above(f64),
    Below(f64),
}

/// Face-relative placement over one video, with its sizes already scaled to the frame.
struct FacePlacer {
    anchor: Anchor,
    frame_w: f64,
    frame_h: f64,
    safe_left: f64,
    safe_right: f64,
    margin: f64,
    padding: f64,
    gap: f64,
    tallest: f64,
    clean: Vec<FaceSample>,
    zones: Vec<FaceZone>,
    current: Option<Spot>,
}

struct Layout {
    split: Option<usize>,
    penalty: f64,
}

/// A stretch of audio Silero VAD hears a voice in.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct VoiceSpan {
    pub start: f64,
    pub end: f64,
}

/// A cue's text and speech span, before placement and hold.
#[derive(Clone, Debug, PartialEq)]
struct Page {
    start: f64,
    end: f64,
    speaker: Option<u32>,
    lines: Vec<String>,
    /// The narrowest natural two lines, for when only a narrow place beside a face is free.
    narrow: Vec<String>,
}

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
    /// Below 1 only when more rows are shown together than fit the frame at full size.
    #[serde(default = "unscaled", skip_serializing_if = "is_unscaled")]
    pub scale: f64,
}

fn is_unscaled(scale: &f64) -> bool {
    *scale == 1.0
}

/// Font and width for one video, derived from its resolution.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Metrics {
    pub font_px: f64,
    pub outline_px: f64,
    pub max_line_px: f64,
    /// The chosen font's advances for laying out a video; `None` estimates them. A transcript stores
    /// none: it is laid out again with the computer's own font.
    #[serde(skip)]
    pub glyphs: Option<&'static GlyphWidths>,
    /// How much larger than `font_px` the chosen format is drawn.
    #[serde(default = "unscaled")]
    pub rendered_scale: f64,
}

const fn unscaled() -> f64 {
    1.0
}

fn fcpxml_title_scale(info: &VideoInfo) -> f64 {
    f64::from(info.height) / FCPXML_REFERENCE_HEIGHT * FCPXML_ADVANCE_RATIO
}

/// Font and caption width scale with the video, so vertical and 4K need no extra settings.
#[must_use]
pub fn metrics_for(info: &VideoInfo, settings: &Settings) -> Metrics {
    let short_edge = f64::from(info.width.min(info.height));
    let font_px = (short_edge * settings.font_ratio).round().max(1.0);
    let outline_px = (font_px * settings.widest_edge_ratio()).max(1.0);
    let frame_w = f64::from(info.width);
    let safe_share = 1.0 - (settings.safe_left_percent + settings.safe_right_percent) / PERCENT;
    let usable = (frame_w * settings.width_ratio).min(frame_w * safe_share);
    let rendered_scale = match settings.subtitle_format {
        // A timeline is drawn at FCPXML's measured size, so the two formats look the same.
        SubtitleFormat::Drt | SubtitleFormat::Fcpxml => fcpxml_title_scale(info),
        SubtitleFormat::Ttml | SubtitleFormat::Srt | SubtitleFormat::Capcut => 1.0,
    };
    // Whole pixels: the transcript stores this, and a whole number reads back exactly.
    let max_line_px = (usable - 2.0 * block_edge(font_px, outline_px, settings))
        .floor()
        .max((font_px * rendered_scale).ceil());
    Metrics {
        font_px,
        outline_px,
        max_line_px,
        glyphs: None,
        rendered_scale,
    }
}

fn overlap(word: &Word, turn: &SpeakerTurn) -> f64 {
    (word.end.min(turn.end) - word.start.max(turn.start)).max(0.0)
}

fn turn_order(left: &SpeakerTurn, right: &SpeakerTurn) -> std::cmp::Ordering {
    left.start.total_cmp(&right.start)
}

/// The number a turn should show: its own, unless others speak longer around it. Real overlaps stay.
fn steady_speaker(turn: &SpeakerTurn, near: &[SpeakerTurn]) -> u32 {
    let shared = |other: &SpeakerTurn, from: f64, to: f64| {
        (other.end.min(to) - other.start.max(from)).max(0.0)
    };
    let together = near.iter().any(|other| {
        other.speaker != turn.speaker
            && shared(other, turn.start, turn.end) >= TRULY_TOGETHER_SECONDS
    });
    if together {
        return turn.speaker;
    }
    let middle = (turn.start + turn.end) / 2.0;
    let (from, to) = (
        middle - STEADY_WINDOW_SECONDS / 2.0,
        middle + STEADY_WINDOW_SECONDS / 2.0,
    );
    let mut amounts: Vec<(u32, f64)> = vec![(turn.speaker, turn.end - turn.start)];
    for other in near {
        let share = shared(other, from, to);
        match amounts
            .iter_mut()
            .find(|(speaker, _)| *speaker == other.speaker)
        {
            Some((_, total)) => *total += share,
            None => amounts.push((other.speaker, share)),
        }
    }
    // The turn's own number comes first, so a tie keeps it.
    amounts
        .iter()
        .fold((turn.speaker, f64::MIN), |best, entry| {
            if entry.1 > best.1 {
                *entry
            } else {
                best
            }
        })
        .0
}

/// Steadies the model's numbers, which flicker between voices it finds alike within a few seconds.
#[must_use]
pub fn steadied_turns(turns: &[SpeakerTurn]) -> Vec<SpeakerTurn> {
    let mut ordered = turns.to_vec();
    ordered.sort_by(turn_order);
    let longest = ordered
        .iter()
        .map(|turn| turn.end - turn.start)
        .fold(0.0, f64::max);
    ordered
        .iter()
        .map(|turn| {
            let middle = (turn.start + turn.end) / 2.0;
            let reach = STEADY_WINDOW_SECONDS / 2.0 + longest;
            let first = ordered.partition_point(|other| other.start < middle - reach);
            let last = ordered.partition_point(|other| other.start <= middle + reach);
            SpeakerTurn {
                speaker: steady_speaker(turn, &ordered[first..last]),
                ..*turn
            }
        })
        .collect()
}

/// The speaker whose turns cover the word longest; on a tie, whoever spoke the word before.
fn longest_voice(word: &Word, turns: &[SpeakerTurn], previous: Option<u32>) -> Option<u32> {
    let mut amounts: Vec<(u32, f64)> = Vec::new();
    for turn in turns {
        let share = overlap(word, turn);
        if share <= 0.0 {
            continue;
        }
        match amounts
            .iter_mut()
            .find(|(speaker, _)| *speaker == turn.speaker)
        {
            Some((_, total)) => *total += share,
            None => amounts.push((turn.speaker, share)),
        }
    }
    let longest = amounts.iter().map(|entry| entry.1).fold(0.0, f64::max);
    let mut tied = amounts.iter().filter(|entry| entry.1 == longest);
    let first = tied.next()?.0;
    Some(
        previous
            .filter(|speaker| tied.any(|entry| entry.0 == *speaker) || first == *speaker)
            .unwrap_or(first),
    )
}

fn distance_to_turn(word: &Word, turn: &SpeakerTurn) -> f64 {
    (turn.start - word.end).max(word.start - turn.end).max(0.0)
}

fn nearest_speaker(word: &Word, turns: &[SpeakerTurn]) -> Option<u32> {
    turns
        .iter()
        .min_by(|left, right| {
            distance_to_turn(word, left).total_cmp(&distance_to_turn(word, right))
        })
        .map(|turn| turn.speaker)
}

/// Gives every word a speaker: the longest voice over it, else the nearest; with no turns at all, speaker 1.
pub fn assign_speakers(words: &mut [Word], turns: &[SpeakerTurn]) {
    let mut previous = None;
    for word in words {
        let speaker = longest_voice(word, turns, previous)
            .or_else(|| nearest_speaker(word, turns))
            .unwrap_or(FIRST_SPEAKER);
        word.speaker = Some(speaker);
        previous = Some(speaker);
    }
}

fn estimated_glyph_em(c: char) -> f64 {
    if c.is_ascii() {
        NARROW_GLYPH_EM
    } else if ('\u{FF61}'..='\u{FFDC}').contains(&c) {
        HALF_WIDTH_GLYPH_EM
    } else {
        1.0
    }
}

/// A character the font lacks takes the estimate: the renderer's fallback font cannot be read.
fn glyph_width(c: char, metrics: &Metrics) -> f64 {
    let em = metrics
        .glyphs
        .and_then(|glyphs| glyphs.em(c))
        .unwrap_or_else(|| estimated_glyph_em(c));
    metrics.font_px * em * metrics.rendered_scale
}

/// How far an outlined block reaches past its glyphs on each side. Resolve draws half of
/// `strokeWidth` in frame pixels on every frame and font size, whatever percent of the font it says,
/// and draws no FCPXML shadow at all.
fn block_edge(font_px: f64, outline_px: f64, settings: &Settings) -> f64 {
    match settings.subtitle_format {
        SubtitleFormat::Drt | SubtitleFormat::Fcpxml => outline_px / font_px * PERCENT,
        SubtitleFormat::Ttml | SubtitleFormat::Srt | SubtitleFormat::Capcut => outline_px,
    }
}

fn is_punctuation_only(text: &str) -> bool {
    text.chars()
        .all(|c| PUNCTUATION.contains(c) || c.is_whitespace())
}

fn is_lengthening(c: char) -> bool {
    ELONGATION_VOWELS.contains(c) || NON_LEXICAL_MARKS.contains(c)
}

/// The short unit a text only repeats, and how many times (`_repeated_unit`).
fn repeated_unit(text: &str) -> Option<(String, usize)> {
    let compact: Vec<char> = text.chars().filter(|c| !PUNCTUATION.contains(*c)).collect();
    if compact.is_empty() {
        return None;
    }
    (1..=DEGENERATE_UNIT_MAXIMUM)
        .find(|unit| {
            compact.len().is_multiple_of(*unit)
                && compact
                    .iter()
                    .enumerate()
                    .all(|(index, c)| *c == compact[index % unit])
        })
        .map(|unit| (compact[..unit].iter().collect(), compact.len() / unit))
}

fn is_degenerate_repetition(text: &str) -> bool {
    repeated_unit(text).is_some_and(|(_, count)| count >= DEGENERATE_MINIMUM_REPEATS)
}

/// The unit and length of a loop starting at `start`: `LOOP_RUN`, which `regex` cannot express.
fn loop_run_at(chars: &[char], start: usize) -> Option<(usize, usize)> {
    (1..=DEGENERATE_UNIT_MAXIMUM).find_map(|unit| {
        let pattern = chars.get(start..start + unit)?;
        let repeats = chars[start..]
            .chunks(unit)
            .take_while(|chunk| *chunk == pattern)
            .count();
        (repeats >= DEGENERATE_MINIMUM_REPEATS).then_some((unit, unit * repeats))
    })
}

/// Drops a loop run inside one token ("ララ…ラなん" keeps "なん"); small vowels still lengthen.
fn without_loop(word: &Word) -> String {
    let chars: Vec<char> = word.text.chars().collect();
    let seconds = (word.end - word.start).max(SHORTEST_SPAN_SECONDS);
    if (chars.len() as f64) < LOOP_UNITS_PER_SECOND * seconds {
        return word.text.clone();
    }
    let mut kept = String::new();
    let mut index = 0;
    while index < chars.len() {
        match loop_run_at(&chars, index) {
            Some((unit, length)) => {
                if chars[index..index + unit]
                    .iter()
                    .all(|c| is_lengthening(*c))
                {
                    kept.extend(&chars[index..index + length]);
                }
                index += length;
            }
            None => {
                kept.push(chars[index]);
                index += 1;
            }
        }
    }
    if kept.trim().is_empty() {
        word.text.clone()
    } else {
        kept
    }
}

/// Elongation tokens of a run once together they pass `ELONGATION_MAXIMUM` (`_past_elongation`).
fn past_elongation(words: &[&Word], first: usize, last: usize) -> Vec<usize> {
    let mut total = 0;
    (first..=last)
        .filter(|index| {
            total += words[*index].text.chars().count();
            total > ELONGATION_MAXIMUM
        })
        .collect()
}

/// Consecutive tokens repeating one short unit faster than speech are a recognizer loop, even when
/// each stays below the whole-token threshold (`_loop_runs`).
fn loop_runs(words: &[&Word]) -> HashSet<usize> {
    let mut loops = HashSet::new();
    let mut first = 0;
    while first < words.len() {
        let Some((unit, mut count)) = repeated_unit(&words[first].text) else {
            first += 1;
            continue;
        };
        let mut last = first;
        while let Some((_, more)) = words
            .get(last + 1)
            .and_then(|next| repeated_unit(&next.text))
            .filter(|(next_unit, _)| *next_unit == unit)
        {
            count += more;
            last += 1;
        }
        let seconds = (words[last].end - words[first].start).max(SHORTEST_SPAN_SECONDS);
        let fast = last > first && count as f64 >= LOOP_UNITS_PER_SECOND * seconds;
        if fast && unit.chars().all(is_lengthening) {
            loops.extend(past_elongation(words, first, last));
        } else if fast && count >= DEGENERATE_MINIMUM_REPEATS {
            loops.extend(first..=last);
        }
        first = last + 1;
    }
    loops
}

/// A small vowel or long mark right after a spoken kana lengthens that word (`_word_mark_continuation`).
fn is_word_mark(previous: &[Word], token: &Word) -> bool {
    let mut letters = token.text.chars();
    let lead = letters.next();
    let vowel_run =
        lead.is_some_and(|c| ELONGATION_VOWELS.contains(c)) && letters.all(|c| Some(c) == lead);
    if token.text != "ー" && !vowel_run {
        return false;
    }
    let mut following = token;
    for last in previous.iter().rev() {
        if last.speaker != following.speaker || following.start - last.end > PHRASE_GAP_SECONDS {
            return false;
        }
        if has_plain_kana(&last.text) {
            return true;
        }
        if !last
            .text
            .chars()
            .all(|c| ELONGATION_VOWELS.contains(c) || c == 'ー')
        {
            return false;
        }
        following = last;
    }
    false
}

fn has_plain_kana(text: &str) -> bool {
    text.chars()
        .any(|c| ('ぁ'..='ヺ').contains(&c) && !ELONGATION_VOWELS.contains(c) && c != 'ー')
}

/// Tokens spelling a credits phrase Whisper invents on silence or laughter, unless they fall where
/// a recording really closes (`asr_hallucination`; without its audio energy check here).
fn stock_phrase_tokens(words: &[Word], duration: f64) -> HashSet<usize> {
    let spelled: Vec<&str> = words
        .iter()
        .map(|word| word.text.trim_matches(|c| STOCK_PHRASE_MARKS.contains(c)))
        .collect();
    let text = spelled.concat();
    let mut starts = Vec::with_capacity(spelled.len());
    let mut position = 0;
    for piece in &spelled {
        starts.push(position);
        position += piece.len();
    }
    let mut taken = HashSet::new();
    let matches = STOCK_PHRASES
        .iter()
        .flat_map(|phrase| text.match_indices(phrase));
    for (found, phrase) in matches {
        let inside: Vec<usize> = (0..words.len())
            .filter(|index| {
                !spelled[*index].is_empty()
                    && starts[*index] >= found
                    && starts[*index] + spelled[*index].len() <= found + phrase.len()
            })
            .collect();
        let covered: usize = inside.iter().map(|index| spelled[*index].len()).sum();
        let closing = inside
            .last()
            .is_some_and(|index| words[*index].end >= duration - CLOSING_WINDOW_SECONDS);
        if covered == phrase.len() && !closing && inside.iter().all(|i| !taken.contains(i)) {
            taken.extend(inside);
        }
    }
    taken
}

/// What a caption shows of the recognized words, which themselves stay untouched for later re-layout.
fn display_words(words: &[Word], duration: f64) -> Vec<Word> {
    let stock = stock_phrase_tokens(words, duration);
    let spoken: Vec<&Word> = words
        .iter()
        .enumerate()
        // Whisper can run on past the end of the audio; no frame could show these.
        .filter(|(index, word)| !stock.contains(index) && word.start < duration)
        .map(|(_, word)| word)
        .collect();
    let loops = loop_runs(&spoken);
    let mut shown: Vec<Word> = Vec::with_capacity(spoken.len());
    for (index, word) in spoken.iter().enumerate() {
        if loops.contains(&index) {
            continue;
        }
        let token = Word {
            text: without_loop(word),
            end: word.end.min(word.start + MAX_CUE_SECONDS),
            ..(*word).clone()
        };
        let only_marks = token.text.chars().all(|c| NON_LEXICAL_MARKS.contains(c));
        if !is_word_mark(&shown, &token) && (only_marks || is_degenerate_repetition(&token.text)) {
            continue;
        }
        if is_punctuation_only(&token.text) {
            if let Some(previous) = shown.last_mut() {
                previous.text.push_str(&token.text);
            }
            continue;
        }
        shown.push(token);
    }
    shown
}

fn ends_sentence(word: &Word) -> bool {
    word.text.trim_end().ends_with(SENTENCE_ENDS)
}

/// Hard breaks only: speaker change, a real pause, and a sentence end once the page can be read.
fn starts_new_group(group: &[Word], next: &Word) -> bool {
    let (Some(first), Some(last)) = (group.first(), group.last()) else {
        return false;
    };
    let gap = next.start - last.end;
    if last.speaker != next.speaker {
        return true;
    }
    (gap > CUE_BREAK_PAUSE_SECONDS || ends_sentence(last))
        && next.start - first.start >= SHORTEST_GROUP_SECONDS
}

fn group_words(words: &[Word]) -> Vec<Vec<Word>> {
    let mut groups: Vec<Vec<Word>> = Vec::new();
    for word in words {
        match groups.last_mut() {
            Some(group) if !starts_new_group(group, word) => group.push(word.clone()),
            _ => groups.push(vec![word.clone()]),
        }
    }
    groups
}

/// A stretch too brief to read alone stays with the same speaker's words beside it on the page.
fn joins_stray(group: &[Word], next: &[Word]) -> bool {
    let (Some(last), Some(first)) = (group.last(), next.first()) else {
        return false;
    };
    let too_brief = next.iter().last().map_or(0.0, |word| word.end) - first.start
        < SHORTEST_GROUP_SECONDS
        || last.end - group.first().map_or(last.start, |word| word.start) < SHORTEST_GROUP_SECONDS;
    too_brief
        && last.speaker == first.speaker
        && first.start - last.end <= CUE_BREAK_PAUSE_SECONDS
        && !ends_sentence(last)
}

fn merge_strays(groups: Vec<Vec<Word>>) -> Vec<Vec<Word>> {
    let mut merged: Vec<Vec<Word>> = Vec::with_capacity(groups.len());
    for group in groups {
        match merged.last_mut() {
            Some(previous) if joins_stray(previous, &group) => previous.extend(group),
            _ => merged.push(group),
        }
    }
    merged
}

fn class_cost(class: BreakClass) -> f64 {
    match class {
        BreakClass::Clause => 0.0,
        BreakClass::Closing | BreakClass::Predicate => CLOSING_BREAK_COST,
        BreakClass::Connective => CONNECTIVE_BREAK_COST,
        BreakClass::Particle => PARTICLE_BREAK_COST,
        BreakClass::Plain => PLAIN_BREAK_COST,
        BreakClass::Argument | BreakClass::Compound => ARGUMENT_BREAK_COST,
        BreakClass::Ambiguous => ATOMIC_BREAK_COST,
    }
}

/// Whether a timing gap fell inside a word or before something that cannot stand alone (頑|張る,
/// っ|て, 雨|です): silence inside a word is no place for a caption to end.
fn splits_a_word(left: &[Word], right: &[Word], segmenter: &Segmenter) -> AppResult<bool> {
    let (Some(last), Some(first)) = (left.last(), right.first()) else {
        return Ok(false);
    };
    if last.speaker != first.speaker || first.start - last.end > SPLIT_WORD_REACH_SECONDS {
        return Ok(false);
    }
    let head: String = right.iter().map(|word| word.text.as_str()).collect();
    let gap = first.start - last.end;
    if gap <= DEPENDENT_WORD_REACH_SECONDS && segmenter.leans_on_previous(head.trim_start())? {
        return Ok(true);
    }
    if ends_sentence(last) {
        return Ok(false);
    }
    let tail: String = left.iter().map(|word| word.text.as_str()).collect();
    for context in JUNCTION_CONTEXT_CHARACTERS {
        if word_spans_gap(&tail, &head, context, gap, segmenter)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the morphology around a gap reads one word across it. A parse depends on what surrounds
/// it, and either reading is reason enough to keep the two sides together.
fn word_spans_gap(
    tail: &str,
    head: &str,
    context: usize,
    gap: f64,
    segmenter: &Segmenter,
) -> AppResult<bool> {
    let before: Vec<char> = tail.chars().rev().take(context).collect();
    let window: String = before
        .iter()
        .rev()
        .copied()
        .chain(head.chars().take(context))
        .collect();
    if !is_japanese(&window) {
        return Ok(false);
    }
    let boundaries = segmenter.boundaries(&window)?;
    if gap > DEPENDENT_WORD_REACH_SECONDS {
        return Ok(!boundaries.morphemes.contains(&before.len()));
    }
    Ok(!boundaries
        .phrases
        .iter()
        .any(|phrase| phrase.offset == before.len()))
}

/// Characters of one morpheme are said together. Warping can stretch a word's first token back
/// over the silence before it (明[2040.3] 日[2041.6] for 明日 heard at 2041.3), which would show a
/// caption seconds early and split the word: such a token moves up to just before the next one.
fn close_split_morphemes(words: &mut [Word], segmenter: &Segmenter) -> AppResult<()> {
    let text: String = words.iter().map(|word| word.text.as_str()).collect();
    if text.is_empty() {
        return Ok(());
    }
    let inside: HashSet<usize> = segmenter.boundaries(&text)?.morphemes.into_iter().collect();
    let mut offsets = Vec::with_capacity(words.len());
    let mut offset = 0;
    for word in words.iter() {
        offset += word.text.chars().count();
        offsets.push(offset);
    }
    for index in (0..words.len().saturating_sub(1)).rev() {
        let (next_start, next_speaker) = (words[index + 1].start, words[index + 1].speaker);
        let word = &mut words[index];
        let said = word.text.trim().chars().count() as f64 * LONGEST_SPOKEN_CHARACTER_SECONDS;
        let stretched = next_start - word.start;
        if !inside.contains(&offsets[index])
            && word.speaker == next_speaker
            && stretched > said
            && stretched <= STRETCHED_WORD_REACH_SECONDS
        {
            word.start = next_start - said;
            word.end = next_start;
        }
    }
    Ok(())
}

fn rejoin_split_words(groups: Vec<Vec<Word>>, segmenter: &Segmenter) -> AppResult<Vec<Vec<Word>>> {
    let mut joined: Vec<Vec<Word>> = Vec::with_capacity(groups.len());
    for group in groups {
        match joined.last_mut() {
            Some(previous) if splits_a_word(previous, &group, segmenter)? => previous.extend(group),
            _ => joined.push(group),
        }
    }
    Ok(joined)
}

fn is_line_start_prohibited(c: char) -> bool {
    NO_LINE_START.contains(c) || DEPENDENT_KANA.contains(c)
}

impl<'a> Run<'a> {
    fn new(
        group: &[Word],
        following_gap: f64,
        metrics: &'a Metrics,
        segmenter: &Segmenter,
    ) -> AppResult<Self> {
        let mut chars: Vec<char> = Vec::new();
        let mut times: Vec<(f64, f64)> = Vec::new();
        let mut token_edges: HashSet<usize> = HashSet::new();
        for word in group {
            token_edges.insert(chars.len());
            let count = word.text.chars().count() as f64;
            let step = (word.end - word.start) / count;
            chars.extend(word.text.chars());
            times.extend(word.text.chars().enumerate().map(|(index, _)| {
                let offset = word.start + step * index as f64;
                (offset, offset + step)
            }));
        }
        let text: String = chars.iter().collect();
        let boundaries = segmenter.boundaries(&text)?;
        let mut widths = vec![0.0];
        let mut visible = vec![0];
        for c in &chars {
            widths.push(widths[widths.len() - 1] + glyph_width(*c, metrics));
            visible.push(visible[visible.len() - 1] + usize::from(!c.is_whitespace()));
        }
        let mut run = Self {
            chars,
            times,
            token_edges,
            widths,
            visible,
            boundaries,
            graphemes: grapheme_ends(&text),
            kept_runs: Vec::new(),
            metrics,
            speaker: group.iter().find_map(|word| word.speaker),
            following_gap,
        };
        run.kept_runs = run.fitting_word_runs();
        Ok(run)
    }

    /// Katakana or Latin runs that fit a line stay whole (`fitting_runs` in `wrap_measured_lines`).
    fn fitting_word_runs(&self) -> Vec<(usize, usize)> {
        let mut runs = Vec::new();
        let mut start = 0;
        while start < self.chars.len() {
            let latin = is_latin_or_number(self.chars[start]);
            let is_part = |c: &char| {
                if latin {
                    is_latin_or_number(*c) || *c == '_'
                } else {
                    is_katakana_run(*c)
                }
            };
            let length = self.chars[start..]
                .iter()
                .take_while(|c| is_part(c))
                .count();
            if length > 1 && self.width(start, start + length) <= self.metrics.max_line_px {
                runs.push((start, start + length));
            }
            start += length.max(1);
        }
        runs
    }

    fn width(&self, start: usize, end: usize) -> f64 {
        self.widths[end] - self.widths[start]
    }

    fn count(&self, start: usize, end: usize) -> usize {
        self.visible[end] - self.visible[start]
    }

    /// `start..end` without the spaces at its edges.
    fn trimmed(&self, mut start: usize, mut end: usize) -> (usize, usize) {
        while start < end && self.chars[start].is_whitespace() {
            start += 1;
        }
        while start < end && self.chars[end - 1].is_whitespace() {
            end -= 1;
        }
        (start, end)
    }

    /// Kinsoku at a line break, looking past the space the break removes.
    fn may_break_line(&self, point: usize) -> bool {
        let (before, after) = self.trimmed(point, self.chars.len());
        let (head, last) = self.trimmed(0, point);
        before < after
            && head < last
            && !is_line_start_prohibited(self.chars[before])
            && !NO_LINE_END.contains(self.chars[last - 1])
            && !(GEMINATES.contains(self.chars[last - 1]) && is_kana(self.chars[before]))
    }

    /// What a line break at `point` costs beyond imbalance: only phrase ends differ in strength.
    fn line_break_cost(&self, point: usize, phrase_tier: bool) -> f64 {
        let phrases = &self.boundaries.phrases;
        let found = phrases.binary_search_by_key(&point, |phrase| phrase.offset);
        match found {
            Ok(index) if phrase_tier => class_cost(phrases[index].class) * LINE_BREAK_EMS_PER_COST,
            _ => 0.0,
        }
    }

    /// The most balanced legal two-line split among `points` (`wrap_measured_lines`), weighing how
    /// strong each phrase end is, so a clause wins over a slightly evener mid-phrase break.
    fn balanced_split(
        &self,
        points: &[usize],
        start: usize,
        end: usize,
        keep_runs: bool,
    ) -> Option<usize> {
        let first = points.partition_point(|point| *point <= start);
        let last = points.partition_point(|point| *point < end);
        points[first..last]
            .iter()
            .copied()
            .filter(|point| self.may_break_line(*point))
            .filter(|point| {
                !keep_runs || !self.kept_runs.iter().any(|(a, b)| a < point && point < b)
            })
            .filter_map(|point| {
                let (left_start, left_end) = self.trimmed(start, point);
                let (right_start, right_end) = self.trimmed(point, end);
                let (left, right) = (
                    self.width(left_start, left_end),
                    self.width(right_start, right_end),
                );
                let imbalance = (left - right).abs() / self.metrics.font_px;
                (left.max(right) <= self.metrics.max_line_px)
                    .then_some((point, imbalance + self.line_break_cost(point, keep_runs)))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(point, _)| point)
    }

    /// Lines of one page: phrase ends, then intact morphemes, then characters for a single long word.
    fn lay_out(&self, start: usize, end: usize) -> Option<Layout> {
        let (start, end) = self.trimmed(start, end);
        let width = self.width(start, end);
        if width <= COMFORTABLE_LINE_SHARE * self.metrics.max_line_px {
            return Some(Layout {
                split: None,
                penalty: 0.0,
            });
        }
        let phrase_split = self.balanced_split(&self.boundaries.line_ends, start, end, true);
        if width <= self.metrics.max_line_px {
            let good = phrase_split.filter(|point| {
                self.line_break_cost(*point, true) <= PARTICLE_BREAK_COST * LINE_BREAK_EMS_PER_COST
            });
            return Some(Layout {
                split: good,
                penalty: 0.0,
            });
        }
        if let Some(point) = phrase_split {
            return Some(Layout {
                split: Some(point),
                penalty: 0.0,
            });
        }
        if let Some(point) = self.balanced_split(&self.boundaries.morphemes, start, end, false) {
            return Some(Layout {
                split: Some(point),
                penalty: MORPHEME_LINE_COST,
            });
        }
        self.balanced_split(&self.graphemes, start, end, false)
            .map(|point| Layout {
                split: Some(point),
                penalty: CHARACTER_LINE_COST,
            })
    }

    fn speech_seconds(&self, start: usize, end: usize) -> f64 {
        let (start, end) = self.trimmed(start, end);
        if start == end {
            return 0.0;
        }
        self.times[end - 1].1 - self.times[start].0
    }

    fn within_budget(&self, start: usize, end: usize) -> bool {
        self.speech_seconds(start, end) <= MAX_CUE_SECONDS
    }

    fn fits(&self, start: usize, end: usize) -> bool {
        self.within_budget(start, end) && self.lay_out(start, end).is_some()
    }

    fn phrase_cost(&self, phrase: &PhraseEnd) -> f64 {
        let class_cost = class_cost(phrase.class);
        if phrase.offset >= self.chars.len() {
            return 0.0;
        }
        let gap = self.times[phrase.offset].0 - self.times[phrase.offset - 1].1;
        // A real gap makes any phrase end a natural break, except where a predicate or a misread
        // word would be cut off: a speaker can pause there without finishing.
        let weak = matches!(phrase.class, BreakClass::Argument | BreakClass::Ambiguous);
        if gap >= PHRASE_GAP_SECONDS && !weak {
            return 0.0;
        }
        if self.token_edges.contains(&phrase.offset) {
            class_cost
        } else {
            class_cost + INSIDE_TOKEN_COST
        }
    }

    /// Phrase ends, refined only where a span cannot fit: words, then characters as the last resort.
    fn page_breaks(&self) -> Vec<(usize, f64)> {
        let mut breaks: BTreeMap<usize, f64> = self
            .boundaries
            .phrases
            .iter()
            .filter(|phrase| {
                phrase.offset == self.chars.len() || self.may_break_line(phrase.offset)
            })
            .map(|phrase| (phrase.offset, self.phrase_cost(phrase)))
            .collect();
        breaks.insert(0, 0.0);
        // Time may split at words only; a token Whisper stretched past the limit is cut at display.
        let levels = [
            (&self.boundaries.atomic, ATOMIC_BREAK_COST, true),
            (&self.boundaries.morphemes, MORPHEME_BREAK_COST, true),
            (&self.boundaries.overflow, OVERFLOW_BREAK_COST, false),
            (&self.graphemes, CHARACTER_BREAK_COST, false),
        ];
        for (points, cost, timed) in levels {
            let offsets: Vec<usize> = breaks.keys().copied().collect();
            let tight: Vec<&[usize]> = offsets
                .windows(2)
                .filter(|span| {
                    if timed {
                        !self.fits(span[0], span[1])
                    } else {
                        self.lay_out(span[0], span[1]).is_none()
                    }
                })
                .collect();
            let added: Vec<usize> = points
                .iter()
                .copied()
                .filter(|point| self.may_break_line(*point))
                .filter(|point| {
                    tight
                        .iter()
                        .any(|span| span[0] < *point && *point < span[1])
                })
                .collect();
            breaks.extend(added.into_iter().map(|point| (point, cost)));
        }
        breaks.into_iter().collect()
    }

    /// How badly a page reads: fewer, balanced, readable pages ending at strong boundaries win.
    fn page_cost(&self, start: usize, end: usize) -> f64 {
        let count = self.count(start, end);
        if count == 0 {
            return 0.0;
        }
        // Longer is worse even when forced, so nothing else joins a page that cannot fit.
        let shown_ems = self.width(start, end) / self.metrics.font_px;
        let Some(layout) = self.lay_out(start, end) else {
            return FORCED_PAGE_COST + shown_ems;
        };
        let seconds = self.speech_seconds(start, end);
        // How much a page holds follows the speech: the same time holds more when it is fast.
        let fill = seconds / PAGE_TARGET_SECONDS;
        let tiny = match count {
            0..=STRAY_CHARACTERS => TINY_PAGE_COST,
            _ if count <= WRAP_ORPHAN_MAXIMUM => SHORT_PAGE_COST,
            _ => 0.0,
        };
        let on_screen = if end >= self.chars.len() {
            seconds + self.following_gap
        } else {
            seconds
        };
        let flash = if on_screen < MIN_DISPLAY {
            FLASH_COST
        } else {
            0.0
        };
        let overrun = if self.within_budget(start, end) {
            0.0
        } else {
            OVERLONG_PAGE_COST
        };
        // In full-width character widths, so a narrow Latin letter counts for what it is.
        let rate = shown_ems / seconds.max(SHORTEST_SPAN_SECONDS);
        let fast =
            ((rate / FAST_READING_CPS - 1.0).max(0.0) * FAST_PAGE_COST).min(FASTEST_PAGE_COST);
        let filled = self.width(start, end) / self.metrics.max_line_px;
        PAGE_COST
            + fill * fill
            + WIDTH_PAGE_COST * filled * filled
            + tiny
            + flash
            + fast
            + overrun
            + layout.penalty
    }

    /// The cheapest way to reach `breaks[index]`, as `(cost, previous break)`.
    fn best_arrival(&self, breaks: &[(usize, f64)], best: &[f64], index: usize) -> (f64, usize) {
        let (end, end_cost) = breaks[index];
        let mut arrival = (f64::INFINITY, index - 1);
        for previous in (0..index).rev() {
            let start = breaks[previous].0;
            let cost = best[previous] + self.page_cost(start, end) + end_cost;
            if cost < arrival.0 {
                arrival = (cost, previous);
            }
            if self.width(start, end) > MAX_LINES as f64 * self.metrics.max_line_px {
                break;
            }
        }
        arrival
    }

    /// Splits the run into pages by phrase boundaries, weighing all of them rather than filling greedily.
    fn page_spans(&self) -> Vec<(usize, usize)> {
        let breaks = self.page_breaks();
        let mut best = vec![0.0; breaks.len()];
        let mut from = vec![0; breaks.len()];
        for index in 1..breaks.len() {
            (best[index], from[index]) = self.best_arrival(&breaks, &best, index);
        }
        let mut spans = Vec::new();
        let mut index = breaks.len() - 1;
        while index > 0 {
            spans.push((breaks[from[index]].0, breaks[index].0));
            index = from[index];
        }
        spans.reverse();
        spans
    }

    fn line(&self, start: usize, end: usize) -> String {
        let (start, end) = self.trimmed(start, end);
        self.chars[start..end].iter().collect()
    }

    fn pages(&self) -> Vec<Page> {
        self.page_spans()
            .into_iter()
            .filter(|(start, end)| self.count(*start, *end) > 0)
            .map(|(start, end)| {
                let (start, end) = self.trimmed(start, end);
                let split = self.lay_out(start, end).and_then(|layout| layout.split);
                let lines = split.map_or_else(
                    || vec![self.line(start, end)],
                    |point| vec![self.line(start, point), self.line(point, end)],
                );
                let narrow_split = self
                    .balanced_split(&self.boundaries.line_ends, start, end, true)
                    .or_else(|| self.balanced_split(&self.boundaries.morphemes, start, end, false));
                let narrow = narrow_split.map_or_else(
                    || lines.clone(),
                    |point| vec![self.line(start, point), self.line(point, end)],
                );
                Page {
                    narrow,
                    start: self.times[start].0,
                    end: self.times[end - 1].1,
                    speaker: self.speaker,
                    lines,
                }
            })
            .collect()
    }
}

/// The time a text needs on screen at its speech's rate (`caption_display_hold.required`).
fn required_seconds(characters: usize, start: f64, end: f64) -> f64 {
    let count = characters as f64;
    let spoken = count / (end - start).max(SHORTEST_SPAN_SECONDS);
    let rate = spoken.clamp(READING_CPS, FAST_READING_CPS);
    MIN_DISPLAY.max(count / rate)
}

/// A cue's end: readable, never stale, never delaying the next cue (`caption_display_hold.held_end`).
fn held_end(page: &Page, following: f64, rate: f64) -> f64 {
    let start = page.start;
    let end = page.end.min(start + MAX_CUE_SECONDS);
    let characters: usize = page
        .lines
        .iter()
        .map(|line| line.chars().filter(|c| !c.is_whitespace()).count())
        .sum();
    let latest = (start + MAX_CUE_SECONDS).min(following - MIN_GAP_FRAMES / rate);
    let wanted = (start + required_seconds(characters, start, end))
        .max(end + LINGER)
        .min(latest);
    if wanted <= end {
        return end.min(following).max(start + SHORTEST_SPAN_SECONDS);
    }
    wanted
}

/// The start and end that give a cue at least one frame of its own (`ensure_display_frames`).
fn on_frames(start: f64, end: f64, previous_end: f64, rate: f64) -> (f64, f64) {
    let start = start.max(previous_end);
    let first_frame = (start * rate).ceil() / rate;
    (
        start,
        if end <= first_frame {
            first_frame + 1.0 / rate
        } else {
            end
        },
    )
}

/// `caption_layout_file.resolve`: one line's measured box plus a further line's advance.
fn block_height(line_count: usize, metrics: &Metrics, settings: &Settings) -> f64 {
    metrics.font_px
        * metrics.rendered_scale
        * (EM_LINE_HEIGHT + line_count.saturating_sub(1) as f64 * settings.line_height)
}

fn block_width(lines: &[String], metrics: &Metrics, settings: &Settings) -> f64 {
    let widest = lines
        .iter()
        .map(|line| line.chars().map(|c| glyph_width(c, metrics)).sum::<f64>())
        .fold(0.0, f64::max);
    widest + 2.0 * block_edge(metrics.font_px, metrics.outline_px, settings)
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied()
}

/// Drops samples far from the median of their neighbours in time: detector mistakes, not shots.
fn without_outliers(faces: &[FaceSample], drift: f64) -> Vec<FaceSample> {
    let is_close = |face: &FaceSample, value: fn(&FaceSample) -> f64| {
        let first = faces.partition_point(|near| near.time < face.time - OUTLIER_WINDOW_SECONDS);
        let last = faces.partition_point(|near| near.time <= face.time + OUTLIER_WINDOW_SECONDS);
        let near = &faces[first..last];
        near.len() < OUTLIER_MINIMUM_SAMPLES
            || median(near.iter().map(value).collect())
                .is_some_and(|middle| (value(face) - middle).abs() <= drift)
    };
    faces
        .iter()
        .filter(|face| {
            is_close(face, |near| near.top)
                && is_close(face, |near| near.bottom)
                && is_close(face, |near| near.center_x)
        })
        .copied()
        .collect()
}

fn zone_of(face: &FaceSample) -> FaceZone {
    let height = face.bottom - face.top;
    FaceZone {
        start: face.time,
        end: face.time,
        head_top: (face.top - HEAD_EXTENSION_FACE_HEIGHT_RATIO * height).max(0.0),
        top: face.top,
        bottom: face.bottom,
        // The sample holds no width; a detected face box is about as wide as it is tall.
        left: face.center_x - height / 2.0,
        right: face.center_x + height / 2.0,
    }
}

fn union(left: &FaceZone, right: &FaceZone) -> FaceZone {
    FaceZone {
        start: left.start.min(right.start),
        end: left.end.max(right.end),
        head_top: left.head_top.min(right.head_top),
        top: left.top.min(right.top),
        bottom: left.bottom.max(right.bottom),
        left: left.left.min(right.left),
        right: left.right.max(right.right),
    }
}

/// A new zone starts when a face moves away from where its zone began by more than `drift`.
fn face_zones(faces: &[FaceSample], drift: f64) -> Vec<FaceZone> {
    let mut zones: Vec<FaceZone> = Vec::new();
    let mut reference: Option<FaceSample> = None;
    for face in faces {
        let moved = reference.is_none_or(|first| {
            (face.top - first.top).abs() > drift
                || (face.bottom - first.bottom).abs() > drift
                || (face.center_x - first.center_x).abs() > drift
        });
        match zones.last_mut() {
            Some(zone) if !moved => *zone = union(zone, &zone_of(face)),
            _ => {
                zones.push(zone_of(face));
                reference = Some(*face);
            }
        }
    }
    zones
}

impl FacePlacer {
    fn new(faces: &[FaceSample], info: &VideoInfo, metrics: &Metrics, settings: &Settings) -> Self {
        let scale = f64::from(info.width.min(info.height)) / REFERENCE_SHORT_EDGE;
        let drift = MAX_DRIFT_PX * scale;
        let clean = without_outliers(faces, drift);
        Self {
            anchor: settings.placement.anchor,
            frame_w: f64::from(info.width),
            frame_h: f64::from(info.height),
            safe_left: f64::from(info.width) * settings.safe_left_percent / PERCENT,
            safe_right: f64::from(info.width) * settings.safe_right_percent / PERCENT,
            margin: MARGIN_PX * scale,
            padding: PADDING_PX * scale,
            gap: FACE_GAP_PX * scale,
            tallest: block_height(MAX_LINES, metrics, settings),
            zones: face_zones(&clean, drift),
            clean,
            current: None,
        }
    }

    /// Everywhere a face stays during a span; between shots, the shots on both sides.
    fn zone_during(&self, start: f64, end: f64) -> Option<FaceZone> {
        let during = |zone: &&FaceZone| zone.start <= end && zone.end >= start;
        let before = self.zones.iter().rev().find(|zone| zone.end < start);
        let after = self.zones.iter().find(|zone| zone.start > end);
        let mut found: Vec<&FaceZone> = self.zones.iter().filter(during).collect();
        if found.is_empty() {
            found.extend(before.into_iter().chain(after));
        }
        found
            .into_iter()
            .copied()
            .reduce(|left, right| union(&left, &right))
    }

    fn above(&self, reference: f64, margin: f64) -> Option<Spot> {
        let bottom = reference - self.gap;
        (bottom - self.tallest >= margin).then_some(Spot::Above(bottom))
    }

    fn below(&self, zone: &FaceZone, margin: f64) -> Option<Spot> {
        let top = zone.bottom + self.gap;
        (top + self.tallest <= self.frame_h - margin).then_some(Spot::Below(top))
    }

    /// The side in use or asked for, the other side, then the same inside the padding.
    fn vertical_spot(&self, zone: &FaceZone) -> Option<Spot> {
        let (margin, padding) = (self.margin, self.padding);
        let above = |margin| self.above(zone.head_top, margin);
        let below = |margin| self.below(zone, margin);
        let prefers_below = match self.current {
            Some(Spot::Below(_)) => true,
            Some(Spot::Above(_)) => false,
            None => self.anchor == Anchor::FaceBelow,
        };
        if prefers_below {
            below(margin)
                .or_else(|| above(margin))
                .or_else(|| below(padding))
                .or_else(|| above(padding))
        } else {
            above(margin)
                .or_else(|| below(margin))
                .or_else(|| above(padding))
                .or_else(|| below(padding))
        }
    }

    /// Whether the tallest block at the current position clears the head and face; while it does,
    /// a caption never follows the face (`caption_fixed_y`, which holds one origin over a run).
    fn holds(&self, zone: &FaceZone) -> bool {
        match self.current {
            Some(Spot::Above(bottom)) => bottom <= zone.head_top,
            Some(Spot::Below(top)) => top >= zone.bottom,
            None => false,
        }
    }

    /// No two-line room around the face. The low edge keeps captions in one place: centred when
    /// this block's own height clears the face, else beside it on the wider free side; only then
    /// over the face box at the top, under the hair the head estimate protects.
    fn free_center(&self, zone: &FaceZone, block: (f64, f64)) -> Option<(f64, f64)> {
        let (width, height) = block;
        let low = self.frame_h - self.padding - height / 2.0;
        if low - height / 2.0 >= zone.bottom {
            return Some((self.frame_w / 2.0, low));
        }
        let free_left = zone.left - self.gap - self.safe_left;
        let free_right = self.frame_w - self.safe_right - zone.right - self.gap;
        if free_left >= free_right && width <= free_left {
            return Some((self.safe_left + free_left / 2.0, low));
        }
        if free_right > free_left && width <= free_right {
            return Some((self.frame_w - self.safe_right - free_right / 2.0, low));
        }
        let high = zone.top - self.gap - height;
        (high >= self.padding).then_some((self.frame_w / 2.0, high + height / 2.0))
    }

    /// Whether a block of this size can avoid the faces of a span without moving past the frame.
    fn has_room(&self, start: f64, end: f64, block: (f64, f64)) -> bool {
        let Some(zone) = self.zone_during(start, end) else {
            return true;
        };
        self.holds(&zone)
            || self.vertical_spot(&zone).is_some()
            || self.free_center(&zone, block).is_some()
            || self
                .zone_of_span(start, end)
                .and_then(|span| self.free_center(&span, block))
                .is_some()
    }

    /// Only the samples inside the span, when the whole shot leaves no room.
    fn zone_of_span(&self, start: f64, end: f64) -> Option<FaceZone> {
        self.clean
            .iter()
            .filter(|face| (start..=end).contains(&face.time))
            .map(zone_of)
            .reduce(|left, right| union(&left, &right))
    }

    /// A block's centre during `start..end`, or `None` when no face is known at all. It stays clear
    /// of every clean face in the span unless the faces leave no place in the frame at all.
    fn center(&mut self, start: f64, end: f64, block: (f64, f64)) -> Option<(f64, f64)> {
        let zone = self.zone_during(start, end)?;
        if !self.holds(&zone) {
            self.current = self.vertical_spot(&zone);
        }
        if let Some(spot) = self.current {
            let y = match spot {
                Spot::Above(bottom) => bottom - block.1 / 2.0,
                Spot::Below(top) => top + block.1 / 2.0,
            };
            return Some((self.frame_w / 2.0, y));
        }
        let narrow = self
            .zone_of_span(start, end)
            .and_then(|span| self.free_center(&span, block));
        Some(self.free_center(&zone, block).or(narrow).unwrap_or((
            self.frame_w / 2.0,
            self.frame_h - self.padding - block.1 / 2.0,
        )))
    }
}

fn clamp_center(center: f64, block: f64, frame: f64) -> f64 {
    let half = block / 2.0;
    if block >= frame {
        return frame / 2.0;
    }
    center.clamp(half, frame - half)
}

/// The centre that keeps a block inside the frame's safe zone; a block wider than the zone takes the
/// zone's middle.
fn inside_safe_zone(center: f64, width: f64, frame_w: f64, settings: &Settings) -> f64 {
    let left = frame_w * settings.safe_left_percent / PERCENT;
    let right = frame_w * (1.0 - settings.safe_right_percent / PERCENT);
    if width >= right - left {
        return (left + right) / 2.0;
    }
    center.clamp(left + width / 2.0, right - width / 2.0)
}

fn following_silence(group: &[Word], next: Option<&Vec<Word>>) -> f64 {
    let last_end = group.last().map_or(0.0, |word| word.end);
    next.and_then(|words| words.first())
        .map_or(f64::INFINITY, |word| word.start - last_end)
}

/// Consecutive words of one speaker: `words[first..end]`, sounding from `start` to `stop`.
struct Turn {
    first: usize,
    end: usize,
    speaker: Option<u32>,
    start: f64,
    stop: f64,
    /// Sounds at once with the turn beside it: real overlap, shown on rows of its own.
    together: bool,
}

fn turns_of(words: &[Word]) -> Vec<Turn> {
    let mut turns: Vec<Turn> = Vec::new();
    for (index, word) in words.iter().enumerate() {
        match turns.last_mut() {
            Some(turn) if turn.speaker == word.speaker => {
                turn.end = index + 1;
                turn.stop = turn.stop.max(word.end);
            }
            _ => turns.push(Turn {
                first: index,
                end: index + 1,
                speaker: word.speaker,
                start: word.start,
                stop: word.end,
                together: false,
            }),
        }
    }
    for index in 1..turns.len() {
        let (left, right) = turns.split_at_mut(index);
        let (before, after) = (&mut left[index - 1], &mut right[0]);
        let at_once: f64 = words[before.first..before.end]
            .iter()
            .flat_map(|a| words[after.first..after.end].iter().map(move |b| (a, b)))
            .map(|(a, b)| (a.end.min(b.end) - a.start.max(b.start)).max(0.0))
            .sum();
        if at_once >= SIMULTANEOUS_SECONDS {
            before.together = true;
            after.together = true;
        }
    }
    turns
}

/// The neighbour a brief turn belongs with: right beside it, the same speaker on both sides first,
/// then the closer one. A turn set apart by pauses is a caption of its own and keeps its speaker.
fn turn_to_join(turns: &[Turn], index: usize) -> Option<usize> {
    let turn = &turns[index];
    let beside = |other: usize, gap: f64| {
        (!turns[other].together && gap <= CUE_BREAK_PAUSE_SECONDS).then_some((other, gap))
    };
    let before = index
        .checked_sub(1)
        .and_then(|other| beside(other, turn.start - turns[other].stop));
    let after = turns
        .get(index + 1)
        .and_then(|next| beside(index + 1, next.start - turn.stop));
    match (before, after) {
        (Some((left, _)), Some((right, _))) if turns[left].speaker == turns[right].speaker => {
            Some(left)
        }
        (Some((left, gap_left)), Some((right, gap_right))) => {
            Some(if gap_left <= gap_right { left } else { right })
        }
        (one, other) => one.or(other).map(|(neighbour, _)| neighbour),
    }
}

fn merge_turn_into_neighbours(turns: &mut Vec<Turn>, index: usize, speaker: Option<u32>) {
    turns[index].speaker = speaker;
    if turns
        .get(index + 1)
        .is_some_and(|next| next.speaker == speaker)
    {
        let next = turns.remove(index + 1);
        turns[index].end = next.end;
        turns[index].stop = turns[index].stop.max(next.stop);
    }
    if index > 0 && turns[index - 1].speaker == speaker {
        let turn = turns.remove(index);
        let before = &mut turns[index - 1];
        before.end = turn.end;
        before.stop = before.stop.max(turn.stop);
    }
}

/// Moves a change of speaker inside a word to the end of that word, whichever side is shorter to move.
fn keep_words_whole(words: &mut [Word], turns: &[Turn], segmenter: &Segmenter) -> AppResult<()> {
    let text: String = words.iter().map(|word| word.text.as_str()).collect();
    let inside: HashSet<usize> = segmenter.boundaries(&text)?.morphemes.into_iter().collect();
    let mut offsets = Vec::with_capacity(words.len());
    let mut offset = 0;
    for word in words.iter() {
        offset += word.text.chars().count();
        offsets.push(offset);
    }
    for pair in turns
        .windows(2)
        .filter(|pair| !pair[0].together && !pair[1].together)
    {
        let change = pair[1].first;
        let word_start = (pair[0].first..change)
            .rev()
            .find(|index| *index == pair[0].first || inside.contains(&offsets[index - 1]))
            .unwrap_or(change);
        let word_end = (change..pair[1].end)
            .find(|index| inside.contains(&offsets[*index]))
            .map_or(pair[1].end, |index| index + 1);
        if inside.contains(&offsets[change - 1]) {
            continue;
        }
        let seconds = |range: std::ops::Range<usize>| -> f64 {
            words[range].iter().map(|word| word.end - word.start).sum()
        };
        let (range, speaker) = if seconds(word_start..change) <= seconds(change..word_end) {
            (word_start..change, pair[1].speaker)
        } else {
            (change..word_end, pair[0].speaker)
        };
        words[range]
            .iter_mut()
            .for_each(|word| word.speaker = speaker);
    }
    Ok(())
}

/// The words of `speaker` next to `from` in `order`, nearest first, as far as a sentence could matter.
fn own_words(
    words: &[Word],
    order: impl Iterator<Item = usize>,
    speaker: Option<u32>,
) -> Vec<usize> {
    order
        .filter(|index| words[*index].speaker == speaker)
        .take(PHRASE_CONTEXT_WORDS)
        .collect()
}

/// A change of speaker in the middle of a phrase or a run of nouns is the model's mistake: nobody takes over
/// at "1週間|前じゃん". Each voice's words are read on their own, so rows beside them cannot split the phrase,
/// and the shorter side up to the phrase's edge takes the longer side's speaker. Anywhere else stays: measured
/// against delivered captions, merging up to the end of a sentence loses 6.5 points of the right colour,
/// because people in conversation do take over there. `words` are in time order.
pub fn keep_phrases_whole(words: &mut [Word], segmenter: &Segmenter) -> AppResult<()> {
    let in_time_order: Vec<usize> = (0..words.len()).collect();
    let together = simultaneous_stretches(words, &in_time_order);
    let at_once = |moment: f64| {
        together
            .iter()
            .any(|(from, to)| (*from..=*to).contains(&moment))
    };
    for at in 1..words.len() {
        let (before, after) = (words[at - 1].speaker, words[at].speaker);
        let gap = words[at].start - words[at - 1].end;
        // Sounding at once is two voices, whatever the words say, and each one's row stays as it is.
        if before == after
            || !(0.0..=CUE_BREAK_PAUSE_SECONDS).contains(&gap)
            || at_once(words[at].start)
        {
            continue;
        }
        let mut left = own_words(words, (0..at).rev(), before);
        left.reverse();
        let right = own_words(words, at..words.len(), after);
        let text_of = |indexes: &[usize]| -> String {
            indexes
                .iter()
                .map(|index| words[*index].text.as_str())
                .collect()
        };
        let (left_text, right_text) = (text_of(&left), text_of(&right));
        let text = format!("{left_text}{right_text}");
        if !is_japanese(&text) {
            continue;
        }
        let edges: HashSet<usize> = segmenter
            .boundaries(&text)?
            .phrases
            .iter()
            .filter(|phrase| phrase.class != BreakClass::Compound)
            .map(|phrase| phrase.offset)
            .collect();
        let change = left_text.chars().count();
        if edges.contains(&change) {
            continue;
        }
        let mut offset = change;
        let left_part: Vec<usize> = left
            .iter()
            .rev()
            .copied()
            .take_while(|index| {
                offset -= words[*index].text.chars().count();
                !edges.contains(&(offset + words[*index].text.chars().count()))
                    || offset + words[*index].text.chars().count() == change
            })
            .collect();
        let mut reached = change;
        let right_part: Vec<usize> = right
            .iter()
            .copied()
            .take_while(|index| {
                let open = reached == change || !edges.contains(&reached);
                reached += words[*index].text.chars().count();
                open
            })
            .collect();
        let seconds = |indexes: &[usize]| -> f64 {
            indexes
                .iter()
                .map(|index| words[*index].end - words[*index].start)
                .sum()
        };
        let left_is_a_row = at_once(words[at - 1].start);
        let (moved, speaker) = if left_is_a_row || seconds(&left_part) >= seconds(&right_part) {
            (right_part, before)
        } else {
            (left_part, after)
        };
        moved
            .iter()
            .for_each(|index| words[*index].speaker = speaker);
    }
    Ok(())
}

/// Takes the flicker out of the diarizer's labels: a brief turn right beside other speech joins its
/// neighbour, shortest first, and no word is split between two speakers. Real overlap stays as it is.
pub fn smooth_speakers(words: &mut [Word], segmenter: &Segmenter) -> AppResult<()> {
    let mut turns = turns_of(words);
    if turns.len() < 2 {
        return Ok(());
    }
    loop {
        let shortest = (0..turns.len())
            .filter(|index| {
                !turns[*index].together
                    && turns[*index].stop - turns[*index].start < MIN_TURN_SECONDS
            })
            .filter_map(|index| turn_to_join(&turns, index).map(|neighbour| (index, neighbour)))
            .min_by(|left, right| {
                let length = |index: usize| turns[index].stop - turns[index].start;
                length(left.0).total_cmp(&length(right.0))
            });
        let Some((index, neighbour)) = shortest else {
            break;
        };
        let speaker = turns[neighbour].speaker;
        merge_turn_into_neighbours(&mut turns, index, speaker);
    }
    for turn in &turns {
        words[turn.first..turn.end]
            .iter_mut()
            .for_each(|word| word.speaker = turn.speaker);
    }
    keep_words_whole(words, &turns, segmenter)
}

/// One speaker's words without a real pause, as indexes into the words.
struct SpeakerSpan {
    speaker: Option<u32>,
    start: f64,
    end: f64,
    words: Vec<usize>,
}

fn speaker_spans(words: &[Word], in_time_order: &[usize]) -> Vec<SpeakerSpan> {
    let mut spans: Vec<SpeakerSpan> = Vec::new();
    for &index in in_time_order {
        let word = &words[index];
        match spans.iter_mut().rfind(|span| span.speaker == word.speaker) {
            Some(span) if word.start - span.end <= CUE_BREAK_PAUSE_SECONDS => {
                span.end = span.end.max(word.end);
                span.words.push(index);
            }
            _ => spans.push(SpeakerSpan {
                speaker: word.speaker,
                start: word.start,
                end: word.end,
                words: vec![index],
            }),
        }
    }
    spans
}

/// How long two spans' words really sound at once: quick turns interleave without coinciding.
fn seconds_together(left: &SpeakerSpan, right: &SpeakerSpan, words: &[Word]) -> f64 {
    let at_once = |(a, b): (&usize, &usize)| {
        (words[*a].end.min(words[*b].end) - words[*a].start.max(words[*b].start)).max(0.0)
    };
    left.words
        .iter()
        .flat_map(|a| right.words.iter().map(move |b| (a, b)))
        .map(at_once)
        .sum()
}

/// The stretches where two speakers talk at once, in time order.
fn simultaneous_stretches(words: &[Word], in_time_order: &[usize]) -> Vec<(f64, f64)> {
    let spans = speaker_spans(words, in_time_order);
    let mut found: Vec<(f64, f64)> = spans
        .iter()
        .enumerate()
        .flat_map(|(at, left)| spans[at + 1..].iter().map(move |right| (left, right)))
        .filter(|(left, right)| {
            left.speaker != right.speaker
                && seconds_together(left, right, words) >= SIMULTANEOUS_SECONDS
        })
        .map(|(left, right)| (left.start.min(right.start), left.end.max(right.end)))
        .collect();
    found.sort_by(|left, right| left.0.total_cmp(&right.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (start, end) in found {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

/// Words of speakers talking at once, one speaker after the other so each reads as its own captions;
/// `None` when nobody does, which leaves every word where it is.
fn speakers_apart(words: &[Word]) -> Option<Vec<Word>> {
    let mut in_time_order: Vec<usize> = (0..words.len()).collect();
    in_time_order.sort_by(|left, right| words[*left].start.total_cmp(&words[*right].start));
    let stretches = simultaneous_stretches(words, &in_time_order);
    if stretches.is_empty() {
        return None;
    }
    let mut apart = Vec::with_capacity(words.len());
    let mut rest = in_time_order.as_slice();
    for (start, end) in stretches {
        let before = rest.partition_point(|index| words[*index].start < start);
        apart.extend(rest[..before].iter().map(|index| words[*index].clone()));
        rest = &rest[before..];
        let inside = rest.partition_point(|index| words[*index].start <= end);
        let mut speakers: Vec<Option<u32>> = Vec::new();
        for index in &rest[..inside] {
            if !speakers.contains(&words[*index].speaker) {
                speakers.push(words[*index].speaker);
            }
        }
        for speaker in speakers {
            let own = rest[..inside]
                .iter()
                .filter(|index| words[**index].speaker == speaker);
            apart.extend(own.map(|index| words[*index].clone()));
        }
        rest = &rest[inside..];
    }
    apart.extend(rest.iter().map(|index| words[*index].clone()));
    Some(apart)
}

fn pages_together(left: &Page, right: &Page) -> bool {
    left.speaker != right.speaker
        && left.end.min(right.end) - left.start.max(right.start) >= SIMULTANEOUS_SECONDS
}

/// Gives captions shown together one row per speaker, in speaker order from the anchor outwards. The
/// rows keep to the frame's margins, shrinking only when they cannot fit there at full size.
fn stack_rows(
    cues: &mut [Cue],
    grows_down: bool,
    info: &VideoInfo,
    metrics: &Metrics,
    settings: &Settings,
) {
    let frame_h = f64::from(info.height);
    let short_edge = f64::from(info.width.min(info.height)) / REFERENCE_SHORT_EDGE;
    let margin = MARGIN_PX * short_edge;
    // `row_spacing` runs from one row's middle to the next; the rows' heights already hold the line box.
    let gap = (settings.row_spacing - EM_LINE_HEIGHT) * metrics.font_px * metrics.rendered_scale;
    let height_of = |cue: &Cue| block_height(cue.lines.len(), metrics, settings);
    let mut speakers: Vec<Option<u32>> = cues.iter().map(|cue| cue.speaker).collect();
    speakers.sort_unstable();
    speakers.dedup();
    let rows: Vec<f64> = speakers
        .iter()
        .map(|speaker| {
            cues.iter()
                .filter(|cue| cue.speaker == *speaker)
                .map(height_of)
                .fold(0.0, f64::max)
        })
        .collect();
    let total = rows.iter().sum::<f64>() + gap * rows.len().saturating_sub(1) as f64;
    let scale = ((frame_h - 2.0 * margin) / total).min(1.0);
    let stacked = total * scale;
    let edges = cues.iter().map(|cue| {
        let half = height_of(cue) / 2.0;
        if grows_down {
            cue.y - half
        } else {
            cue.y + half
        }
    });
    let edge = if grows_down {
        edges
            .fold(f64::NEG_INFINITY, f64::max)
            .clamp(0.0, frame_h - margin - stacked)
    } else {
        edges
            .fold(f64::INFINITY, f64::min)
            .clamp(margin + stacked, frame_h)
    };
    let mut reach = 0.0;
    for (speaker, row) in speakers.iter().zip(&rows) {
        for cue in cues.iter_mut().filter(|cue| cue.speaker == *speaker) {
            let half = block_height(cue.lines.len(), metrics, settings) * scale / 2.0;
            cue.y = if grows_down {
                edge + reach + half
            } else {
                edge - reach - half
            };
            cue.scale = scale;
        }
        reach += (row + gap) * scale;
    }
}

/// Stacks each run of captions that are on screen at the same time; a caption alone stays as placed.
fn stack_simultaneous(
    cues: &mut [Cue],
    grows_down: &[bool],
    info: &VideoInfo,
    metrics: &Metrics,
    settings: &Settings,
) {
    let mut first = 0;
    while first < cues.len() {
        let mut last = first + 1;
        let mut until = cues[first].end;
        while last < cues.len() && cues[last].start < until {
            until = until.max(cues[last].end);
            last += 1;
        }
        if last - first > 1 {
            stack_rows(
                &mut cues[first..last],
                grows_down[first],
                info,
                metrics,
                settings,
            );
        }
        first = last;
    }
}

/// With several speakers, every caption starts at the same place: the left edge of the widest one.
fn share_left_edge(cues: &mut [Cue], metrics: &Metrics, settings: &Settings) {
    let one_speaker = cues
        .windows(2)
        .all(|pair| pair[0].speaker == pair[1].speaker);
    if !settings.rows_share_left || one_speaker {
        return;
    }
    let half_width = |cue: &Cue| block_width(&cue.lines, metrics, settings) * cue.scale / 2.0;
    let left = cues
        .iter()
        .map(|cue| cue.x - half_width(cue))
        .fold(f64::INFINITY, f64::min);
    cues.iter_mut()
        .for_each(|cue| cue.x = left + half_width(cue));
}

/// A page timed into silence starts when the voice does. Only later, never earlier: a page whose
/// start lies in a voice may be mid-sentence, where the voice's own start says nothing about it.
fn start_with_the_voice(pages: &mut [Page], voice: &[VoiceSpan]) {
    pages.iter_mut().for_each(|page| {
        let next = voice.partition_point(|span| span.end < page.start);
        let onset = voice.get(next).map_or(f64::INFINITY, |span| span.start);
        if onset > page.start && onset - page.start <= VOICE_ONSET_REACH_SECONDS && onset < page.end
        {
            page.start = onset;
        }
    });
}

/// Builds positioned cues from timed words. The returned flag reports a face anchor that found no
/// face; words stay as they are, so a transcript can be laid out again.
pub fn build_cues(
    words: &[Word],
    faces: &[FaceSample],
    voice: &[VoiceSpan],
    info: &VideoInfo,
    metrics: &Metrics,
    settings: &Settings,
    segmenter: &Segmenter,
) -> AppResult<(Vec<Cue>, bool)> {
    let placement = settings.placement;
    let (frame_w, frame_h) = (f64::from(info.width), f64::from(info.height));
    let margin = MARGIN_PX * f64::from(info.width.min(info.height)) / REFERENCE_SHORT_EDGE;
    let wants_face = matches!(placement.anchor, Anchor::FaceAbove | Anchor::FaceBelow);
    let mut placer = FacePlacer::new(faces, info, metrics, settings);
    let rate = f64::from(info.fps_num) / f64::from(info.fps_den);
    let mut shown = display_words(words, info.duration);
    // In time order, so the end of a row and the words after it read as the one phrase they are.
    shown.sort_by(|left, right| left.start.total_cmp(&right.start));
    keep_phrases_whole(&mut shown, segmenter)?;
    // Voices at once first: their words, one after the other, must not read as flickering labels.
    let apart = speakers_apart(&shown);
    let talk_at_once = apart.is_some();
    if let Some(words) = apart {
        shown = words;
    }
    smooth_speakers(&mut shown, segmenter)?;
    close_split_morphemes(&mut shown, segmenter)?;
    let groups = rejoin_split_words(merge_strays(group_words(&shown)), segmenter)?;
    let mut pages: Vec<Page> = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        let silence = following_silence(group, groups.get(index + 1));
        // The next speaker may already be talking: that says nothing about this page's own end.
        let following_gap = if talk_at_once && silence < 0.0 {
            f64::INFINITY
        } else {
            silence
        };
        pages.extend(Run::new(group, following_gap, metrics, segmenter)?.pages());
    }
    if talk_at_once {
        pages.sort_by(|left, right| left.start.total_cmp(&right.start));
    }
    start_with_the_voice(&mut pages, voice);
    let mut cues: Vec<Cue> = Vec::with_capacity(pages.len());
    let mut grows_down: Vec<bool> = Vec::with_capacity(pages.len());
    let mut faceless = false;
    for (index, page) in pages.iter().enumerate() {
        // Another speaker talking at the same time neither cuts this caption short nor delays it.
        let following = pages[index + 1..]
            .iter()
            .find(|next| !pages_together(page, next))
            .map_or(info.duration, |next| next.start);
        let previous_end = pages
            .iter()
            .zip(&cues)
            .filter(|(earlier, _)| !pages_together(earlier, page))
            .map(|(_, cue)| cue.end)
            .fold(0.0, f64::max);
        // A caption's minimum time comes first: the next one may start a moment late, never early.
        let shortest = page.start.max(previous_end) + MIN_DISPLAY;
        let end = held_end(page, following, rate).max(shortest.min(info.duration));
        let (start, end) = on_frames(page.start, end, previous_end, rate);
        let size = |lines: &[String]| {
            (
                block_width(lines, metrics, settings),
                block_height(lines.len(), metrics, settings),
            )
        };
        let rewrap = wants_face
            && !placer.has_room(start, end, size(&page.lines))
            && placer.has_room(start, end, size(&page.narrow));
        let lines = if rewrap { &page.narrow } else { &page.lines }.clone();
        let height = block_height(lines.len(), metrics, settings);
        let width = block_width(&lines, metrics, settings);
        let at_face = wants_face
            .then(|| placer.center(start, end, (width, height)))
            .flatten();
        faceless |= wants_face && at_face.is_none();
        let (center_x, center_y) = at_face.unwrap_or_else(|| {
            let center_x = frame_w / 2.0;
            if placement.anchor == Anchor::VideoTop {
                (center_x, margin + height / 2.0)
            } else {
                (center_x, frame_h - margin - height / 2.0)
            }
        });
        let x = inside_safe_zone(
            center_x + placement.offset_x_percent / PERCENT * frame_w,
            width,
            frame_w,
            settings,
        );
        let y = clamp_center(
            center_y + placement.offset_y_percent / 100.0 * frame_h,
            height,
            frame_h,
        );
        grows_down.push(if at_face.is_some() {
            matches!(placer.current, Some(Spot::Below(_)))
        } else {
            placement.anchor == Anchor::VideoTop
        });
        cues.push(Cue {
            start,
            end,
            speaker: page.speaker,
            lines,
            x,
            y,
            scale: 1.0,
        });
    }
    stack_simultaneous(&mut cues, &grows_down, info, metrics, settings);
    share_left_edge(&mut cues, metrics, settings);
    Ok((cues, faceless))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(start: f64, end: f64, speaker: u32) -> SpeakerTurn {
        SpeakerTurn {
            start,
            end,
            speaker,
        }
    }

    fn steady_numbers(turns: &[SpeakerTurn]) -> Vec<u32> {
        steadied_turns(turns)
            .iter()
            .map(|turn| turn.speaker)
            .collect()
    }

    fn said(text: &str, start: f64, end: f64, speaker: u32) -> Word {
        Word {
            text: text.to_owned(),
            start,
            end,
            speaker: Some(speaker),
        }
    }

    fn speakers_of(words: &[Word]) -> Vec<Option<u32>> {
        words.iter().map(|word| word.speaker).collect()
    }

    #[test]
    fn a_run_of_nouns_cut_between_two_speakers_goes_to_the_one_who_says_more_of_it() {
        let mut words = vec![
            said("先週", 0.0, 1.0, 2),
            said("は", 1.0, 1.2, 2),
            said("1週間", 1.2, 2.5, 2),
            said("前", 2.8, 3.1, 1),
            said("じゃん", 3.1, 3.5, 1),
        ];
        keep_phrases_whole(&mut words, test_segmenter()).unwrap();
        assert_eq!(
            speakers_of(&words),
            vec![Some(2), Some(2), Some(2), Some(2), Some(2)]
        );
    }

    #[test]
    fn words_right_after_a_row_join_it_and_the_row_stays_as_it_is() {
        let mut words = vec![
            said("今日", 0.0, 2.0, 1),
            said("先週", 0.5, 1.5, 2),
            said("1週間", 1.5, 2.5, 2),
            said("前じゃん", 2.8, 5.0, 3),
        ];
        keep_phrases_whole(&mut words, test_segmenter()).unwrap();
        assert_eq!(
            speakers_of(&words),
            vec![Some(1), Some(2), Some(2), Some(2)]
        );
    }

    #[test]
    fn voices_at_once_stay_apart_even_inside_a_run_of_nouns() {
        let mut words = vec![said("1週間", 0.0, 2.0, 1), said("前じゃん", 1.5, 3.0, 2)];
        keep_phrases_whole(&mut words, test_segmenter()).unwrap();
        assert_eq!(speakers_of(&words), vec![Some(1), Some(2)]);
    }

    #[test]
    fn a_new_person_may_start_where_a_sentence_ends() {
        let mut words = vec![
            said("行きました。", 0.0, 2.0, 1),
            said("本当ですか", 2.1, 4.0, 2),
        ];
        keep_phrases_whole(&mut words, test_segmenter()).unwrap();
        assert_eq!(speakers_of(&words), vec![Some(1), Some(2)]);
    }

    #[test]
    fn a_long_pause_lets_the_speaker_change_even_inside_a_sentence() {
        let mut words = vec![said("1週間", 0.0, 2.0, 1), said("前じゃん", 3.0, 5.0, 2)];
        keep_phrases_whole(&mut words, test_segmenter()).unwrap();
        assert_eq!(speakers_of(&words), vec![Some(1), Some(2)]);
    }

    #[test]
    fn other_languages_keep_the_speakers_as_they_are() {
        let mut words = vec![said("one ", 0.0, 2.0, 1), said("week", 2.0, 4.0, 2)];
        keep_phrases_whole(&mut words, test_segmenter()).unwrap();
        assert_eq!(speakers_of(&words), vec![Some(1), Some(2)]);
    }

    #[test]
    fn a_short_turn_inside_someone_elses_speech_takes_their_number() {
        assert_eq!(
            steady_numbers(&[turn(0.0, 2.0, 1), turn(2.0, 2.4, 2), turn(2.4, 5.0, 1)]),
            vec![1, 1, 1]
        );
    }

    #[test]
    fn a_long_turn_keeps_its_number_among_short_ones() {
        assert_eq!(
            steady_numbers(&[turn(0.0, 0.5, 1), turn(0.5, 5.0, 2), turn(5.0, 5.5, 3)]),
            vec![2, 2, 2]
        );
    }

    #[test]
    fn voices_at_once_for_a_second_stay_apart() {
        assert_eq!(
            steady_numbers(&[turn(0.0, 3.0, 1), turn(1.0, 2.5, 2)]),
            vec![1, 2]
        );
    }

    #[test]
    fn a_tie_keeps_a_turns_own_number() {
        assert_eq!(
            steady_numbers(&[turn(0.0, 1.0, 1), turn(1.0, 2.0, 2)]),
            vec![1, 2]
        );
    }

    #[test]
    fn turns_come_back_in_time_order() {
        assert_eq!(
            steadied_turns(&[turn(10.0, 11.0, 2), turn(0.0, 1.0, 1)])[0].start,
            0.0
        );
        assert_eq!(steadied_turns(&[]).len(), 0);
    }

    fn placed(speaker: u32, line: &str, x: f64) -> Cue {
        Cue {
            start: 0.0,
            end: 1.0,
            speaker: Some(speaker),
            lines: vec![line.to_owned()],
            x,
            y: 0.0,
            scale: 1.0,
        }
    }

    const ROW_METRICS: Metrics = Metrics {
        font_px: 10.0,
        outline_px: 1.0,
        max_line_px: 1000.0,
        glyphs: None,
        rendered_scale: 1.0,
    };

    #[test]
    fn the_default_rows_sit_closer_than_the_earlier_ones_and_follow_the_setting() {
        let row_gap = |row_spacing: f64| {
            let settings = Settings {
                row_spacing,
                ..fcpxml_settings()
            };
            let metrics = metrics_for(&FULL_HD, &settings);
            let mut cues = [placed(1, "a", 960.0), placed(2, "b", 960.0)];
            cues[0].y = 900.0;
            stack_rows(&mut cues, false, &FULL_HD, &metrics, &settings);
            ((cues[0].y - cues[1].y) / metrics.font_px / metrics.rendered_scale * 100.0).round()
        };
        assert_eq!(row_gap(Settings::default().row_spacing), 145.0);
        assert_eq!(row_gap(1.0), 100.0);
        assert_eq!(row_gap(2.5), 250.0);
    }

    #[test]
    fn several_speakers_start_at_the_left_edge_of_the_widest_caption() {
        let settings = Settings::default();
        let mut cues = [placed(1, "aaaaaaaa", 100.0), placed(2, "aa", 100.0)];
        share_left_edge(&mut cues, &ROW_METRICS, &settings);
        let edge = |cue: &Cue| cue.x - block_width(&cue.lines, &ROW_METRICS, &settings) / 2.0;
        assert_eq!(edge(&cues[0]), edge(&cues[1]));
        assert_eq!(cues[0].x, 100.0);
        assert!(cues[1].x < 100.0);
    }

    #[test]
    fn one_speaker_or_the_setting_off_leaves_captions_centred() {
        let mut one = [placed(1, "aaaa", 100.0), placed(1, "a", 100.0)];
        share_left_edge(&mut one, &ROW_METRICS, &Settings::default());
        assert_eq!((one[0].x, one[1].x), (100.0, 100.0));
        let off = Settings {
            rows_share_left: false,
            ..Settings::default()
        };
        let mut two = [placed(1, "aaaa", 100.0), placed(2, "a", 100.0)];
        share_left_edge(&mut two, &ROW_METRICS, &off);
        assert_eq!((two[0].x, two[1].x), (100.0, 100.0));
    }

    /// The layout expectations below were calibrated on FCPXML, which scales titles and strokes itself.
    fn fcpxml_settings() -> Settings {
        Settings {
            subtitle_format: SubtitleFormat::Fcpxml,
            ..crate::settings::earlier_look()
        }
    }

    /// Lays out with estimated widths, so what these tests pin holds whichever fonts a computer has.
    fn build_cues(
        words: &[Word],
        faces: &[FaceSample],
        info: &VideoInfo,
        settings: &Settings,
        segmenter: &Segmenter,
    ) -> AppResult<(Vec<Cue>, bool)> {
        super::build_cues(
            words,
            faces,
            &[],
            info,
            &metrics_for(info, settings),
            settings,
            segmenter,
        )
    }
    use crate::morphology::test_segmenter;
    use crate::settings::Placement;
    use std::path::PathBuf;

    fn word(start: f64, end: f64, text: &str) -> Word {
        Word {
            start,
            end,
            text: text.to_owned(),
            speaker: None,
        }
    }

    fn spoken(start: f64, end: f64, text: &str, speaker: u32) -> Word {
        Word {
            start,
            end,
            text: text.to_owned(),
            speaker: Some(speaker),
        }
    }

    const FULL_HD: VideoInfo = VideoInfo {
        width: 1920,
        height: 1080,
        duration: 60.0,
        fps_num: 30,
        fps_den: 1,
        audio: None,
    };
    const VERTICAL: VideoInfo = VideoInfo {
        width: 1080,
        height: 1920,
        duration: 60.0,
        fps_num: 30,
        fps_den: 1,
        audio: None,
    };
    const VERTICAL_SMALL: VideoInfo = VideoInfo {
        width: 720,
        height: 1280,
        duration: 60.0,
        fps_num: 30,
        fps_den: 1,
        audio: None,
    };
    const UHD: VideoInfo = VideoInfo {
        width: 3840,
        height: 2160,
        duration: 60.0,
        fps_num: 30,
        fps_den: 1,
        audio: None,
    };

    fn at(anchor: Anchor, x: f64, y: f64) -> Settings {
        Settings {
            placement: Placement {
                anchor,
                offset_x_percent: x,
                offset_y_percent: y,
            },
            ..fcpxml_settings()
        }
    }

    #[test]
    fn caption_size_and_line_width_follow_the_resolution() {
        let settings = fcpxml_settings();
        assert_eq!(metrics_for(&FULL_HD, &settings).font_px, 52.0);
        assert_eq!(metrics_for(&UHD, &settings).font_px, 104.0);
        assert_eq!(metrics_for(&VERTICAL, &settings).font_px, 52.0);
        assert_eq!(metrics_for(&FULL_HD, &settings).max_line_px.round(), 1705.0);
        assert_eq!(metrics_for(&UHD, &settings).max_line_px.round(), 3433.0);
        assert_eq!(metrics_for(&VERTICAL, &settings).max_line_px.round(), 949.0);
        assert_eq!(
            metrics_for(&VERTICAL_SMALL, &settings).max_line_px.round(),
            625.0
        );
    }

    #[test]
    fn dominant_voice_wins() {
        let mut words = [word(0.0, 1.0, "は")];
        assign_speakers(
            &mut words,
            &[SpeakerTurn {
                start: 0.0,
                end: 5.0,
                speaker: 2,
            }],
        );
        assert_eq!(words[0].speaker, Some(2));
    }

    #[test]
    fn a_tie_with_nobody_before_goes_to_the_first_voice() {
        let mut words = [word(0.0, 1.0, "は")];
        assign_speakers(
            &mut words,
            &[
                SpeakerTurn {
                    start: 0.0,
                    end: 0.5,
                    speaker: 1,
                },
                SpeakerTurn {
                    start: 0.5,
                    end: 1.0,
                    speaker: 2,
                },
            ],
        );
        assert_eq!(words[0].speaker, Some(1));
    }

    #[test]
    fn a_word_in_a_small_silent_gap_takes_the_nearest_speaker() {
        let mut words = [word(5.2, 5.3, "ね")];
        assign_speakers(
            &mut words,
            &[SpeakerTurn {
                start: 0.0,
                end: 5.0,
                speaker: 3,
            }],
        );
        assert_eq!(words[0].speaker, Some(3));
    }

    #[test]
    fn a_word_far_from_any_speech_takes_the_nearest_speaker() {
        let mut words = [word(9.0, 9.1, "ね")];
        assign_speakers(
            &mut words,
            &[SpeakerTurn {
                start: 0.0,
                end: 5.0,
                speaker: 3,
            }],
        );
        assert_eq!(words[0].speaker, Some(3));
    }

    #[test]
    fn quiet_words_between_one_speakers_words_take_that_speaker() {
        let mut words = [
            word(0.0, 1.0, "あ"),
            word(5.0, 5.1, "い"),
            word(9.0, 10.0, "う"),
        ];
        assign_speakers(
            &mut words,
            &[
                SpeakerTurn {
                    start: 0.0,
                    end: 1.0,
                    speaker: 1,
                },
                SpeakerTurn {
                    start: 9.0,
                    end: 10.0,
                    speaker: 1,
                },
            ],
        );
        assert_eq!(words[1].speaker, Some(1));
    }

    #[test]
    fn quiet_words_between_two_speakers_take_the_nearer_one() {
        let mut words = [
            word(0.0, 1.0, "あ"),
            word(5.0, 5.1, "い"),
            word(9.0, 10.0, "う"),
        ];
        assign_speakers(
            &mut words,
            &[
                SpeakerTurn {
                    start: 0.0,
                    end: 1.0,
                    speaker: 1,
                },
                SpeakerTurn {
                    start: 9.0,
                    end: 10.0,
                    speaker: 2,
                },
            ],
        );
        assert_eq!(words[1].speaker, Some(2));
    }

    #[test]
    fn a_tie_goes_to_the_speaker_of_the_word_before() {
        let mut words = [
            word(0.0, 1.0, "あ"),
            word(2.0, 3.0, "い"),
            word(4.0, 5.0, "う"),
        ];
        assign_speakers(
            &mut words,
            &[
                SpeakerTurn {
                    start: 0.0,
                    end: 2.5,
                    speaker: 1,
                },
                SpeakerTurn {
                    start: 2.5,
                    end: 3.0,
                    speaker: 2,
                },
                SpeakerTurn {
                    start: 4.0,
                    end: 5.0,
                    speaker: 1,
                },
            ],
        );
        assert_eq!(words[1].speaker, Some(1));
    }

    #[test]
    fn speaker_changes_start_a_new_cue() {
        let words = [spoken(0.0, 1.0, "はい", 1), spoken(1.1, 2.2, "いいえ", 2)];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].speaker, Some(1));
        assert_eq!(cues[1].speaker, Some(2));
    }

    #[test]
    fn pauses_and_sentence_ends_split_cues() {
        let words = [
            spoken(0.0, 1.0, "はい", 1),
            spoken(4.5, 5.5, "そう。", 1),
            spoken(5.6, 6.5, "次", 1),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(
            cues.iter()
                .map(|cue| cue.lines.join(""))
                .collect::<Vec<_>>(),
            vec!["はい", "そう。", "次"]
        );
    }

    #[test]
    fn nothing_stays_on_screen_through_a_silence() {
        let words = [
            spoken(0.0, 2.0, "今日は公園へ行きました", 1),
            spoken(6.0, 8.0, "楽しかったです", 1),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(cues[0].end, 2.12);
        assert_eq!(cues[1].start, 6.0);
    }

    #[test]
    fn a_short_cue_grows_only_after_its_speech() {
        let words = [spoken(1.0, 1.2, "はい", 1)];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(cues[0].start, 1.0);
        assert_eq!((cues[0].end * 1000.0).round(), 1833.0);
    }

    #[test]
    fn a_gap_inside_a_word_keeps_the_word_together() {
        let words = [
            spoken(0.0, 1.0, "うーん頑", 1),
            spoken(1.8, 3.0, "張るよ", 1),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(cues[0].lines, vec!["うーん頑張るよ"]);
    }

    #[test]
    fn words_without_time_still_break_at_the_line_width() {
        let metrics = Metrics {
            font_px: 10.0,
            outline_px: 1.0,
            max_line_px: 130.0,
            glyphs: None,
            rendered_scale: 1.0,
        };
        let group = [spoken(
            1.0,
            1.0,
            "ちょっとレベルアップしたほうがいいのかなっていうふうに",
            1,
        )];
        let run = Run::new(&group, f64::INFINITY, &metrics, test_segmenter()).unwrap();
        let pages = run.pages();
        assert!(pages.len() > 1);
        assert!(pages
            .iter()
            .flat_map(|page| page.lines.iter())
            .all(|line| line.chars().count() <= 13));
    }

    #[test]
    fn a_short_cue_gets_its_minimum_time_before_a_late_next_cue() {
        let words = [
            spoken(0.0, 0.3, "はい。", 1),
            spoken(0.81, 2.0, "いいえ", 2),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(
            cues.iter()
                .map(|cue| ((cue.start * 1000.0).round(), (cue.end * 1000.0).round()))
                .collect::<Vec<_>>(),
            vec![(0.0, 833.0), (833.0, 2120.0)]
        );
    }

    #[test]
    fn the_minimum_time_stops_at_the_end_of_the_video() {
        let words = [spoken(59.8, 59.9, "はい", 1)];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(cues[0].end, 60.0);
    }

    #[test]
    fn a_long_silence_always_ends_the_caption_whatever_the_next_word_is() {
        let words = [
            spoken(0.0, 1.0, "言っちゃった", 1),
            spoken(4.0, 5.0, "でもねぇまぁ", 1),
        ];
        let (cues, _) = build_cues(
            &words,
            &[],
            &VERTICAL_SMALL,
            &fcpxml_settings(),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(
            cues.iter()
                .map(|cue| (cue.lines.join(""), cue.start, (cue.end * 100.0).round()))
                .collect::<Vec<_>>(),
            vec![
                ("言っちゃった".to_owned(), 0.0, 112.0),
                ("でもねぇまぁ".to_owned(), 4.0, 512.0)
            ]
        );
    }

    #[test]
    fn a_particle_after_a_long_silence_is_not_pulled_back() {
        let words = [
            spoken(0.0, 1.0, "今日は雨", 1),
            spoken(3.0, 4.0, "ですね", 1),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(cues.len(), 2);
    }

    #[test]
    fn a_word_stretched_over_a_silence_starts_when_it_is_said() {
        let words = [
            spoken(0.0, 0.5, "それで", 1),
            spoken(1.0, 3.0, "明", 1),
            spoken(3.0, 3.5, "日", 1),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(
            cues.iter()
                .map(|cue| (cue.lines.join(""), cue.start))
                .collect::<Vec<_>>(),
            vec![("それで".to_owned(), 0.0), ("明日".to_owned(), 2.75)]
        );
    }

    #[test]
    fn a_pause_between_two_words_stays_where_it_is() {
        let mut words = [spoken(1.0, 1.5, "公園", 1), spoken(5.0, 5.5, "明日", 1)];
        close_split_morphemes(&mut words, test_segmenter()).unwrap();
        assert_eq!(words[0].start, 1.0);
    }

    #[test]
    fn closing_up_a_word_keeps_to_one_speaker_and_a_plausible_stretch() {
        let mut across = [spoken(1.0, 1.2, "明", 1), spoken(2.5, 3.0, "日", 2)];
        close_split_morphemes(&mut across, test_segmenter()).unwrap();
        assert_eq!(across[0].start, 1.0);
        let mut far = [spoken(1.0, 1.2, "明", 1), spoken(9.0, 9.5, "日", 1)];
        close_split_morphemes(&mut far, test_segmenter()).unwrap();
        assert_eq!(far[0].start, 1.0);
    }

    #[test]
    fn nothing_to_close_is_fine() {
        close_split_morphemes(&mut [], test_segmenter()).unwrap();
    }

    #[test]
    fn a_wide_line_splits_at_a_good_phrase_end_but_stays_whole_without_one() {
        let metrics = Metrics {
            font_px: 10.0,
            outline_px: 1.0,
            max_line_px: 200.0,
            glyphs: None,
            rendered_scale: 1.0,
        };
        let group = [spoken(0.0, 6.0, "今日は天気がいいので、みんなで公園へ", 1)];
        let run = Run::new(&group, f64::INFINITY, &metrics, test_segmenter()).unwrap();
        assert_eq!(run.lay_out(0, 18).unwrap().split, Some(11));
        let noun = [spoken(0.0, 6.0, "サンフランシスコサンフランシスコ", 1)];
        let run = Run::new(&noun, f64::INFINITY, &metrics, test_segmenter()).unwrap();
        assert_eq!(run.lay_out(0, 16).unwrap().split, None);
    }

    #[test]
    fn using_more_of_the_width_costs_more() {
        let metrics = Metrics {
            font_px: 10.0,
            outline_px: 1.0,
            max_line_px: 1000.0,
            glyphs: None,
            rendered_scale: 1.0,
        };
        let short = [spoken(0.0, 3.0, "今日は公園へ", 1)];
        let long = [spoken(0.0, 3.0, "今日はみんなで公園に行きました", 1)];
        let short = Run::new(&short, f64::INFINITY, &metrics, test_segmenter()).unwrap();
        let long = Run::new(&long, f64::INFINITY, &metrics, test_segmenter()).unwrap();
        assert!(long.page_cost(0, 15) > short.page_cost(0, 6));
    }

    #[test]
    fn punctuation_stays_with_its_sentence() {
        let words = [spoken(0.0, 1.0, "はい", 1), spoken(5.0, 5.1, "。", 2)];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(cues[0].lines, vec!["はい。"]);
    }

    #[test]
    fn video_bottom_and_top_sit_inside_the_margin() {
        let words = [spoken(0.0, 1.0, "はい", 1)];
        let (bottom, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &at(Anchor::VideoBottom, 0.0, 0.0),
            test_segmenter(),
        )
        .unwrap();
        let (top, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &at(Anchor::VideoTop, 0.0, 0.0),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!((bottom[0].x, bottom[0].y.round()), (960.0, 914.0));
        assert_eq!((top[0].x, top[0].y.round()), (960.0, 166.0));
    }

    #[test]
    fn margins_scale_with_the_short_edge_on_every_frame() {
        let words = [spoken(0.0, 1.0, "はい", 1)];
        let settings = at(Anchor::VideoBottom, 0.0, 0.0);
        let (tall, _) =
            build_cues(&words, &[], &VERTICAL_SMALL, &settings, test_segmenter()).unwrap();
        let (large, _) = build_cues(&words, &[], &VERTICAL, &settings, test_segmenter()).unwrap();
        let (uhd, _) = build_cues(&words, &[], &UHD, &settings, test_segmenter()).unwrap();
        assert_eq!((tall[0].x, tall[0].y.round()), (360.0, 1163.0));
        assert_eq!((large[0].x, large[0].y.round()), (540.0, 1719.0));
        assert_eq!((uhd[0].x, uhd[0].y.round()), (1920.0, 1737.0));
    }

    #[test]
    fn a_second_line_grows_by_the_line_height_setting() {
        let metrics = metrics_for(&FULL_HD, &fcpxml_settings());
        assert_eq!(block_height(1, &metrics, &fcpxml_settings()).round(), 92.0);
        assert_eq!(block_height(2, &metrics, &fcpxml_settings()).round(), 165.0);
        let settings = Settings {
            line_height: 2.0,
            ..fcpxml_settings()
        };
        assert_eq!(block_height(2, &metrics, &settings).round(), 193.0);
    }

    #[test]
    fn a_block_is_as_wide_as_its_longest_line_with_the_outline_and_rim() {
        let metrics = Metrics {
            font_px: 10.0,
            outline_px: 6.0,
            max_line_px: 500.0,
            glyphs: None,
            rendered_scale: 1.0,
        };
        let settings = fcpxml_settings();
        let width = block_width(&["あい".to_owned(), "a".to_owned()], &metrics, &settings);
        assert_eq!((width * 100.0).round(), 14000.0);
    }

    #[test]
    fn offsets_move_captions_in_percent_of_the_frame() {
        let words = [spoken(0.0, 1.0, "はい", 1)];
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &at(Anchor::VideoBottom, 10.0, -10.0),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!((cues[0].x, cues[0].y.round()), (1152.0, 806.0));
    }

    #[test]
    fn offsets_cannot_push_captions_off_the_frame() {
        let words = [spoken(0.0, 1.0, "はい", 1)];
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &at(Anchor::VideoBottom, 50.0, 50.0),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(cues[0].y.round(), 1034.0);
        assert!(cues[0].x < 1920.0);
    }

    #[test]
    fn captions_sit_above_the_estimated_head_or_below_the_face() {
        let words = [spoken(0.0, 1.0, "はい", 1)];
        // A small face low in the frame: head top 600 - 1.25 * 100 = 475.
        let faces = [FaceSample {
            time: 0.5,
            top: 600.0,
            bottom: 700.0,
            center_x: 700.0,
        }];
        let (above, _) = build_cues(
            &words,
            &faces,
            &FULL_HD,
            &at(Anchor::FaceAbove, 0.0, 0.0),
            test_segmenter(),
        )
        .unwrap();
        let (below, _) = build_cues(
            &words,
            &faces,
            &FULL_HD,
            &at(Anchor::FaceBelow, 0.0, 2.0),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!((above[0].x, above[0].y.round()), (960.0, 369.0));
        assert_eq!((below[0].x, below[0].y.round()), (960.0, 827.0));
    }

    #[test]
    fn a_missing_face_falls_back_to_the_bottom_and_reports_it() {
        let words = [spoken(0.0, 1.0, "はい", 1)];
        let (cues, faceless) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &at(Anchor::FaceAbove, 0.0, 0.0),
            test_segmenter(),
        )
        .unwrap();
        assert!(faceless);
        assert_eq!(cues[0].y.round(), 914.0);
    }

    #[test]
    fn between_shots_a_caption_clears_the_faces_on_both_sides() {
        let sample = |time: f64, top: f64, bottom: f64| FaceSample {
            time,
            top,
            bottom,
            center_x: 960.0,
        };
        let placer = FacePlacer::new(
            &[sample(0.0, 100.0, 200.0), sample(10.0, 500.0, 600.0)],
            &FULL_HD,
            &metrics_for(&FULL_HD, &fcpxml_settings()),
            &fcpxml_settings(),
        );
        let zone = placer.zone_during(4.0, 5.0).unwrap();
        assert_eq!((zone.top, zone.bottom), (100.0, 600.0));
        assert_eq!(placer.zone_during(-5.0, -4.0).unwrap().bottom, 200.0);
    }

    #[test]
    fn face_movement_keeps_the_caption_still_while_it_stays_clear() {
        let words = [spoken(0.0, 1.0, "はい。", 1), spoken(2.0, 3.0, "そう。", 1)];
        let faces = [
            FaceSample {
                time: 0.5,
                top: 600.0,
                bottom: 700.0,
                center_x: 700.0,
            },
            FaceSample {
                time: 2.5,
                top: 650.0,
                bottom: 750.0,
                center_x: 900.0,
            },
        ];
        let (cues, _) = build_cues(
            &words,
            &faces,
            &FULL_HD,
            &at(Anchor::FaceAbove, 0.0, 0.0),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(cues[0].y, cues[1].y);
        assert_eq!(cues[0].x, cues[1].x);
    }

    #[test]
    fn a_lone_false_detection_never_moves_a_caption() {
        let steady = |time: f64| FaceSample {
            time,
            top: 600.0,
            bottom: 700.0,
            center_x: 960.0,
        };
        let faces = [
            steady(0.0),
            steady(1.0),
            FaceSample {
                time: 2.0,
                top: 5.0,
                bottom: 80.0,
                center_x: 100.0,
            },
            steady(3.0),
            steady(4.0),
        ];
        assert_eq!(without_outliers(&faces, 150.0).len(), 4);
        assert_eq!(without_outliers(&faces[1..3], 150.0).len(), 2);
        let words = [spoken(1.5, 2.5, "はい", 1)];
        let (cues, _) = build_cues(
            &words,
            &faces,
            &FULL_HD,
            &at(Anchor::FaceAbove, 0.0, 0.0),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(cues[0].y.round(), 369.0);
    }

    #[test]
    fn a_new_shot_moves_the_caption_only_when_the_old_place_covers_the_face() {
        let at_top = |time: f64, top: f64| FaceSample {
            time,
            top,
            bottom: top + 100.0,
            center_x: 960.0,
        };
        let faces = [
            at_top(0.0, 600.0),
            at_top(1.0, 600.0),
            at_top(10.0, 300.0),
            at_top(11.0, 300.0),
            at_top(20.0, 700.0),
            at_top(21.0, 700.0),
        ];
        let words = [
            spoken(0.0, 1.0, "はい。", 1),
            spoken(10.0, 11.0, "そう。", 1),
            spoken(20.0, 21.0, "次。", 1),
        ];
        let (cues, _) = build_cues(
            &words,
            &faces,
            &FULL_HD,
            &at(Anchor::FaceAbove, 0.0, 0.0),
            test_segmenter(),
        )
        .unwrap();
        // Above the head while it fits, below when the head leaves no room, above again when the
        // place below no longer clears the head.
        assert_eq!(
            cues.iter().map(|cue| cue.y.round()).collect::<Vec<_>>(),
            vec![369.0, 506.0, 469.0]
        );
    }

    /// Whether any face rectangle meets any cue's block during that cue.
    fn cues_touch_faces(
        cues: &[Cue],
        faces: &[FaceSample],
        settings: &Settings,
        info: &VideoInfo,
    ) -> bool {
        let metrics = metrics_for(info, settings);
        cues.iter().any(|cue| {
            let width = block_width(&cue.lines, &metrics, settings) / 2.0;
            let height = block_height(cue.lines.len(), &metrics, settings) / 2.0;
            faces
                .iter()
                .filter(|face| (cue.start..=cue.end).contains(&face.time))
                .any(|face| {
                    let half = (face.bottom - face.top) / 2.0;
                    cue.y - height < face.bottom
                        && cue.y + height > face.top
                        && cue.x - width < face.center_x + half
                        && cue.x + width > face.center_x - half
                })
        })
    }

    fn talking_over(face: (f64, f64, f64), info: &VideoInfo, anchor: Anchor) -> bool {
        let faces: Vec<FaceSample> = (0..12)
            .map(|second| FaceSample {
                time: f64::from(second),
                top: face.0,
                bottom: face.1,
                center_x: face.2,
            })
            .collect();
        let words = [
            spoken(0.0, 2.0, "今日はみんなで公園に行って", 1),
            spoken(2.0, 4.0, "お弁当を食べてきました", 1),
            spoken(4.0, 6.0, "とても楽しかったので", 1),
            spoken(6.0, 8.0, "また来週も行こうと思います", 1),
        ];
        let settings = at(anchor, 0.0, 0.0);
        let (cues, _) = build_cues(&words, &faces, info, &settings, test_segmenter()).unwrap();
        cues_touch_faces(&cues, &faces, &settings, info)
    }

    #[test]
    fn captions_never_touch_a_face_wherever_it_is_on_full_hd() {
        assert!(!talking_over(
            (100.0, 300.0, 960.0),
            &FULL_HD,
            Anchor::FaceAbove
        ));
        assert!(!talking_over(
            (400.0, 700.0, 960.0),
            &FULL_HD,
            Anchor::FaceAbove
        ));
        assert!(!talking_over(
            (700.0, 1000.0, 960.0),
            &FULL_HD,
            Anchor::FaceAbove
        ));
        assert!(!talking_over(
            (320.0, 800.0, 1270.0),
            &FULL_HD,
            Anchor::FaceAbove
        ));
        assert!(!talking_over(
            (320.0, 950.0, 300.0),
            &FULL_HD,
            Anchor::FaceBelow
        ));
        assert!(!talking_over(
            (0.0, 1000.0, 1700.0),
            &FULL_HD,
            Anchor::FaceBelow
        ));
        assert!(!talking_over(
            (100.0, 300.0, 960.0),
            &FULL_HD,
            Anchor::FaceBelow
        ));
    }

    #[test]
    fn captions_never_touch_a_face_on_vertical_and_uhd() {
        assert!(!talking_over(
            (500.0, 750.0, 320.0),
            &VERTICAL_SMALL,
            Anchor::FaceAbove
        ));
        assert!(!talking_over(
            (60.0, 300.0, 360.0),
            &VERTICAL_SMALL,
            Anchor::FaceAbove
        ));
        assert!(!talking_over(
            (900.0, 1150.0, 360.0),
            &VERTICAL_SMALL,
            Anchor::FaceBelow
        ));
        assert!(!talking_over(
            (800.0, 1200.0, 540.0),
            &VERTICAL,
            Anchor::FaceAbove
        ));
        assert!(!talking_over(
            (1500.0, 1850.0, 540.0),
            &VERTICAL,
            Anchor::FaceBelow
        ));
        assert!(!talking_over(
            (640.0, 1600.0, 2540.0),
            &UHD,
            Anchor::FaceAbove
        ));
    }

    #[test]
    fn a_face_filling_the_frame_leaves_the_last_resort_at_the_low_edge() {
        let settings = at(Anchor::FaceAbove, 0.0, 0.0);
        let metrics = metrics_for(&FULL_HD, &settings);
        let faces = [FaceSample {
            time: 0.5,
            top: 0.0,
            bottom: 1080.0,
            center_x: 960.0,
        }];
        let mut placer = FacePlacer::new(&faces, &FULL_HD, &metrics, &settings);
        assert_eq!(
            placer.center(0.0, 1.0, (100.0, 94.0)),
            Some((228.0, 1009.0))
        );
        assert_eq!(
            placer.center(0.0, 1.0, (1900.0, 94.0)),
            Some((960.0, 1009.0))
        );
    }

    #[test]
    fn a_position_that_no_longer_clears_the_face_gives_way() {
        let settings = at(Anchor::FaceBelow, 0.0, 0.0);
        let metrics = metrics_for(&FULL_HD, &settings);
        let sample = |time: f64, top: f64, bottom: f64| FaceSample {
            time,
            top,
            bottom,
            center_x: 960.0,
        };
        let faces = [
            sample(0.0, 100.0, 200.0),
            sample(10.0, 40.0, 100.0),
            sample(20.0, 700.0, 800.0),
            sample(30.0, 100.0, 200.0),
        ];
        let mut placer = FacePlacer::new(&faces, &FULL_HD, &metrics, &settings);
        assert_eq!(placer.center(0.0, 0.5, (100.0, 94.0)), Some((960.0, 307.0)));
        assert_eq!(placer.current, Some(Spot::Below(260.0)));
        assert_eq!(
            placer.center(10.0, 10.5, (100.0, 94.0)),
            Some((960.0, 307.0))
        );
        assert_eq!(
            placer.center(20.0, 20.5, (100.0, 94.0)),
            Some((960.0, 468.0))
        );
        assert_eq!(placer.current, Some(Spot::Above(515.0)));
        assert_eq!(
            placer.center(30.0, 30.5, (100.0, 94.0)),
            Some((960.0, 307.0))
        );
        assert_eq!(placer.current, Some(Spot::Below(260.0)));
    }

    #[test]
    fn with_no_room_a_block_goes_beside_the_face_on_its_wider_side() {
        let settings = fcpxml_settings();
        let metrics = metrics_for(&FULL_HD, &settings);
        let placer = FacePlacer::new(&[], &FULL_HD, &metrics, &settings);
        let zone = |left: f64, right: f64| FaceZone {
            start: 0.0,
            end: 1.0,
            head_top: 0.0,
            top: 50.0,
            bottom: 1000.0,
            left,
            right,
        };
        assert_eq!(
            placer.free_center(&zone(1000.0, 1500.0), (400.0, 94.0)),
            Some((518.0, 1009.0))
        );
        assert_eq!(
            placer.free_center(&zone(300.0, 800.0), (400.0, 94.0)),
            Some((1342.0, 1009.0))
        );
        assert_eq!(
            placer.free_center(&zone(300.0, 1600.0), (400.0, 94.0)),
            None
        );
    }

    #[test]
    fn a_block_too_wide_for_the_side_may_use_the_top_above_the_face_box() {
        let settings = fcpxml_settings();
        let metrics = metrics_for(&FULL_HD, &settings);
        let placer = FacePlacer::new(&[], &FULL_HD, &metrics, &settings);
        let zone = FaceZone {
            start: 0.0,
            end: 1.0,
            head_top: 0.0,
            top: 300.0,
            bottom: 1000.0,
            left: 300.0,
            right: 1600.0,
        };
        assert_eq!(
            placer.free_center(&zone, (400.0, 100.0)),
            Some((960.0, 190.0))
        );
    }

    #[test]
    fn a_face_that_only_visits_during_a_shot_still_lets_a_cue_use_its_own_moment() {
        let settings = fcpxml_settings();
        let metrics = metrics_for(&FULL_HD, &settings);
        let sample = |time: f64, top: f64, bottom: f64| FaceSample {
            time,
            top,
            bottom,
            center_x: 960.0,
        };
        let mut placer = FacePlacer::new(
            &[
                sample(0.0, 0.0, 1040.0),
                sample(1.0, 0.0, 1040.0),
                sample(3.0, 0.0, 900.0),
                sample(4.0, 0.0, 900.0),
            ],
            &FULL_HD,
            &metrics,
            &settings,
        );
        assert_eq!(placer.zones.len(), 1);
        assert_eq!(
            placer.center(3.0, 4.0, (2000.0, 94.0)),
            Some((960.0, 1009.0))
        );
    }

    #[test]
    fn a_nudge_is_the_only_way_a_caption_covers_a_face() {
        let words = [spoken(0.0, 1.0, "はい", 1)];
        let faces = [FaceSample {
            time: 0.5,
            top: 600.0,
            bottom: 700.0,
            center_x: 960.0,
        }];
        let (cues, _) = build_cues(
            &words,
            &faces,
            &FULL_HD,
            &at(Anchor::FaceAbove, 0.0, 26.0),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(cues[0].y.round(), 650.0);
    }

    #[test]
    fn every_cue_stays_inside_the_budgets_and_clear_of_the_next() {
        let words = [
            spoken(0.0, 0.1, "はい", 1),
            spoken(0.1, 0.2, "そう", 1),
            spoken(
                0.2,
                1.4,
                "今日はみんなで公園に行ってお弁当を食べてきました",
                1,
            ),
            spoken(
                1.4,
                5.0,
                "それからとても楽しかったので来週もまた行こうと思います",
                1,
            ),
            spoken(5.0, 5.2, "ね。", 1),
            spoken(
                5.2,
                13.0,
                "ゆっくりと話していたらいつの間にか夕方になっていました",
                1,
            ),
            spoken(16.0, 16.1, "ん", 1),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        let metrics = metrics_for(&FULL_HD, &fcpxml_settings());
        assert!(cues
            .iter()
            .flat_map(|cue| cue.lines.iter())
            .all(|line| line.chars().count() as f64 * metrics.font_px <= metrics.max_line_px));
        assert!(cues.iter().all(|cue| cue.lines.len() <= 2));
        assert!(cues
            .iter()
            .all(|cue| cue.end - cue.start >= 5.0 / 6.0 - 1e-9));
        assert!(cues.iter().all(|cue| cue.end - cue.start <= 7.0));
        assert!(cues.windows(2).all(|pair| pair[0].end <= pair[1].start));
    }

    #[test]
    fn a_stray_at_the_cue_time_limit_joins_by_time_too() {
        let group = [spoken(0.0, 0.3, "はいそうですありがとう", 1)];
        let next = [spoken(0.3, 0.5, "いいですよそのとおり", 1)];
        assert!(joins_stray(&group, &next));
    }

    #[test]
    fn silence_after_a_group_keeps_its_last_page_readable() {
        assert_eq!(
            following_silence(&[word(0.0, 1.0, "は")], Some(&vec![word(3.0, 4.0, "い")])),
            2.0
        );
        assert_eq!(
            following_silence(&[word(0.0, 1.0, "は")], None),
            f64::INFINITY
        );
    }

    #[test]
    fn words_past_the_end_of_the_video_show_nothing() {
        let words = [word(0.0, 1.0, "はい"), word(60.5, 61.0, "time.")];
        assert_eq!(display_words(&words, 60.0).len(), 1);
    }

    #[test]
    fn two_turns_of_one_speaker_add_up() {
        let mut words = [word(0.0, 1.0, "は")];
        assign_speakers(
            &mut words,
            &[
                SpeakerTurn {
                    start: 0.0,
                    end: 0.4,
                    speaker: 1,
                },
                SpeakerTurn {
                    start: 0.4,
                    end: 0.8,
                    speaker: 1,
                },
                SpeakerTurn {
                    start: 0.8,
                    end: 1.0,
                    speaker: 2,
                },
            ],
        );
        assert_eq!(words[0].speaker, Some(1));
    }

    #[test]
    fn the_voice_covering_a_word_longest_gets_it() {
        let mut words = [word(0.0, 1.0, "は")];
        assign_speakers(
            &mut words,
            &[
                SpeakerTurn {
                    start: 0.0,
                    end: 0.5,
                    speaker: 1,
                },
                SpeakerTurn {
                    start: 0.9,
                    end: 1.0,
                    speaker: 2,
                },
            ],
        );
        assert_eq!(words[0].speaker, Some(1));
    }

    #[test]
    fn a_block_wider_than_the_frame_is_centred() {
        assert_eq!(clamp_center(10.0, 200.0, 100.0), 50.0);
    }

    #[test]
    fn the_reported_mid_word_breaks_stay_whole() {
        let words = [
            spoken(0.0, 0.4, "結", 1),
            spoken(0.4, 0.8, "構", 1),
            spoken(0.8, 1.2, "お", 1),
            spoken(1.2, 1.6, "腹", 1),
            spoken(1.6, 1.8, "い", 1),
            spoken(1.9, 2.1, "っぱ", 1),
            spoken(2.1, 2.3, "い", 1),
            spoken(2.3, 2.6, "なんだ", 1),
            spoken(2.6, 2.9, "けど", 1),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(cues[0].lines, vec!["結構お腹いっぱいなんだけど"]);
        assert_eq!(cues.len(), 1);
    }

    #[test]
    fn the_reported_broken_copula_stays_whole() {
        let words = [
            spoken(0.0, 0.5, "これは", 1),
            spoken(0.5, 1.0, "字幕", 1),
            spoken(1.0, 1.3, "で", 1),
            spoken(1.4, 1.7, "す", 1),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].lines, vec!["これは字幕です"]);
    }

    fn voice_page(start: f64, end: f64) -> Page {
        Page {
            start,
            end,
            speaker: None,
            lines: vec!["あ".to_owned()],
            narrow: vec!["あ".to_owned()],
        }
    }

    #[test]
    fn a_page_timed_into_silence_starts_with_the_voice() {
        let mut pages = [voice_page(10.0, 12.0)];
        start_with_the_voice(
            &mut pages,
            &[
                VoiceSpan {
                    start: 1.0,
                    end: 2.0,
                },
                VoiceSpan {
                    start: 10.5,
                    end: 13.0,
                },
            ],
        );
        assert_eq!(pages[0].start, 10.5);
    }

    #[test]
    fn a_page_already_in_a_voice_stays() {
        let mut pages = [voice_page(10.0, 12.0)];
        start_with_the_voice(
            &mut pages,
            &[VoiceSpan {
                start: 9.9,
                end: 13.0,
            }],
        );
        assert_eq!(pages[0].start, 10.0);
    }

    #[test]
    fn a_far_voice_or_one_after_the_words_changes_nothing() {
        let mut far = [voice_page(10.0, 12.0)];
        start_with_the_voice(
            &mut far,
            &[VoiceSpan {
                start: 10.7,
                end: 13.0,
            }],
        );
        assert_eq!(far[0].start, 10.0);
        let mut short = [voice_page(10.0, 10.2)];
        start_with_the_voice(
            &mut short,
            &[VoiceSpan {
                start: 10.5,
                end: 13.0,
            }],
        );
        assert_eq!(short[0].start, 10.0);
    }

    #[test]
    fn no_voice_found_changes_nothing() {
        let mut pages = [voice_page(10.0, 12.0)];
        start_with_the_voice(&mut pages, &[]);
        assert_eq!(pages[0].start, 10.0);
    }

    #[test]
    fn a_line_never_breaks_between_a_sokuon_and_the_kana_it_doubles() {
        let group = vec![word(0.0, 4.0, "怒られなくってって")];
        let metrics = metrics_for(&VERTICAL_SMALL, &fcpxml_settings());
        let run = Run::new(&group, f64::INFINITY, &metrics, test_segmenter()).unwrap();
        let sokuon = "怒られなくっ".chars().count();
        assert!(!run.may_break_line(sokuon));
        assert!(run.may_break_line(sokuon - 2));
    }

    #[test]
    fn a_sokuon_before_a_kanji_may_end_a_line() {
        let group = vec![word(0.0, 4.0, "あっ今日")];
        let metrics = metrics_for(&VERTICAL_SMALL, &fcpxml_settings());
        let run = Run::new(&group, f64::INFINITY, &metrics, test_segmenter()).unwrap();
        assert!(run.may_break_line(2));
    }

    fn wrapped(info: &VideoInfo, metrics: &Metrics, settings: &Settings) -> Vec<Vec<String>> {
        let words = vec![word(0.0, 3.0, "今日はとても楽しい一日でしたね")];
        super::build_cues(&words, &[], &[], info, metrics, settings, test_segmenter())
            .unwrap()
            .0
            .into_iter()
            .map(|cue| cue.lines)
            .collect()
    }

    fn narrow_settings() -> Settings {
        Settings {
            width_ratio: 0.5,
            ..fcpxml_settings()
        }
    }

    #[test]
    fn the_frame_size_changes_where_a_line_wraps() {
        let settings = narrow_settings();
        let wide = metrics_for(&FULL_HD, &settings);
        let tall = metrics_for(&VERTICAL_SMALL, &settings);
        assert_eq!(
            wrapped(&FULL_HD, &wide, &settings),
            vec![vec!["今日はとても楽しい一日でしたね".to_owned()]]
        );
        assert_eq!(
            wrapped(&VERTICAL_SMALL, &tall, &settings),
            vec![
                vec!["今日は".to_owned(), "とても楽しい".to_owned()],
                vec!["一日でしたね".to_owned()]
            ]
        );
    }

    #[test]
    fn the_font_size_changes_where_a_line_wraps() {
        let small = narrow_settings();
        let large = Settings {
            font_ratio: 0.08,
            ..narrow_settings()
        };
        assert_eq!(
            wrapped(&FULL_HD, &metrics_for(&FULL_HD, &small), &small),
            vec![vec!["今日はとても楽しい一日でしたね".to_owned()]]
        );
        assert_eq!(
            wrapped(&FULL_HD, &metrics_for(&FULL_HD, &large), &large),
            vec![vec![
                "今日はとても".to_owned(),
                "楽しい一日でしたね".to_owned()
            ]]
        );
    }

    #[test]
    fn a_narrow_fonts_own_widths_change_where_a_line_wraps() {
        let settings = narrow_settings();
        let tall = metrics_for(&VERTICAL_SMALL, &settings);
        let narrow = Metrics {
            glyphs: Some(Box::leak(Box::new(GlyphWidths::uniform(
                "今日はとても楽しい一日でしたね",
                0.5,
            )))),
            ..tall
        };
        assert_eq!(wrapped(&VERTICAL_SMALL, &tall, &settings).concat().len(), 3);
        assert_eq!(
            wrapped(&VERTICAL_SMALL, &narrow, &settings),
            vec![vec!["今日はとても楽しい一日でしたね".to_owned()]]
        );
    }

    #[test]
    fn a_portrait_fcpxml_title_is_measured_larger_and_other_formats_are_not() {
        let fcpxml = fcpxml_settings();
        let ttml = Settings {
            subtitle_format: SubtitleFormat::Ttml,
            ..fcpxml_settings()
        };
        assert_eq!(
            metrics_for(&VERTICAL_SMALL, &fcpxml).rendered_scale.round(),
            1.0
        );
        assert_eq!(
            (metrics_for(&VERTICAL_SMALL, &fcpxml).rendered_scale * 1000.0).round(),
            1154.0
        );
        assert_eq!(metrics_for(&FULL_HD, &fcpxml).rendered_scale, 0.974);
        assert_eq!(metrics_for(&VERTICAL_SMALL, &ttml).rendered_scale, 1.0);
    }

    #[test]
    fn a_blocks_edge_follows_how_the_format_draws_its_outline() {
        let fcpxml = fcpxml_settings();
        let srt = Settings {
            subtitle_format: SubtitleFormat::Srt,
            ..fcpxml.clone()
        };
        assert_eq!(block_edge(40.0, 4.0, &fcpxml), 10.0);
        assert_eq!(block_edge(40.0, 4.0, &srt), 4.0);
    }

    #[test]
    fn a_font_width_replaces_the_estimate_and_a_missing_glyph_keeps_it() {
        let metrics = Metrics {
            font_px: 10.0,
            outline_px: 1.0,
            max_line_px: 100.0,
            glyphs: Some(Box::leak(Box::new(GlyphWidths::uniform("a", 0.25)))),
            rendered_scale: 1.0,
        };
        assert_eq!(glyph_width('a', &metrics), 2.5);
        assert_eq!(glyph_width('b', &metrics), 6.0);
    }

    #[test]
    fn glyph_widths_follow_the_safe_wrap_estimate() {
        let at_ten_px = Metrics {
            font_px: 10.0,
            outline_px: 1.0,
            max_line_px: 100.0,
            glyphs: None,
            rendered_scale: 1.0,
        };
        assert_eq!((glyph_width('a', &at_ten_px) * 100.0).round(), 600.0);
        assert_eq!((glyph_width('ｱ', &at_ten_px) * 100.0).round(), 500.0);
        assert_eq!((glyph_width('あ', &at_ten_px) * 100.0).round(), 1000.0);
    }

    #[test]
    fn an_unequal_safe_zone_narrows_the_line_and_moves_the_block() {
        let settings = Settings {
            safe_left_percent: 20.0,
            safe_right_percent: 5.0,
            ..fcpxml_settings()
        };
        assert_eq!(inside_safe_zone(100.0, 100.0, 1000.0, &settings), 250.0);
        assert_eq!(inside_safe_zone(990.0, 100.0, 1000.0, &settings), 900.0);
        assert_eq!(inside_safe_zone(500.0, 100.0, 1000.0, &settings), 500.0);
        assert_eq!(inside_safe_zone(500.0, 900.0, 1000.0, &settings), 575.0);
        assert!(
            metrics_for(&FULL_HD, &settings).max_line_px
                < metrics_for(&FULL_HD, &fcpxml_settings()).max_line_px
        );
    }

    #[test]
    fn every_block_stays_inside_the_safe_zone_on_narrow_and_wide_frames() {
        let words = [
            spoken(
                0.0,
                3.0,
                "今日はみんなで公園に行ってお弁当を食べてきました",
                1,
            ),
            spoken(
                3.0,
                6.0,
                "とても楽しかったので来週もまた行こうと思います",
                1,
            ),
        ];
        let settings = Settings {
            safe_left_percent: 12.0,
            safe_right_percent: 3.0,
            ..fcpxml_settings()
        };
        let (narrow, _) =
            build_cues(&words, &[], &VERTICAL_SMALL, &settings, test_segmenter()).unwrap();
        let metrics = metrics_for(&VERTICAL_SMALL, &settings);
        assert!(narrow.iter().all(|cue| {
            let half = block_width(&cue.lines, &metrics, &settings) / 2.0;
            cue.x - half >= 720.0 * 0.12 - 1e-6 && cue.x + half <= 720.0 * 0.97 + 1e-6
        }));
        let (wide, _) = build_cues(&words, &[], &FULL_HD, &settings, test_segmenter()).unwrap();
        let metrics = metrics_for(&FULL_HD, &settings);
        assert!(wide.iter().all(|cue| {
            let half = block_width(&cue.lines, &metrics, &settings) / 2.0;
            cue.x - half >= 1920.0 * 0.12 - 1e-6 && cue.x + half <= 1920.0 * 0.97 + 1e-6
        }));
    }

    #[test]
    fn a_nudge_cannot_push_a_caption_past_the_safe_zone() {
        let words = [spoken(0.0, 1.0, "はい", 1)];
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &at(Anchor::VideoBottom, 50.0, 0.0),
            test_segmenter(),
        )
        .unwrap();
        let metrics = metrics_for(&FULL_HD, &fcpxml_settings());
        let half = block_width(&cues[0].lines, &metrics, &fcpxml_settings()) / 2.0;
        assert_eq!((cues[0].x + half).round(), 1824.0);
    }

    #[test]
    fn a_block_beside_a_face_stays_inside_the_safe_zone() {
        let settings = Settings {
            safe_left_percent: 10.0,
            safe_right_percent: 20.0,
            ..fcpxml_settings()
        };
        let metrics = metrics_for(&FULL_HD, &settings);
        let placer = FacePlacer::new(&[], &FULL_HD, &metrics, &settings);
        let zone = |left: f64, right: f64| FaceZone {
            start: 0.0,
            end: 1.0,
            head_top: 0.0,
            top: 50.0,
            bottom: 1000.0,
            left,
            right,
        };
        assert_eq!(
            placer.free_center(&zone(1000.0, 1500.0), (400.0, 94.0)),
            Some((566.0, 1009.0))
        );
        assert_eq!(
            placer.free_center(&zone(300.0, 700.0), (400.0, 94.0)),
            Some((1148.0, 1009.0))
        );
    }

    #[test]
    fn a_page_ends_at_a_clause_rather_than_filling_the_width() {
        let words = [
            spoken(0.0, 3.0, "今日は天気がいいので、", 1),
            spoken(3.0, 5.0, "みんなで公園に行って", 1),
            spoken(5.0, 7.0, "お弁当を食べました", 1),
        ];
        let (cues, _) =
            build_cues(&words, &[], &VERTICAL, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(
            cues.iter().map(|cue| cue.lines.clone()).collect::<Vec<_>>(),
            vec![
                vec!["今日は", "天気がいいので、"],
                vec!["みんなで", "公園に行って"],
                vec!["お弁当を食べました"],
            ]
        );
    }

    #[test]
    fn fast_speech_holds_more_on_a_page_than_slow_speech() {
        let timed = |seconds: f64| {
            [
                spoken(0.0, seconds, "今日は", 1),
                spoken(seconds, seconds * 2.0, "公園に", 1),
                spoken(seconds * 2.0, seconds * 3.0, "行って", 1),
                spoken(seconds * 3.0, seconds * 4.0, "遊びました", 1),
            ]
        };
        let settings = fcpxml_settings();
        let (fast, _) =
            build_cues(&timed(0.6), &[], &FULL_HD, &settings, test_segmenter()).unwrap();
        let (slow, _) =
            build_cues(&timed(1.6), &[], &FULL_HD, &settings, test_segmenter()).unwrap();
        assert_eq!(fast.len(), 1);
        assert_eq!(slow.len(), 2);
    }

    #[test]
    fn a_larger_font_fits_fewer_characters_on_a_line_with_no_fixed_limit() {
        let words = [spoken(
            0.0,
            3.0,
            "あのねお茶漬けをいろんな人に食べてもらいました",
            1,
        )];
        let large = Settings {
            font_ratio: 0.09,
            ..fcpxml_settings()
        };
        let (normal, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        let (big, _) = build_cues(&words, &[], &FULL_HD, &large, test_segmenter()).unwrap();
        assert_eq!(normal[0].lines.len(), 1);
        assert_eq!(big[0].lines.len(), 2);
    }

    #[test]
    fn a_stray_particle_stays_with_its_phrase() {
        let words = [
            spoken(0.0, 0.5, "そうです", 1),
            spoken(0.5, 0.6, "よ", 1),
            spoken(0.7, 1.5, "ね", 1),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].lines, vec!["そうですよね"]);
    }

    #[test]
    fn vertical_video_wraps_at_a_phrase_and_landscape_stays_on_one_line() {
        let words = [spoken(
            0.0,
            3.0,
            "あのねお茶漬けをいろんな人に食べてもらいました",
            1,
        )];
        let (wide, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        let (tall, _) = build_cues(
            &words,
            &[],
            &VERTICAL_SMALL,
            &fcpxml_settings(),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(
            wide[0].lines,
            vec!["あのねお茶漬けをいろんな人に食べてもらいました"]
        );
        assert_eq!(
            tall[0].lines,
            vec!["あのねお茶漬けをいろんな", "人に食べてもらいました"]
        );
    }

    #[test]
    fn pages_and_lines_keep_kinsoku() {
        let metrics = Metrics {
            font_px: 10.0,
            outline_px: 1.0,
            max_line_px: 72.0,
            glyphs: None,
            rendered_scale: 1.0,
        };
        let group = [spoken(0.0, 7.0, "はい、(そうです。)はい", 1)];
        let run = Run::new(&group, f64::INFINITY, &metrics, test_segmenter()).unwrap();
        let lines: Vec<Vec<String>> = run.pages().into_iter().map(|page| page.lines).collect();
        assert_eq!(lines, vec![vec!["はい、"], vec!["(そうです。)", "はい"]]);
    }

    #[test]
    fn one_word_too_long_for_the_page_splits_at_characters_only_as_a_last_resort() {
        let metrics = Metrics {
            font_px: 10.0,
            outline_px: 1.0,
            max_line_px: 50.0,
            glyphs: None,
            rendered_scale: 1.0,
        };
        let group = [spoken(0.0, 5.0, "サンフランシスコ", 1)];
        let run = Run::new(&group, f64::INFINITY, &metrics, test_segmenter()).unwrap();
        let lines: Vec<Vec<String>> = run.pages().into_iter().map(|page| page.lines).collect();
        assert_eq!(lines, vec![vec!["サンフラ", "ンシスコ"]]);
    }

    #[test]
    fn a_glyph_wider_than_nothing_else_fits_still_makes_a_page() {
        let metrics = Metrics {
            font_px: 10.0,
            outline_px: 1.0,
            max_line_px: 10.0,
            glyphs: None,
            rendered_scale: 1.0,
        };
        let group = [spoken(0.0, 1.0, "っっ", 1)];
        let run = Run::new(&group, f64::INFINITY, &metrics, test_segmenter()).unwrap();
        let lines: Vec<Vec<String>> = run.pages().into_iter().map(|page| page.lines).collect();
        assert_eq!(lines, vec![vec!["っっ"]]);
    }

    #[test]
    fn spaced_text_breaks_at_words_without_the_dictionary() {
        let words = [
            spoken(0.0, 0.5, " Hello", 1),
            spoken(0.5, 1.0, " world,", 1),
            spoken(1.0, 1.5, " this", 1),
            spoken(1.5, 2.0, " is", 1),
            spoken(2.0, 2.5, " a", 1),
            spoken(2.5, 3.0, " caption", 1),
        ];
        let missing = Segmenter::new(PathBuf::from("/nonexistent/dictionary"));
        let (cues, _) = build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), &missing).unwrap();
        assert_eq!(cues[0].lines, vec!["Hello world, this is a caption"]);
    }

    #[test]
    fn a_blank_run_makes_no_page() {
        let metrics = Metrics {
            font_px: 10.0,
            outline_px: 1.0,
            max_line_px: 12.0,
            glyphs: None,
            rendered_scale: 1.0,
        };
        let group = [spoken(0.0, 1.0, "   ", 1)];
        let run = Run::new(&group, f64::INFINITY, &metrics, test_segmenter()).unwrap();
        assert_eq!(run.pages(), Vec::<Page>::new());
    }

    #[test]
    fn a_missing_dictionary_is_an_error_for_japanese() {
        let missing = Segmenter::new(PathBuf::from("/nonexistent/dictionary"));
        let words = [spoken(0.0, 1.0, "はい", 1)];
        let error = build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), &missing).unwrap_err();
        assert!(error
            .to_string()
            .starts_with("read the caption dictionary: "));
    }

    #[test]
    fn a_phrase_inside_one_token_is_timed_by_its_characters() {
        let words = [
            spoken(0.0, 12.0, "今日は公園へ行きました", 1),
            spoken(12.0, 14.0, "とても楽しかったです", 1),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(
            cues.iter()
                .map(|cue| cue.lines.join(""))
                .collect::<Vec<_>>(),
            vec!["今日は公園へ行きました", "とても楽しかったです"]
        );
        assert_eq!(cues[1].start.round(), 12.0);
    }

    #[test]
    fn slow_speech_spreads_over_several_pages() {
        let words = [
            spoken(0.0, 1.0, "今日は", 1),
            spoken(1.6, 2.6, "天気が", 1),
            spoken(3.2, 4.2, "良いので", 1),
            spoken(4.8, 5.8, "公園へ", 1),
            spoken(6.4, 7.4, "行きました", 1),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(
            cues.iter()
                .map(|cue| cue.lines.join(""))
                .collect::<Vec<_>>(),
            vec!["今日は", "天気が", "良いので", "公園へ", "行きました"]
        );
    }

    #[test]
    fn a_word_stretched_past_the_longest_cue_stays_whole_and_is_cut_at_display() {
        let words = [
            spoken(0.0, 6.0, "おい", 1),
            spoken(6.0, 12.0, "しい", 1),
            spoken(12.0, 14.0, "すごいこれ", 1),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(
            cues.iter()
                .map(|cue| (cue.start, cue.end, cue.lines.join("")))
                .collect::<Vec<_>>(),
            vec![
                (0.0, 7.0, "おいしい".to_owned()),
                (12.0, 14.12, "すごいこれ".to_owned())
            ]
        );
    }

    #[test]
    fn a_gap_before_a_dependent_word_never_splits_it() {
        let words = [
            spoken(0.0, 1.0, "今日は雨", 1),
            spoken(2.0, 3.0, "ですね", 1),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].lines, vec!["今日は雨ですね"]);
    }

    #[test]
    fn a_gap_between_words_stays_a_break() {
        let words = [
            spoken(0.0, 1.0, "公園に行く", 1),
            spoken(2.0, 3.0, "今日は暑い", 1),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(cues.len(), 2);
    }

    #[test]
    fn no_rejoin_across_speakers_or_a_long_silence() {
        let left = [spoken(0.0, 1.0, "いいね", 1)];
        assert!(!splits_a_word(&left, &[spoken(1.5, 2.0, "え", 2)], test_segmenter()).unwrap());
        assert!(!splits_a_word(&left, &[spoken(9.0, 9.5, "え", 1)], test_segmenter()).unwrap());
        assert!(!splits_a_word(&[], &left, test_segmenter()).unwrap());
    }

    #[test]
    fn a_page_ends_where_the_speaker_really_pauses() {
        let words = [
            spoken(0.0, 1.0, "そうですね", 1),
            spoken(1.3, 2.3, "今日は", 1),
            spoken(2.3, 3.3, "公園に", 1),
        ];
        let group = group_words(&words);
        let metrics = metrics_for(&FULL_HD, &fcpxml_settings());
        let run = Run::new(&group[0], f64::INFINITY, &metrics, test_segmenter()).unwrap();
        assert_eq!(
            run.phrase_cost(&PhraseEnd {
                offset: 5,
                class: BreakClass::Plain,
            }),
            0.0
        );
    }

    #[test]
    fn a_break_inside_a_token_costs_more_than_one_at_its_edge() {
        let group = [spoken(0.0, 1.0, "公園に", 1), spoken(1.0, 2.0, "行く人", 1)];
        let metrics = metrics_for(&FULL_HD, &fcpxml_settings());
        let run = Run::new(&group, f64::INFINITY, &metrics, test_segmenter()).unwrap();
        assert_eq!(
            run.phrase_cost(&PhraseEnd {
                offset: 3,
                class: BreakClass::Particle,
            }),
            1.2
        );
        assert_eq!(
            run.phrase_cost(&PhraseEnd {
                offset: 2,
                class: BreakClass::Particle,
            }),
            1.7
        );
        assert_eq!(
            run.phrase_cost(&PhraseEnd {
                offset: 2,
                class: BreakClass::Clause,
            }),
            0.5
        );
        assert_eq!(
            run.phrase_cost(&PhraseEnd {
                offset: 2,
                class: BreakClass::Closing,
            }),
            0.8
        );
        assert_eq!(
            run.phrase_cost(&PhraseEnd {
                offset: 2,
                class: BreakClass::Connective,
            }),
            1.1
        );
    }

    fn speakers_after_smoothing(mut words: Vec<Word>) -> Vec<Option<u32>> {
        smooth_speakers(&mut words, test_segmenter()).unwrap();
        words.iter().map(|word| word.speaker).collect()
    }

    #[test]
    fn a_brief_turn_between_two_of_the_same_speaker_joins_them() {
        assert_eq!(
            speakers_after_smoothing(vec![
                spoken(0.0, 2.0, "今日は", 1),
                spoken(2.0, 2.4, "うん", 2),
                spoken(2.4, 4.0, "いい天気", 1),
            ]),
            vec![Some(1), Some(1), Some(1)]
        );
    }

    #[test]
    fn a_brief_turn_set_apart_by_pauses_keeps_its_speaker() {
        assert_eq!(
            speakers_after_smoothing(vec![
                spoken(0.0, 2.0, "今日は", 1),
                spoken(3.0, 3.4, "うん", 2),
                spoken(4.0, 6.0, "いい天気", 1),
            ]),
            vec![Some(1), Some(2), Some(1)]
        );
    }

    #[test]
    fn a_long_enough_turn_keeps_its_colour_inside_a_sentence() {
        assert_eq!(
            speakers_after_smoothing(vec![
                spoken(0.0, 2.0, "今日は。", 1),
                spoken(2.0, 3.0, "本当に。", 2),
                spoken(3.0, 5.0, "いい天気。", 1),
            ]),
            vec![Some(1), Some(2), Some(1)]
        );
    }

    #[test]
    fn a_brief_turn_between_two_others_joins_the_closer_one() {
        assert_eq!(
            speakers_after_smoothing(vec![
                spoken(0.0, 2.0, "今日は", 1),
                spoken(2.3, 2.5, "もう", 2),
                spoken(2.6, 5.0, "いい天気", 3),
            ]),
            vec![Some(1), Some(3), Some(3)]
        );
        assert_eq!(
            speakers_after_smoothing(vec![
                spoken(0.0, 2.0, "今日は", 1),
                spoken(2.1, 2.3, "もう", 2),
                spoken(2.7, 5.0, "いい天気", 3),
            ]),
            vec![Some(1), Some(1), Some(3)]
        );
    }

    #[test]
    fn a_brief_first_or_last_turn_joins_the_one_beside_it() {
        assert_eq!(
            speakers_after_smoothing(vec![
                spoken(0.0, 0.3, "ね", 2),
                spoken(0.3, 3.0, "今日は", 1),
                spoken(3.1, 3.3, "ね", 2),
            ]),
            vec![Some(1), Some(1), Some(1)]
        );
    }

    #[test]
    fn flickering_labels_settle_on_one_speaker() {
        assert_eq!(
            speakers_after_smoothing(vec![
                spoken(0.0, 0.2, "あ", 1),
                spoken(0.2, 0.4, "い", 2),
                spoken(0.4, 0.6, "う", 1),
                spoken(0.6, 0.8, "え", 2),
                spoken(0.8, 3.0, "お", 2),
            ]),
            vec![Some(2), Some(2), Some(2), Some(2), Some(2)]
        );
    }

    #[test]
    fn a_change_of_speaker_never_falls_inside_a_word() {
        assert_eq!(
            speakers_after_smoothing(vec![
                spoken(0.0, 2.0, "今日はとても", 1),
                spoken(2.0, 2.1, "眠", 1),
                spoken(2.1, 2.3, "く", 2),
                spoken(2.3, 4.0, "なってきましたね", 2),
            ]),
            vec![Some(1), Some(2), Some(2), Some(2)]
        );
        assert_eq!(
            speakers_after_smoothing(vec![
                spoken(0.0, 2.0, "今日はとても", 1),
                spoken(2.0, 2.3, "眠", 1),
                spoken(2.3, 2.4, "く", 2),
                spoken(2.4, 4.0, "なってきましたね", 2),
            ]),
            vec![Some(1), Some(1), Some(1), Some(2)]
        );
    }

    #[test]
    fn a_change_between_two_words_stays_where_it_is() {
        assert_eq!(
            speakers_after_smoothing(vec![
                spoken(0.0, 2.0, "今日は", 1),
                spoken(2.0, 4.0, "天気です", 2),
            ]),
            vec![Some(1), Some(2)]
        );
    }

    #[test]
    fn voices_really_at_once_are_not_smoothed() {
        assert_eq!(
            speakers_after_smoothing(vec![
                spoken(0.0, 3.0, "今日は", 1),
                spoken(1.0, 1.5, "うん", 2),
            ]),
            vec![Some(1), Some(2)]
        );
    }

    #[test]
    fn one_speaker_has_nothing_to_smooth() {
        assert_eq!(
            speakers_after_smoothing(vec![spoken(0.0, 0.2, "あ", 1), spoken(5.0, 5.2, "い", 1)]),
            vec![Some(1), Some(1)]
        );
    }

    /// Whisper's tokens of two rows interleave in time with next to no overlap per pair of tokens.
    #[test]
    fn two_rows_of_short_tokens_stay_apart_whole() {
        let words: Vec<Word> = ["今日", "は", "いい", "天気", "です", "ね"]
            .iter()
            .enumerate()
            .flat_map(|(at, text)| {
                let start = at as f64 * 0.4;
                [
                    spoken(start, start + 0.38, text, 1),
                    spoken(
                        start + 0.1,
                        start + 0.48,
                        ["明日", "の", "朝", "は", "雨", "だ"][at],
                        2,
                    ),
                ]
            })
            .collect();
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &anchored(Anchor::VideoBottom),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(
            cues.iter()
                .map(|cue| (cue.speaker, cue.lines.join("")))
                .collect::<Vec<_>>(),
            vec![
                (Some(1), "今日はいい天気ですね".to_owned()),
                (Some(2), "明日の朝は雨だ".to_owned())
            ]
        );
    }

    #[test]
    fn a_stray_never_bridges_two_speakers() {
        let group = [spoken(0.0, 0.3, "はい", 1)];
        let next = [spoken(0.3, 0.5, "いいえ", 2)];
        assert!(!joins_stray(&group, &next));
    }

    #[test]
    fn a_tie_prefers_the_speaker_before_over_the_first_voice() {
        let mut words = [word(0.0, 1.0, "あ"), word(2.0, 3.0, "い")];
        assign_speakers(
            &mut words,
            &[
                SpeakerTurn {
                    start: 0.0,
                    end: 1.0,
                    speaker: 2,
                },
                SpeakerTurn {
                    start: 2.0,
                    end: 2.5,
                    speaker: 1,
                },
                SpeakerTurn {
                    start: 2.5,
                    end: 3.0,
                    speaker: 2,
                },
            ],
        );
        assert_eq!(words[1].speaker, Some(2));
    }

    #[test]
    fn no_turns_at_all_make_everyone_speaker_one() {
        let mut words = [word(0.0, 1.0, "あ"), word(2.0, 3.0, "い")];
        assign_speakers(&mut words, &[]);
        assert_eq!(
            words.iter().map(|word| word.speaker).collect::<Vec<_>>(),
            vec![Some(1), Some(1)]
        );
    }

    #[test]
    fn a_long_stretch_of_overlap_keeps_its_own_cue() {
        let words = [
            spoken(0.0, 1.0, "はいそうです", 1),
            word(1.0, 2.0, "いろいろあります"),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(
            cues.iter().map(|cue| cue.speaker).collect::<Vec<_>>(),
            vec![Some(1), None]
        );
    }

    fn anchored(anchor: Anchor) -> Settings {
        Settings {
            placement: Placement {
                anchor,
                offset_x_percent: 0.0,
                offset_y_percent: 0.0,
            },
            ..fcpxml_settings()
        }
    }

    /// Speaker, and the block's top and bottom in pixels.
    fn rows(cues: &[Cue], settings: &Settings, info: &VideoInfo) -> Vec<(Option<u32>, f64, f64)> {
        let metrics = metrics_for(info, settings);
        cues.iter()
            .map(|cue| {
                let half = block_height(cue.lines.len(), &metrics, settings) * cue.scale / 2.0;
                (cue.speaker, (cue.y - half).round(), (cue.y + half).round())
            })
            .collect()
    }

    #[test]
    fn two_people_partly_together_each_keep_their_time_and_a_row() {
        let words = [
            spoken(0.0, 3.0, "今日はいい天気です", 1),
            spoken(1.0, 4.0, "本当にそうですね", 2),
        ];
        let settings = anchored(Anchor::VideoBottom);
        let (cues, _) = build_cues(&words, &[], &FULL_HD, &settings, test_segmenter()).unwrap();
        assert_eq!(
            cues.iter()
                .map(|cue| (cue.start, cue.end.round() as u32))
                .collect::<Vec<_>>(),
            vec![(0.0, 3), (1.0, 4)]
        );
        assert_eq!(
            rows(&cues, &settings, &FULL_HD),
            vec![(Some(1), 868.0, 960.0), (Some(2), 767.0, 859.0)]
        );
        assert_eq!(cues[0].scale, 1.0);
    }

    #[test]
    fn two_people_fully_together_take_rows_in_speaker_order_whoever_came_first() {
        let words = [
            spoken(1.0, 3.0, "本当にそうですね", 2),
            spoken(1.0, 3.0, "今日はいい天気です", 1),
        ];
        let settings = anchored(Anchor::VideoBottom);
        let (cues, _) = build_cues(&words, &[], &FULL_HD, &settings, test_segmenter()).unwrap();
        let mut by_speaker = rows(&cues, &settings, &FULL_HD);
        by_speaker.sort_by_key(|row| row.0);
        assert_eq!(
            by_speaker,
            vec![(Some(1), 868.0, 960.0), (Some(2), 767.0, 859.0)]
        );
    }

    #[test]
    fn words_of_two_people_mixed_in_time_stay_each_speakers_own_caption() {
        let words = [
            spoken(0.0, 0.8, "今日は", 1),
            spoken(0.2, 1.0, "本当に", 2),
            spoken(0.8, 1.6, "いい天気", 1),
            spoken(1.0, 1.8, "そうですね", 2),
        ];
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &anchored(Anchor::VideoBottom),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(
            cues.iter()
                .map(|cue| (cue.speaker, cue.lines.join("")))
                .collect::<Vec<_>>(),
            vec![
                (Some(1), "今日はいい天気".to_owned()),
                (Some(2), "本当にそうですね".to_owned())
            ]
        );
    }

    #[test]
    fn four_people_stack_in_speaker_order() {
        let words = [
            spoken(0.0, 3.0, "今日はいい天気です", 3),
            spoken(0.5, 3.0, "本当にそうですね", 1),
            spoken(1.0, 3.0, "聞こえますか", 4),
            spoken(1.0, 3.0, "出かけましょう", 2),
        ];
        let settings = anchored(Anchor::VideoBottom);
        let (cues, _) = build_cues(&words, &[], &FULL_HD, &settings, test_segmenter()).unwrap();
        let mut by_height = rows(&cues, &settings, &FULL_HD);
        by_height.sort_by(|left, right| right.2.total_cmp(&left.2));
        assert_eq!(
            by_height.iter().map(|row| row.0).collect::<Vec<_>>(),
            vec![Some(1), Some(2), Some(3), Some(4)]
        );
        assert!(by_height.windows(2).all(|pair| pair[1].2 < pair[0].1));
    }

    #[test]
    fn a_long_caption_wrapped_in_two_lines_pushes_the_next_row_up() {
        let words = [
            spoken(
                0.0,
                6.0,
                "今日はとてもいい天気なので午後から近くの公園まで歩いて出かけようと思っていますが皆さんはどうされますか",
                1,
            ),
            spoken(1.0, 4.0, "本当にそうですね", 2),
        ];
        let settings = anchored(Anchor::VideoBottom);
        let (cues, _) = build_cues(&words, &[], &FULL_HD, &settings, test_segmenter()).unwrap();
        let first = cues.iter().find(|cue| cue.speaker == Some(1)).unwrap();
        assert_eq!(first.lines.len(), 2);
        let found = rows(&cues, &settings, &FULL_HD);
        let row = |speaker: u32| found.iter().find(|row| row.0 == Some(speaker)).unwrap();
        assert_eq!(row(1).2, 960.0);
        assert!(row(2).2 < row(1).1, "{found:?}");
    }

    /// Four people saying the same long sentence at once.
    fn four_long_voices() -> Vec<Word> {
        let long = "今日はとてもいい天気なので午後から近くの公園まで歩いて出かけようと思っていますが皆さんはどうされますか";
        (1..=4)
            .map(|speaker| spoken(0.0, 6.0, long, speaker))
            .collect()
    }

    fn inside_without_touching(found: &[(Option<u32>, f64, f64)], top: f64, bottom: f64) -> bool {
        let mut by_height: Vec<(f64, f64)> = found.iter().map(|row| (row.1, row.2)).collect();
        by_height.sort_by(|left, right| left.0.total_cmp(&right.0));
        by_height.windows(2).all(|pair| pair[0].1 <= pair[1].0)
            && found.iter().all(|row| row.1 >= top && row.2 <= bottom)
    }

    #[test]
    fn four_people_in_two_lines_fit_at_full_size() {
        let settings = anchored(Anchor::VideoBottom);
        let (cues, _) = build_cues(
            &four_long_voices(),
            &[],
            &FULL_HD,
            &settings,
            test_segmenter(),
        )
        .unwrap();
        let together: Vec<Cue> = cues.into_iter().filter(|cue| cue.start == 0.0).collect();
        assert_eq!(together.len(), 4);
        assert!(together.iter().all(|cue| cue.lines.len() == 2));
        assert!(together.iter().all(|cue| cue.scale == 1.0));
        let found = rows(&together, &settings, &FULL_HD);
        assert!(inside_without_touching(&found, 120.0, 960.0), "{found:?}");
    }

    #[test]
    fn four_people_stay_inside_a_small_landscape_and_a_vertical_frame() {
        let small = VideoInfo {
            width: 1280,
            height: 720,
            ..FULL_HD
        };
        let settings = anchored(Anchor::VideoBottom);
        let (cues, _) = build_cues(
            &four_long_voices(),
            &[],
            &small,
            &settings,
            test_segmenter(),
        )
        .unwrap();
        let together: Vec<Cue> = cues.into_iter().filter(|cue| cue.start == 0.0).collect();
        assert_eq!(together.len(), 4);
        let found = rows(&together, &settings, &small);
        assert!(inside_without_touching(&found, 80.0, 640.0), "{found:?}");
        let (cues, _) = build_cues(
            &four_long_voices(),
            &[],
            &VERTICAL,
            &settings,
            test_segmenter(),
        )
        .unwrap();
        let together: Vec<Cue> = cues.into_iter().filter(|cue| cue.start == 0.0).collect();
        assert_eq!(together.len(), 4);
        let found = rows(&together, &settings, &VERTICAL);
        assert!(inside_without_touching(&found, 120.0, 1800.0), "{found:?}");
    }

    #[test]
    fn four_people_in_a_very_large_font_shrink_to_stay_in_the_frame() {
        let settings = Settings {
            font_ratio: 0.2,
            ..anchored(Anchor::VideoBottom)
        };
        let (cues, _) = build_cues(
            &four_long_voices(),
            &[],
            &FULL_HD,
            &settings,
            test_segmenter(),
        )
        .unwrap();
        let together: Vec<Cue> = cues.into_iter().filter(|cue| cue.start == 0.0).collect();
        assert_eq!(together.len(), 4);
        assert!(together.iter().all(|cue| cue.scale < 1.0));
        let found = rows(&together, &settings, &FULL_HD);
        assert!(inside_without_touching(&found, 120.0, 960.0), "{found:?}");
    }

    /// Past four people the same logic runs and nothing is lost; how it looks is not promised.
    #[test]
    fn nine_people_keep_every_caption_and_a_track_each() {
        let words: Vec<Word> = (1..=9)
            .map(|speaker| spoken(0.0, 3.0, "そうですね", speaker))
            .collect();
        let settings = anchored(Anchor::VideoBottom);
        let (cues, _) = build_cues(&words, &[], &FULL_HD, &settings, test_segmenter()).unwrap();
        assert_eq!(
            cues.iter().map(|cue| cue.speaker).collect::<Vec<_>>(),
            (1..=9).map(Some).collect::<Vec<_>>()
        );
        assert_eq!(
            crate::export::frame_tracks(&cues, &FULL_HD),
            vec![0, 1, 2, 3, 4, 5, 6, 7, 8]
        );
    }

    #[test]
    fn rows_grow_away_from_a_top_anchor() {
        let words = [
            spoken(0.0, 3.0, "今日はいい天気です", 1),
            spoken(1.0, 4.0, "本当にそうですね", 2),
        ];
        let settings = anchored(Anchor::VideoTop);
        let (cues, _) = build_cues(&words, &[], &FULL_HD, &settings, test_segmenter()).unwrap();
        assert_eq!(
            rows(&cues, &settings, &FULL_HD),
            vec![(Some(1), 120.0, 212.0), (Some(2), 221.0, 313.0)]
        );
    }

    #[test]
    fn rows_grow_away_from_the_face() {
        let face = |top: f64, bottom: f64| -> Vec<FaceSample> {
            [0.0, 1.0, 2.0, 3.0, 4.0]
                .map(|time| FaceSample {
                    time,
                    top,
                    bottom,
                    center_x: 960.0,
                })
                .to_vec()
        };
        let words = [
            spoken(0.0, 3.0, "今日はいい天気です", 1),
            spoken(1.0, 4.0, "本当にそうですね", 2),
        ];
        let above = anchored(Anchor::FaceAbove);
        let low_face = face(600.0, 800.0);
        let (cues, _) = build_cues(&words, &low_face, &FULL_HD, &above, test_segmenter()).unwrap();
        let found = rows(&cues, &above, &FULL_HD);
        assert!(found[1].2 < found[0].1, "{found:?}");
        assert!(found[0].2 <= 350.0, "{found:?}");
        let below = anchored(Anchor::FaceBelow);
        let high_face = face(200.0, 400.0);
        let (cues, _) = build_cues(&words, &high_face, &FULL_HD, &below, test_segmenter()).unwrap();
        let found = rows(&cues, &below, &FULL_HD);
        assert!(found[1].1 > found[0].2, "{found:?}");
        assert_eq!(found[0].1, 460.0, "{found:?}");
    }

    #[test]
    fn rows_that_would_leave_the_frame_move_back_inside() {
        let words: Vec<Word> = (1..=4)
            .map(|speaker| spoken(0.0, 3.0, "そうですね", speaker))
            .collect();
        let settings = Settings {
            placement: Placement {
                anchor: Anchor::VideoTop,
                offset_x_percent: 0.0,
                offset_y_percent: 80.0,
            },
            ..fcpxml_settings()
        };
        let (cues, _) = build_cues(&words, &[], &FULL_HD, &settings, test_segmenter()).unwrap();
        let found = rows(&cues, &settings, &FULL_HD);
        assert!(found.iter().all(|row| row.2 <= 960.0), "{found:?}");
    }

    #[test]
    fn one_speaker_never_stacks_whatever_the_word_times_say() {
        let words = [
            spoken(0.0, 3.0, "今日はいい天気です", 1),
            spoken(1.0, 4.0, "本当にそうですね", 1),
        ];
        assert_eq!(speakers_apart(&words), None);
        let settings = anchored(Anchor::VideoBottom);
        let (cues, _) = build_cues(&words, &[], &FULL_HD, &settings, test_segmenter()).unwrap();
        assert!(cues.iter().all(|cue| cue.scale == 1.0));
        assert!(cues.windows(2).all(|pair| pair[0].end <= pair[1].start));
        assert!(rows(&cues, &settings, &FULL_HD)
            .iter()
            .all(|row| row.2 == 960.0));
        let saved = serde_json::to_value(&cues[0]).unwrap();
        assert_eq!(saved.get("scale"), None);
    }

    #[test]
    fn quick_turns_and_timing_noise_are_not_talking_together() {
        let turns = [
            spoken(0.0, 0.5, "はい", 1),
            spoken(0.5, 1.0, "ええ", 2),
            spoken(1.0, 1.5, "そう", 1),
            spoken(1.5, 2.0, "うん", 2),
        ];
        assert_eq!(speakers_apart(&turns), None);
        let noise = [
            spoken(0.0, 1.1, "今日は", 1),
            spoken(1.0, 2.0, "そうですね", 2),
        ];
        assert_eq!(speakers_apart(&noise), None);
    }

    #[test]
    fn only_the_stretch_of_talking_together_is_rearranged() {
        let words = [
            spoken(0.0, 1.0, "あ", 2),
            spoken(5.0, 7.0, "い", 2),
            spoken(5.5, 6.0, "う", 1),
            spoken(6.0, 8.0, "え", 2),
            spoken(20.0, 21.0, "お", 1),
        ];
        assert_eq!(
            speakers_apart(&words)
                .unwrap()
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>(),
            vec!["あ", "い", "え", "う", "お"]
        );
    }

    #[test]
    fn a_saved_cue_without_a_scale_reads_at_full_size() {
        let cue: Cue = serde_json::from_str(
            r#"{"start":0.0,"end":1.0,"speaker":1,"lines":["あ"],"x":1.0,"y":2.0}"#,
        )
        .unwrap();
        assert_eq!(cue.scale, 1.0);
    }

    #[test]
    fn nothing_groups_with_no_group() {
        assert!(!starts_new_group(&[], &word(0.0, 1.0, "は")));
        assert!(!joins_stray(&[], &[word(0.0, 1.0, "は")]));
    }

    #[test]
    fn a_short_exclamation_leads_the_next_words() {
        let words = [
            spoken(0.0, 0.3, "え?", 1),
            spoken(0.3, 2.0, "色々あります", 1),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(cues[0].lines, vec!["え?色々あります"]);
    }

    #[test]
    fn a_sentence_end_never_strands_a_small_kana() {
        let words = [
            spoken(0.0, 1.0, "うん。", 1),
            spoken(1.0, 2.0, "っていう", 1),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(cues[0].lines, vec!["うん。っていう"]);
    }

    #[test]
    fn a_whole_token_loop_is_dropped() {
        let words = [word(0.0, 1.0, "はい"), word(1.0, 2.0, "FFFFFFFF")];
        assert_eq!(
            display_words(&words, 60.0)
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>(),
            vec!["はい"]
        );
    }

    #[test]
    fn a_loop_inside_a_token_is_cut_out_when_dense() {
        let word_with_loop = word(0.0, 0.5, "ララララララララララなん");
        assert_eq!(without_loop(&word_with_loop), "なん");
    }

    #[test]
    fn a_slow_token_keeps_its_repeats() {
        let held = word(0.0, 5.0, "ララララララララララなん");
        assert_eq!(without_loop(&held), "ララララララララララなん");
    }

    #[test]
    fn repeated_small_vowels_still_lengthen_their_word() {
        let long = word(0.0, 0.5, "こぉぉぉぉぉぉぉぉぉ");
        assert_eq!(without_loop(&long), "こぉぉぉぉぉぉぉぉぉ");
    }

    #[test]
    fn a_token_that_is_only_a_loop_is_left_for_the_whole_token_check() {
        let only = word(0.0, 0.1, "ラララララララララ");
        assert_eq!(without_loop(&only), "ラララララララララ");
    }

    #[test]
    fn a_loop_cut_into_tokens_is_dropped_when_faster_than_speech() {
        let words = [
            word(0.0, 1.0, "はい"),
            word(1.0, 1.1, "ララ"),
            word(1.1, 1.2, "ラララ"),
            word(1.2, 1.3, "ララララ"),
            word(1.3, 2.0, "そう"),
        ];
        assert_eq!(
            display_words(&words, 60.0)
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>(),
            vec!["はい", "そう"]
        );
    }

    #[test]
    fn slow_repetition_is_singing_not_a_loop() {
        let words = [
            word(0.0, 1.0, "ラ"),
            word(1.0, 2.0, "ラ"),
            word(2.0, 3.0, "ラ"),
            word(3.0, 4.0, "ラ"),
        ];
        assert_eq!(display_words(&words, 60.0).len(), 4);
    }

    #[test]
    fn a_split_elongation_keeps_only_what_fits_two_lines() {
        let words = [
            word(0.0, 0.1, "ふ"),
            word(0.1, 0.2, "ぉぉぉぉぉぉぉぉぉぉ"),
            word(0.2, 0.3, "ぉぉぉぉぉぉぉぉぉぉ"),
            word(0.3, 0.4, "ぉぉぉぉぉぉぉぉぉぉ"),
        ];
        assert_eq!(
            display_words(&words, 60.0)
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>(),
            vec!["ふ", "ぉぉぉぉぉぉぉぉぉぉ", "ぉぉぉぉぉぉぉぉぉぉ"]
        );
    }

    #[test]
    fn marks_alone_are_dropped_but_lengthen_the_kana_before_them() {
        let words = [
            word(0.0, 1.0, "あ"),
            word(1.0, 1.1, "ー"),
            word(5.0, 5.1, "ー"),
            word(9.0, 9.1, "〜〜"),
        ];
        assert_eq!(
            display_words(&words, 60.0)
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>(),
            vec!["あ", "ー"]
        );
    }

    #[test]
    fn a_vowel_run_lengthens_through_earlier_marks_only_from_kana() {
        let words = [
            word(0.0, 1.0, "か"),
            word(1.0, 1.1, "ー"),
            word(1.1, 1.2, "ぁぁ"),
        ];
        assert!(is_word_mark(&words[..2], &words[2]));
        let latin = [word(0.0, 1.0, "OK"), word(1.0, 1.1, "ー")];
        assert!(!is_word_mark(&latin[..1], &latin[1]));
        let marks = [word(0.0, 0.1, "ー"), word(0.1, 0.2, "ー")];
        assert!(!is_word_mark(&marks[..1], &marks[1]));
    }

    #[test]
    fn a_stock_credits_phrase_is_dropped_except_at_the_close() {
        let words = [
            word(0.0, 1.0, "はい"),
            word(1.0, 2.0, "ご視聴"),
            word(2.0, 3.0, "ありがとう"),
            word(3.0, 4.0, "ございました。"),
        ];
        assert_eq!(
            display_words(&words, 600.0)
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>(),
            vec!["はい"]
        );
        assert_eq!(display_words(&words, 60.0).len(), 4);
    }

    #[test]
    fn a_stock_phrase_inside_a_longer_token_stays() {
        let words = [
            word(0.0, 1.0, "ご視聴ありがとうございましたね"),
            word(1.0, 2.0, "はい"),
        ];
        assert_eq!(display_words(&words, 600.0).len(), 2);
    }

    #[test]
    fn a_long_token_is_capped_at_the_longest_cue() {
        let words = [word(0.0, 30.0, "はい")];
        assert_eq!(display_words(&words, 60.0)[0].end, 7.0);
    }

    #[test]
    fn punctuation_alone_joins_the_word_before_it_or_disappears() {
        let words = [
            word(0.0, 1.0, "、"),
            word(1.0, 2.0, "はい"),
            word(2.0, 2.1, "。"),
        ];
        let shown = display_words(&words, 60.0);
        assert_eq!(
            shown.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
            vec!["はい。"]
        );
    }

    #[test]
    fn cues_stay_long_enough_to_read_and_never_overlap_the_next() {
        let words = [
            spoken(0.0, 0.1, "はい。", 1),
            spoken(0.7, 2.0, "いいえ。", 2),
            spoken(2.6, 2.7, "はい。", 1),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(
            cues.iter()
                .map(|cue| (cue.start, (cue.end * 1000.0).round()))
                .collect::<Vec<_>>(),
            vec![(0.0, 833.0), (0.8333333333333334, 2120.0), (2.6, 3433.0)]
        );
    }

    #[test]
    fn a_hold_lingers_past_its_speech_and_stops_before_stale() {
        let page = Page {
            start: 10.0,
            end: 11.0,
            speaker: None,
            lines: vec!["はい".to_owned()],
            narrow: Vec::new(),
        };
        assert_eq!(held_end(&page, 100.0, 30.0), 11.12);
        assert_eq!(held_end(&page, 11.2, 30.0), 11.12);
        assert_eq!((held_end(&page, 11.1, 30.0) * 1000.0).round(), 11033.0);
        assert_eq!(held_end(&page, 10.5, 30.0), 10.5);
    }

    #[test]
    fn a_long_text_holds_for_its_reading_time() {
        let page = Page {
            start: 0.0,
            end: 0.5,
            speaker: None,
            lines: vec![
                "あいうえおかきくけこ".to_owned(),
                "さしすせそたちつてと".to_owned(),
            ],
            narrow: Vec::new(),
        };
        assert_eq!((required_seconds(20, 0.0, 0.5) * 1000.0).round(), 1667.0);
        assert_eq!((held_end(&page, 100.0, 30.0) * 1000.0).round(), 1667.0);
    }

    #[test]
    fn a_cue_narrower_than_a_frame_gets_one_of_its_own() {
        let (start, end) = on_frames(1.01, 1.02, 0.0, 10.0);
        assert_eq!((start, (end * 10.0).round()), (1.01, 12.0));
        assert_eq!(on_frames(1.0, 2.0, 1.5, 10.0), (1.5, 2.0));
    }

    #[test]
    fn blank_words_make_no_cue() {
        let (cues, faceless) = build_cues(
            &[word(0.0, 1.0, " ")],
            &[],
            &FULL_HD,
            &fcpxml_settings(),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(cues, Vec::<Cue>::new());
        assert!(!faceless);
    }

    #[test]
    fn a_light_verb_is_never_cut_from_its_word_on_a_line() {
        let words = [
            spoken(0.0, 1.0, "方がいいかも", 1),
            spoken(1.0, 1.1, "も", 1),
            spoken(1.1, 1.3, "しか", 1),
            spoken(1.3, 1.5, "した", 1),
            spoken(1.5, 1.6, "ら", 1),
            spoken(1.6, 3.0, "私あられあんま入んなかった", 1),
        ];
        let (cues, _) = build_cues(
            &words,
            &[],
            &VERTICAL_SMALL,
            &fcpxml_settings(),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(
            cues.iter()
                .map(|cue| cue.lines.join("/"))
                .collect::<Vec<_>>(),
            vec!["方がいいかももしかしたら私/あられあんま入んなかった"]
        );
    }

    #[test]
    fn a_word_read_two_ways_never_ends_a_page_by_choice() {
        let words = [
            spoken(0.0, 1.5, "これ皆さんよく", 1),
            spoken(1.5, 1.7, "振", 1),
            spoken(1.7, 2.0, "って", 1),
            spoken(2.0, 2.3, "から", 1),
            spoken(2.3, 2.4, "い", 1),
            spoken(2.4, 2.6, "ある", 1),
            spoken(2.6, 3.0, "入れた", 1),
            spoken(3.0, 3.1, " ", 1),
            spoken(3.1, 6.0, "方がいいかもしかしたら", 1),
        ];
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &Settings {
                width_ratio: 0.75,
                ..fcpxml_settings()
            },
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(
            cues[0].lines.join("/"),
            "これ皆さんよく振ってからいある/入れた 方がいいかもしかしたら"
        );
    }

    #[test]
    fn a_predicate_stays_with_its_particle_across_whispers_space() {
        let words = [
            spoken(0.0, 2.8, "塗りがある名前も出たちゃんと", 1),
            spoken(2.8, 3.0, "チャ", 1),
            spoken(3.0, 3.1, "パ", 1),
            spoken(3.1, 3.3, "が", 1),
            spoken(3.3, 3.31, " ", 1),
            spoken(3.31, 3.7, "いる", 1),
            spoken(3.7, 3.9, "チャ", 1),
            spoken(3.9, 4.1, "ッ", 1),
            spoken(4.1, 4.3, "パ", 1),
            spoken(4.3, 7.0, "がチャッパがあります", 1),
        ];
        let (cues, _) =
            build_cues(&words, &[], &FULL_HD, &fcpxml_settings(), test_segmenter()).unwrap();
        assert_eq!(
            cues.iter()
                .map(|cue| cue.lines.join("/"))
                .collect::<Vec<_>>(),
            vec![
                "塗りがある名前も出た",
                "ちゃんとチャパが いるチャッパが/チャッパがあります"
            ]
        );
    }

    #[test]
    fn a_line_prefers_a_clause_to_a_slightly_evener_mid_phrase_break() {
        let metrics = Metrics {
            font_px: 10.0,
            outline_px: 1.0,
            max_line_px: 139.0,
            glyphs: None,
            rendered_scale: 1.0,
        };
        let group = [spoken(0.0, 3.0, "今日は天気がいいので、みんなで公園へ", 1)];
        let run = Run::new(&group, f64::INFINITY, &metrics, test_segmenter()).unwrap();
        assert_eq!(run.lay_out(0, 18).unwrap().split, Some(11));
    }

    #[test]
    fn a_timeline_is_laid_out_exactly_as_fcpxml() {
        let drt = Settings {
            subtitle_format: SubtitleFormat::Drt,
            ..fcpxml_settings()
        };
        assert_eq!(
            metrics_for(&VERTICAL_SMALL, &drt),
            metrics_for(&VERTICAL_SMALL, &fcpxml_settings())
        );
        assert_eq!(
            metrics_for(&FULL_HD, &drt),
            metrics_for(&FULL_HD, &fcpxml_settings())
        );
        assert_eq!(
            block_edge(50.0, 6.0, &drt),
            block_edge(50.0, 6.0, &fcpxml_settings())
        );
        let words = [spoken(0.0, 1.0, "はい", 1), spoken(1.2, 2.0, "どうも", 2)];
        let (with_drt, _) =
            build_cues(&words, &[], &VERTICAL_SMALL, &drt, test_segmenter()).unwrap();
        let (with_fcpxml, _) = build_cues(
            &words,
            &[],
            &VERTICAL_SMALL,
            &fcpxml_settings(),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(with_drt, with_fcpxml);
    }
}
