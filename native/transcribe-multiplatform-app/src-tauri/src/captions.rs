//! Turns timed words and speaker turns into positioned caption cues.
//!
//! Speaker rule from `replay.diarized_words`: a word gets a speaker only when one voice clearly
//! dominates its time span. Overlapping voices stay unassigned rather than guessed.
//!
//! Paging is the repo's static whole-phrase display (`caption_phrase_ass.phrase_cues`) over the
//! morphology of `replay.caption_linguistics`, with the word filters of `caption_timing` and the
//! hold of `caption_display_hold`. Its growing reveal and carried words need a renderer that shows
//! words one by one, so they have no place in a cue shown whole from its start.

use crate::engines::VideoInfo;
use crate::error::AppResult;
use crate::face::FaceSample;
use crate::morphology::{
    grapheme_ends, is_katakana_run, is_latin_or_number, Boundaries, BreakClass, PhraseEnd,
    Segmenter, DEPENDENT_KANA,
};
use crate::settings::{Anchor, Settings};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

/// Fraction of a word one speaker must cover, and the lead it needs over the next (diarized_words).
const DOMINANT_SHARE: f64 = 0.6;
const DOMINANT_MARGIN: f64 = 0.2;
/// Timing noise: a word just outside a turn belongs to the nearest speaker within this gap.
const SILENT_WORD_REACH_SECONDS: f64 = 1.0;
const CUE_BREAK_PAUSE_SECONDS: f64 = 0.7;
/// `caption_display_hold`: Netflix minimum and maximum event duration, and its reading rates.
const MAX_CUE_SECONDS: f64 = 7.0;
const MIN_DISPLAY: f64 = 5.0 / 6.0;
const LINGER: f64 = 0.5;
const CHAIN_GAP: f64 = 0.5;
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
const SENTENCE_ENDS: [char; 6] = ['。', '！', '？', '!', '?', '．'];
/// Kinsoku rules shared with `replay.speaker_subtitles` and `caption_linguistics`.
const NO_LINE_START: &str = "、。！？!?.,;:…」』）)]｝〜～";
const NO_LINE_END: &str = "「『（([｛";
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
const STOCK_PHRASES: [&str; 6] = [
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
const FORCED_PAGE_COST: f64 = 1000.0;

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

/// A cue's text and speech span, before placement and hold.
#[derive(Clone, Debug, PartialEq)]
struct Page {
    start: f64,
    end: f64,
    speaker: Option<u32>,
    lines: Vec<String>,
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
    let rim_px = font_px * settings.rim_ratio;
    let max_line_px =
        (f64::from(info.width) * settings.width_ratio - 2.0 * (outline_px + rim_px)).max(font_px);
    Metrics {
        font_px,
        outline_px,
        max_line_px,
    }
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
        match amounts
            .iter_mut()
            .find(|(speaker, _)| *speaker == turn.speaker)
        {
            Some((_, total)) => *total += share,
            None => amounts.push((turn.speaker, share)),
        }
    }
    amounts.sort_by(descending_share);
    let Some(&(top, top_share)) = amounts.first() else {
        return Voice::Silent;
    };
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

/// Words the diarizer heard nothing for, between two words of the same speaker, are that speaker's
/// quiet speech. Overlapped voices never take a neighbour's identity.
fn fill_quiet_runs(words: &mut [Word], voices: &[Voice]) {
    let mut previous = None;
    let mut run_start = 0;
    for index in 0..words.len() {
        let Some(speaker) = words[index].speaker else {
            continue;
        };
        if previous == Some(speaker) && !voices[run_start..index].contains(&Voice::Overlapped) {
            for word in &mut words[run_start..index] {
                word.speaker = Some(speaker);
            }
        }
        previous = Some(speaker);
        run_start = index + 1;
    }
}

/// Sets each word's speaker. Overlapped voices stay `None`; silent gaps are timing noise or quiet speech.
pub fn assign_speakers(words: &mut [Word], turns: &[SpeakerTurn]) {
    let voices: Vec<Voice> = words.iter().map(|word| voice_of(word, turns)).collect();
    for (word, voice) in words.iter_mut().zip(&voices) {
        word.speaker = match voice {
            Voice::Speaker(speaker) => Some(*speaker),
            Voice::Overlapped => None,
            Voice::Silent => nearest_speaker(word, turns),
        };
    }
    fill_quiet_runs(words, &voices);
}

fn glyph_width(c: char, font_px: f64) -> f64 {
    if c.is_ascii() {
        font_px * NARROW_GLYPH_EM
    } else if ('\u{FF61}'..='\u{FFDC}').contains(&c) {
        font_px * HALF_WIDTH_GLYPH_EM
    } else {
        font_px
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

/// Speech continuing the word before it: a small kana or long mark never opens a page.
fn leans_on_previous(word: &Word) -> bool {
    word.text
        .trim_start()
        .chars()
        .next()
        .is_some_and(|c| DEPENDENT_KANA.contains(c))
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
    if leans_on_previous(next) && gap <= SILENT_WORD_REACH_SECONDS {
        return false;
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

fn visible_characters<'a>(words: impl Iterator<Item = &'a Word>) -> usize {
    words
        .flat_map(|word| word.text.chars())
        .filter(|c| !c.is_whitespace())
        .count()
}

/// A short stretch the diarizer could not attribute, often one piece of a word (暮|ら|し), stays
/// with the neighbouring speaker's words on the page; the words themselves keep no speaker.
fn joins_stray(group: &[Word], next: &[Word]) -> bool {
    let (Some(last), Some(first)) = (group.last(), next.first()) else {
        return false;
    };
    let unattributed = |word: &&Word| word.speaker.is_none();
    let stray = visible_characters(group.iter().rev().take_while(unattributed))
        .max(visible_characters(next.iter().take_while(unattributed)));
    let speakers = [group, next].map(|words| words.iter().find_map(|word| word.speaker));
    let too_brief = next.iter().last().map_or(0.0, |word| word.end) - first.start
        < SHORTEST_GROUP_SECONDS
        || last.end - group.first().map_or(last.start, |word| word.start) < SHORTEST_GROUP_SECONDS;
    (stray > 0 && stray <= WRAP_ORPHAN_MAXIMUM || too_brief)
        && (speakers[0].is_none() || speakers[1].is_none() || speakers[0] == speakers[1])
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
            widths.push(widths[widths.len() - 1] + glyph_width(*c, metrics.font_px));
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
    }

    /// The most balanced legal two-line split among `points` (`wrap_measured_lines`).
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
                (left.max(right) <= self.metrics.max_line_px)
                    .then_some((point, (left - right).abs()))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(point, _)| point)
    }

    /// Lines of one page: phrase ends, then intact morphemes, then characters for a single long word.
    fn lay_out(&self, start: usize, end: usize) -> Option<Layout> {
        let (start, end) = self.trimmed(start, end);
        if self.width(start, end) <= self.metrics.max_line_px {
            return Some(Layout {
                split: None,
                penalty: 0.0,
            });
        }
        if let Some(point) = self.balanced_split(&self.boundaries.line_ends, start, end, true) {
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
        let class_cost = match phrase.class {
            BreakClass::Clause => 0.0,
            BreakClass::Closing => CLOSING_BREAK_COST,
            BreakClass::Connective => CONNECTIVE_BREAK_COST,
            BreakClass::Particle => PARTICLE_BREAK_COST,
            BreakClass::Plain => PLAIN_BREAK_COST,
        };
        if phrase.offset >= self.chars.len() {
            return 0.0;
        }
        let gap = self.times[phrase.offset].0 - self.times[phrase.offset - 1].1;
        if gap >= PHRASE_GAP_SECONDS {
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
        let levels = [
            (&self.boundaries.atomic, ATOMIC_BREAK_COST),
            (&self.boundaries.morphemes, MORPHEME_BREAK_COST),
            (&self.boundaries.overflow, OVERFLOW_BREAK_COST),
            (&self.graphemes, CHARACTER_BREAK_COST),
        ];
        for (points, cost) in levels {
            let offsets: Vec<usize> = breaks.keys().copied().collect();
            let tight: Vec<&[usize]> = offsets
                .windows(2)
                .filter(|span| !self.fits(span[0], span[1]))
                .collect();
            let added = points
                .iter()
                .filter(|point| self.may_break_line(**point))
                .filter(|point| {
                    tight
                        .iter()
                        .any(|span| span[0] < **point && **point < span[1])
                });
            let added: Vec<usize> = added.copied().collect();
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
        if !self.within_budget(start, end) {
            return FORCED_PAGE_COST;
        }
        let Some(layout) = self.lay_out(start, end) else {
            return FORCED_PAGE_COST;
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
        // In full-width character widths, so a narrow Latin letter counts for what it is.
        let shown_ems = self.width(start, end) / self.metrics.font_px;
        let rate = shown_ems / seconds.max(SHORTEST_SPAN_SECONDS);
        let fast = (rate / FAST_READING_CPS - 1.0).max(0.0) * FAST_PAGE_COST;
        PAGE_COST + fill * fill + tiny + flash + fast + layout.penalty
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
                Page {
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
    let (start, end) = (page.start, page.end);
    let characters: usize = page
        .lines
        .iter()
        .map(|line| line.chars().filter(|c| !c.is_whitespace()).count())
        .sum();
    // Past `MAX_LINGER` only as far as the fast reading rate needs, never past the next cue.
    let readable = start + characters as f64 / FAST_READING_CPS;
    let latest = (end + MIN_DISPLAY)
        .max(readable)
        .min(start + MAX_CUE_SECONDS)
        .min(following - MIN_GAP_FRAMES / rate);
    if latest <= end {
        return end.min(following).max(start + SHORTEST_SPAN_SECONDS);
    }
    let wanted = (start + required_seconds(characters, start, end))
        .max(end + LINGER)
        .min(latest);
    if following - wanted < CHAIN_GAP {
        latest
    } else {
        wanted
    }
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
    metrics.font_px * (EM_LINE_HEIGHT + line_count.saturating_sub(1) as f64 * settings.line_height)
}

fn block_width(lines: &[String], metrics: &Metrics, settings: &Settings) -> f64 {
    let widest = lines
        .iter()
        .map(|line| {
            line.chars()
                .map(|c| glyph_width(c, metrics.font_px))
                .sum::<f64>()
        })
        .fold(0.0, f64::max);
    widest + 2.0 * (metrics.outline_px + settings.rim_ratio * metrics.font_px)
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
        let free_left = zone.left - self.gap;
        let free_right = self.frame_w - zone.right - self.gap;
        if free_left >= free_right && width <= free_left {
            return Some((free_left / 2.0, low));
        }
        if free_right > free_left && width <= free_right {
            return Some((self.frame_w - free_right / 2.0, low));
        }
        let high = zone.top - self.gap - height;
        (high >= self.padding).then_some((self.frame_w / 2.0, high + height / 2.0))
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

fn following_silence(group: &[Word], next: Option<&Vec<Word>>) -> f64 {
    let last_end = group.last().map_or(0.0, |word| word.end);
    next.and_then(|words| words.first())
        .map_or(f64::INFINITY, |word| word.start - last_end)
}

/// Builds positioned cues from timed words. The returned flag reports a face anchor that found no
/// face; words stay as they are, so a transcript can be laid out again.
pub fn build_cues(
    words: &[Word],
    faces: &[FaceSample],
    info: &VideoInfo,
    settings: &Settings,
    segmenter: &Segmenter,
) -> AppResult<(Vec<Cue>, bool)> {
    let metrics = metrics_for(info, settings);
    let placement = settings.placement;
    let (frame_w, frame_h) = (f64::from(info.width), f64::from(info.height));
    let margin = MARGIN_PX * f64::from(info.width.min(info.height)) / REFERENCE_SHORT_EDGE;
    let wants_face = matches!(placement.anchor, Anchor::FaceAbove | Anchor::FaceBelow);
    let mut placer = FacePlacer::new(faces, info, &metrics, settings);
    let rate = f64::from(info.fps_num) / f64::from(info.fps_den);
    let groups = merge_strays(group_words(&display_words(words, info.duration)));
    let mut pages: Vec<Page> = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        let following_gap = following_silence(group, groups.get(index + 1));
        pages.extend(Run::new(group, following_gap, &metrics, segmenter)?.pages());
    }
    let mut cues: Vec<Cue> = Vec::with_capacity(pages.len());
    let mut faceless = false;
    for (index, page) in pages.iter().enumerate() {
        let following = pages
            .get(index + 1)
            .map_or(info.duration, |next| next.start);
        let previous_end = cues.last().map_or(0.0, |cue| cue.end);
        let (start, end) = on_frames(
            page.start,
            held_end(page, following, rate),
            previous_end,
            rate,
        );
        let lines = page.lines.clone();
        let height = block_height(lines.len(), &metrics, settings);
        let width = block_width(&lines, &metrics, settings);
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
        let x = clamp_center(
            center_x + placement.offset_x_percent / 100.0 * frame_w,
            width,
            frame_w,
        );
        let y = clamp_center(
            center_y + placement.offset_y_percent / 100.0 * frame_h,
            height,
            frame_h,
        );
        cues.push(Cue {
            start,
            end,
            speaker: page.speaker,
            lines,
            x,
            y,
        });
    }
    Ok((cues, faceless))
}

#[cfg(test)]
mod tests {
    use super::*;
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
    };
    const VERTICAL: VideoInfo = VideoInfo {
        width: 1080,
        height: 1920,
        duration: 60.0,
        fps_num: 30,
        fps_den: 1,
    };
    const VERTICAL_SMALL: VideoInfo = VideoInfo {
        width: 720,
        height: 1280,
        duration: 60.0,
        fps_num: 30,
        fps_den: 1,
    };
    const UHD: VideoInfo = VideoInfo {
        width: 3840,
        height: 2160,
        duration: 60.0,
        fps_num: 30,
        fps_den: 1,
    };

    fn at(anchor: Anchor, x: f64, y: f64) -> Settings {
        Settings {
            placement: Placement {
                anchor,
                offset_x_percent: x,
                offset_y_percent: y,
            },
            ..Settings::default()
        }
    }

    #[test]
    fn caption_size_and_line_width_follow_the_resolution() {
        let settings = Settings::default();
        assert_eq!(metrics_for(&FULL_HD, &settings).font_px, 52.0);
        assert_eq!(metrics_for(&UHD, &settings).font_px, 104.0);
        assert_eq!(metrics_for(&VERTICAL, &settings).font_px, 52.0);
        assert_eq!(metrics_for(&FULL_HD, &settings).max_line_px.round(), 1712.0);
        assert_eq!(metrics_for(&UHD, &settings).max_line_px.round(), 3425.0);
        assert_eq!(metrics_for(&VERTICAL, &settings).max_line_px.round(), 956.0);
        assert_eq!(
            metrics_for(&VERTICAL_SMALL, &settings).max_line_px.round(),
            638.0
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
    fn overlapping_voices_are_not_guessed() {
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
        assert_eq!(words[0].speaker, None);
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
    fn a_word_far_from_any_speech_stays_unassigned() {
        let mut words = [word(9.0, 9.1, "ね")];
        assign_speakers(
            &mut words,
            &[SpeakerTurn {
                start: 0.0,
                end: 5.0,
                speaker: 3,
            }],
        );
        assert_eq!(words[0].speaker, None);
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
    fn quiet_words_between_two_speakers_stay_unassigned() {
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
        assert_eq!(words[1].speaker, None);
    }

    #[test]
    fn overlapped_words_never_take_a_neighbours_speaker() {
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
        assert_eq!(words[1].speaker, None);
    }

    #[test]
    fn speaker_changes_start_a_new_cue() {
        let words = [spoken(0.0, 1.0, "はい", 1), spoken(1.1, 2.0, "いいえ", 2)];
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].speaker, Some(1));
        assert_eq!(cues[1].speaker, Some(2));
    }

    #[test]
    fn pauses_and_sentence_ends_split_cues() {
        let words = [
            spoken(0.0, 1.0, "はい", 1),
            spoken(2.0, 3.0, "そう。", 1),
            spoken(3.1, 4.0, "次", 1),
        ];
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(
            cues.iter()
                .map(|cue| cue.lines.join(""))
                .collect::<Vec<_>>(),
            vec!["はい", "そう。", "次"]
        );
    }

    #[test]
    fn punctuation_stays_with_its_sentence() {
        let words = [spoken(0.0, 1.0, "はい", 1), spoken(5.0, 5.1, "。", 2)];
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
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
        assert_eq!((bottom[0].x, bottom[0].y.round()), (960.0, 913.0));
        assert_eq!((top[0].x, top[0].y.round()), (960.0, 167.0));
    }

    #[test]
    fn margins_scale_with_the_short_edge_on_every_frame() {
        let words = [spoken(0.0, 1.0, "はい", 1)];
        let settings = at(Anchor::VideoBottom, 0.0, 0.0);
        let (tall, _) =
            build_cues(&words, &[], &VERTICAL_SMALL, &settings, test_segmenter()).unwrap();
        let (large, _) = build_cues(&words, &[], &VERTICAL, &settings, test_segmenter()).unwrap();
        let (uhd, _) = build_cues(&words, &[], &UHD, &settings, test_segmenter()).unwrap();
        assert_eq!((tall[0].x, tall[0].y.round()), (360.0, 1168.0));
        assert_eq!((large[0].x, large[0].y.round()), (540.0, 1753.0));
        assert_eq!((uhd[0].x, uhd[0].y.round()), (1920.0, 1826.0));
    }

    #[test]
    fn a_second_line_grows_by_the_line_height_setting() {
        let metrics = metrics_for(&FULL_HD, &Settings::default());
        assert_eq!(
            block_height(1, &metrics, &Settings::default()).round(),
            94.0
        );
        assert_eq!(
            block_height(2, &metrics, &Settings::default()).round(),
            169.0
        );
        let settings = Settings {
            line_height: 2.0,
            ..Settings::default()
        };
        assert_eq!(block_height(2, &metrics, &settings).round(), 198.0);
    }

    #[test]
    fn a_block_is_as_wide_as_its_longest_line_with_the_outline_and_rim() {
        let metrics = Metrics {
            font_px: 10.0,
            outline_px: 1.0,
            max_line_px: 500.0,
        };
        let settings = Settings {
            rim_ratio: 0.5,
            ..Settings::default()
        };
        let width = block_width(&["あい".to_owned(), "a".to_owned()], &metrics, &settings);
        assert_eq!(width, 32.0);
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
        assert_eq!((cues[0].x, cues[0].y.round()), (1152.0, 805.0));
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
        assert_eq!(cues[0].y.round(), 1033.0);
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
        assert_eq!((above[0].x, above[0].y.round()), (960.0, 368.0));
        assert_eq!((below[0].x, below[0].y.round()), (960.0, 829.0));
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
        assert_eq!(cues[0].y.round(), 913.0);
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
            &metrics_for(&FULL_HD, &Settings::default()),
            &Settings::default(),
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
        assert_eq!(cues[0].y.round(), 368.0);
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
            vec![368.0, 507.0, 468.0]
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
            Some((180.0, 1009.0))
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
        let settings = Settings::default();
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
            Some((470.0, 1009.0))
        );
        assert_eq!(
            placer.free_center(&zone(300.0, 800.0), (400.0, 94.0)),
            Some((1390.0, 1009.0))
        );
        assert_eq!(
            placer.free_center(&zone(300.0, 1600.0), (400.0, 94.0)),
            None
        );
    }

    #[test]
    fn a_block_too_wide_for_the_side_may_use_the_top_above_the_face_box() {
        let settings = Settings::default();
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
        let settings = Settings::default();
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
        assert_eq!(cues[0].y.round(), 649.0);
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
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
        let metrics = metrics_for(&FULL_HD, &Settings::default());
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
        let group = [word(0.0, 0.3, "はいそうですありがとう")];
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
    fn a_leading_voice_below_the_dominant_share_is_not_guessed() {
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
        assert_eq!(words[0].speaker, None);
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
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
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
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].lines, vec!["これは字幕です"]);
    }

    #[test]
    fn glyph_widths_follow_the_safe_wrap_estimate() {
        assert_eq!(glyph_width('a', 10.0), 6.0);
        assert_eq!(glyph_width('ｱ', 10.0), 5.0);
        assert_eq!(glyph_width('あ', 10.0), 10.0);
    }

    #[test]
    fn a_page_ends_at_a_clause_rather_than_filling_the_width() {
        let words = [
            spoken(0.0, 3.0, "今日は天気がいいので、", 1),
            spoken(3.0, 5.0, "みんなで公園に行って", 1),
            spoken(5.0, 7.0, "お弁当を食べました", 1),
        ];
        let (cues, _) = build_cues(
            &words,
            &[],
            &VERTICAL,
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(
            cues.iter().map(|cue| cue.lines.clone()).collect::<Vec<_>>(),
            vec![
                vec!["今日は天気がいいので、"],
                vec!["みんなで公園に行って", "お弁当を食べました"],
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
        let settings = Settings::default();
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
            ..Settings::default()
        };
        let (normal, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
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
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
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
        let (wide, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
        let (tall, _) = build_cues(
            &words,
            &[],
            &VERTICAL_SMALL,
            &Settings::default(),
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
            max_line_px: 62.0,
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
        let (cues, _) = build_cues(&words, &[], &FULL_HD, &Settings::default(), &missing).unwrap();
        assert_eq!(cues[0].lines, vec!["Hello world, this is a caption"]);
    }

    #[test]
    fn a_blank_run_makes_no_page() {
        let metrics = Metrics {
            font_px: 10.0,
            outline_px: 1.0,
            max_line_px: 12.0,
        };
        let group = [spoken(0.0, 1.0, "   ", 1)];
        let run = Run::new(&group, f64::INFINITY, &metrics, test_segmenter()).unwrap();
        assert_eq!(run.pages(), Vec::<Page>::new());
    }

    #[test]
    fn a_missing_dictionary_is_an_error_for_japanese() {
        let missing = Segmenter::new(PathBuf::from("/nonexistent/dictionary"));
        let words = [spoken(0.0, 1.0, "はい", 1)];
        let error = build_cues(&words, &[], &FULL_HD, &Settings::default(), &missing).unwrap_err();
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
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
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
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(
            cues.iter()
                .map(|cue| cue.lines.join(""))
                .collect::<Vec<_>>(),
            vec!["今日は天気が", "良いので公園へ", "行きました"]
        );
    }

    #[test]
    fn a_single_phrase_cannot_outlast_the_longest_cue() {
        let words = [
            spoken(0.0, 6.0, "サン", 1),
            spoken(6.0, 12.0, "フラン", 1),
            spoken(12.0, 18.0, "シスコ", 1),
        ];
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(
            cues.iter()
                .map(|cue| (cue.start, cue.lines.join("")))
                .collect::<Vec<_>>(),
            vec![
                (0.0, "サン".to_owned()),
                (6.0, "フラン".to_owned()),
                (12.0, "シスコ".to_owned())
            ]
        );
    }

    #[test]
    fn a_page_ends_where_the_speaker_really_pauses() {
        let words = [
            spoken(0.0, 1.0, "そうですね", 1),
            spoken(1.3, 2.3, "今日は", 1),
            spoken(2.3, 3.3, "公園に", 1),
        ];
        let group = group_words(&words);
        let metrics = metrics_for(&FULL_HD, &Settings::default());
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
        let metrics = metrics_for(&FULL_HD, &Settings::default());
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

    #[test]
    fn stray_words_of_no_speaker_join_the_speakers_words_on_the_page() {
        let words = [
            spoken(0.0, 0.5, "二人暮", 1),
            word(0.5, 0.7, "ら"),
            spoken(0.7, 1.0, "しで", 1),
        ];
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(cues[0].lines, vec!["二人暮らしで"]);
        assert_eq!(cues[0].speaker, Some(1));
        assert_eq!(words[1].speaker, None);
    }

    #[test]
    fn a_stray_never_bridges_two_speakers() {
        let words = [
            spoken(0.0, 0.5, "はい", 1),
            word(0.5, 0.7, "ん"),
            spoken(0.7, 1.0, "いいえ", 2),
        ];
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(
            cues.iter()
                .map(|cue| (cue.lines.join(""), cue.speaker))
                .collect::<Vec<_>>(),
            vec![
                ("はいん".to_owned(), Some(1)),
                ("いいえ".to_owned(), Some(2))
            ]
        );
    }

    #[test]
    fn a_long_stretch_of_overlap_keeps_its_own_cue() {
        let words = [
            spoken(0.0, 1.0, "はいそうです", 1),
            word(1.0, 2.0, "いろいろあります"),
        ];
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(
            cues.iter().map(|cue| cue.speaker).collect::<Vec<_>>(),
            vec![Some(1), None]
        );
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
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(cues[0].lines, vec!["え?色々あります"]);
    }

    #[test]
    fn a_sentence_end_never_strands_a_small_kana() {
        let words = [
            spoken(0.0, 1.0, "うん。", 1),
            spoken(1.0, 2.0, "っていう", 1),
        ];
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
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
            spoken(0.0, 0.1, "あ", 1),
            spoken(0.3, 2.0, "い", 2),
            spoken(2.0, 2.1, "う", 1),
        ];
        let (cues, _) = build_cues(
            &words,
            &[],
            &FULL_HD,
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(
            cues.iter()
                .map(|cue| (cue.start, (cue.end * 1000.0).round()))
                .collect::<Vec<_>>(),
            vec![(0.0, 233.0), (0.3, 2000.0), (2.0, 2833.0)]
        );
    }

    #[test]
    fn a_hold_lingers_past_its_speech_and_stops_before_stale() {
        let page = Page {
            start: 10.0,
            end: 11.0,
            speaker: None,
            lines: vec!["はい".to_owned()],
        };
        assert_eq!(held_end(&page, 100.0, 30.0), 11.5);
        assert_eq!((held_end(&page, 11.2, 30.0) * 1000.0).round(), 11133.0);
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
            &Settings::default(),
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(cues, Vec::<Cue>::new());
        assert!(!faceless);
    }
}
