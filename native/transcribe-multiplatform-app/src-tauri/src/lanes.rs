//! Rows for voices talking at once: the mix around each overlap goes through MossFormer2, each track
//! through Whisper again, and the tracks replace the first pass's text there, one row per voice.

use crate::captions::{SpeakerTurn, Word, STOCK_PHRASES};
use crate::engines::WhisperClip;
use crate::error::AppResult;
use crate::morphology::{is_japanese, ReadingChar, Segmenter};
use crate::separation::{cut_clips, SeparationClip};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Speakers' overlaps this close are one exchange. Measured at 2 s; not yet derived per video.
const JOIN_SECONDS: f64 = 2.0;
/// Less overlap than this in a range rarely gives two rows. Two hours of three people: under it 26% of the
/// ranges' audio gave 10% of the ranges with rows.
const SHORTEST_OVERLAP_SECONDS: f64 = 0.5;
/// Every row must share this much with the first pass, or the range stays in the first pass's text. Of the
/// same hours' rows, those above it hold 74% of the good ones and 41% of the bad ones.
const ROW_SUPPORT: f64 = 0.5;
/// A text this much taken by one repeated unit is Whisper stuck in a loop, not speech.
const LOOP_SHARE: f64 = 0.5;
const LOOP_MINIMUM_REPEATS: usize = 3;
const LONG_UNIT_READINGS: usize = 4;
const LONGEST_UNIT_READINGS: usize = 20;
/// Longer ranges are divided at a phrase boundary: as many voices are found as with longer ones, on a
/// third of the memory (2.35 GB for 8 s, 7.25 GB for 30 s).
const LONGEST_RANGE_SECONDS: f64 = 8.0;
/// More than this with three people at once sends both tracks through the model again.
const DEEP_SECONDS: f64 = 0.5;
/// A track sharing less than this with the first pass's text is noise the first pass could not hear.
const FIRST_PASS_SUPPORT: f64 = 0.05;
/// Two tracks sharing this much say the same thing.
const SAME_TEXT_SHARE: f64 = 0.72;
/// Runs of readings shorter than this agree by chance.
const SHORTEST_RUN: usize = 3;
/// A deep track with fewer readings says nothing.
const SHORTEST_DEEP_TRACK: usize = 2;
/// How far past a stock phrase a track may run and still be only that phrase.
const STOCK_PHRASE_SLACK: usize = 2;
const STOCK_ONLY_EXTRAS: [&str; 2] = ["おやすみなさい", "お疲れ様でした"];
/// A row's first and last readings are compared with the first pass just outside the range.
const EDGE_READINGS: usize = 12;
const NEIGHBOUR_SECONDS: f64 = 3.0;
const FIRST_TRACKS: usize = 2;

/// What the rows did to one job, reported with its outcome.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct LaneStats {
    pub ranges: usize,
    pub deep_ranges: usize,
    pub range_seconds: f64,
    /// How many ranges ended with this many rows.
    pub rows: BTreeMap<usize, usize>,
    pub same_text: usize,
    pub dropped_tracks: usize,
    pub first_pass_kept: usize,
    /// Ranges with too little overlap to send to the model.
    pub skipped_ranges: usize,
    /// Ranges kept in the first pass's text because a row could not be trusted.
    pub unsure_ranges: usize,
    pub separation_seconds: f64,
    pub transcription_seconds: f64,
}

/// Whisper over many clips, with the words each one held.
pub type TranscribeStep<'a> = dyn FnMut(&[WhisperClip]) -> AppResult<Vec<Vec<Word>>> + 'a;

/// The two long-running steps, passed in so the logic around them can be tested without the models.
pub struct LaneEngine<'a> {
    /// Whether three people at once may go through the model twice.
    pub deep: bool,
    pub separate: &'a mut dyn FnMut(&[SeparationClip]) -> AppResult<()>,
    pub transcribe: &'a mut TranscribeStep<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Span {
    start: f64,
    end: f64,
}

/// Where at least `people` different speakers talk at once, in time order.
fn spans_with(turns: &[SpeakerTurn], people: usize) -> Vec<Span> {
    let mut points: Vec<f64> = turns
        .iter()
        .flat_map(|turn| [turn.start, turn.end])
        .collect();
    points.sort_by(f64::total_cmp);
    points.dedup();
    let mut ordered: Vec<&SpeakerTurn> = turns.iter().collect();
    ordered.sort_by(|left, right| left.start.total_cmp(&right.start));
    let mut spans: Vec<Span> = Vec::new();
    let mut active: Vec<&SpeakerTurn> = Vec::new();
    let mut next = 0;
    for pair in points.windows(2) {
        while next < ordered.len() && ordered[next].start <= pair[0] {
            active.push(ordered[next]);
            next += 1;
        }
        active.retain(|turn| turn.end >= pair[1]);
        let mut speakers: Vec<u32> = active.iter().map(|turn| turn.speaker).collect();
        speakers.sort_unstable();
        speakers.dedup();
        if speakers.len() >= people {
            match spans.last_mut() {
                Some(last) if last.end >= pair[0] => last.end = pair[1],
                _ => spans.push(Span {
                    start: pair[0],
                    end: pair[1],
                }),
            }
        }
    }
    spans
}

fn joined(spans: Vec<Span>, reach: f64) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    for span in spans {
        match out.last_mut() {
            Some(last) if span.start - last.end <= reach => last.end = last.end.max(span.end),
            _ => out.push(span),
        }
    }
    out
}

/// The times of phrase ends in Japanese and of word ends elsewhere, in order.
fn boundary_times(words: &[Word], segmenter: &Segmenter) -> AppResult<Vec<f64>> {
    let text: String = words.iter().map(|word| word.text.as_str()).collect();
    if !is_japanese(&text) {
        return Ok(words.iter().map(|word| word.end).collect());
    }
    let ends: Vec<usize> = words
        .iter()
        .scan(0, |offset, word| {
            *offset += word.text.chars().count();
            Some(*offset)
        })
        .collect();
    let phrases = segmenter.boundaries(&text)?.phrases;
    Ok(phrases
        .iter()
        .filter_map(|phrase| {
            let word = ends.partition_point(|end| *end < phrase.offset);
            words.get(word).map(|word| word.end)
        })
        .collect())
}

/// Each span out to the boundaries around it, ranges that then touch as one, and long ones divided.
fn ranges_around(spans: &[Span], boundaries: &[f64], duration: f64) -> Vec<Span> {
    let widened = spans.iter().map(|span| {
        let before = boundaries.partition_point(|time| *time <= span.start);
        let after = boundaries.partition_point(|time| *time < span.end);
        Span {
            start: before.checked_sub(1).map_or(0.0, |at| boundaries[at]),
            end: boundaries.get(after).copied().unwrap_or(duration),
        }
    });
    joined(widened.collect(), 0.0)
        .into_iter()
        .flat_map(|range| divided(range, boundaries))
        .collect()
}

fn divided(range: Span, boundaries: &[f64]) -> Vec<Span> {
    let mut parts = Vec::new();
    let mut start = range.start;
    while range.end - start > LONGEST_RANGE_SECONDS {
        let limit = start + LONGEST_RANGE_SECONDS;
        let last = boundaries.partition_point(|time| *time <= limit);
        let cut = last
            .checked_sub(1)
            .map(|at| boundaries[at])
            .filter(|time| *time > start)
            .unwrap_or(limit);
        parts.push(Span { start, end: cut });
        start = cut;
    }
    parts.push(Span {
        start,
        end: range.end,
    });
    parts
}

fn seconds_inside(span: Span, others: &[Span]) -> f64 {
    others
        .iter()
        .map(|other| (other.end.min(span.end) - other.start.max(span.start)).max(0.0))
        .sum()
}

fn middle(word: &Word) -> f64 {
    (word.start + word.end) / 2.0
}

fn text_of(words: &[Word]) -> String {
    words.iter().map(|word| word.text.as_str()).collect()
}

fn sounds(text: &str, segmenter: &Segmenter) -> AppResult<Vec<char>> {
    Ok(segmenter
        .readings(text)?
        .iter()
        .map(|item| item.reading)
        .collect())
}

/// The longest common runs, longest first then leftmost, as `(left, right, length)` (`difflib`).
fn matching_runs(left: &[char], right: &[char]) -> Vec<(usize, usize, usize)> {
    let mut lengths = vec![0usize; right.len() + 1];
    let mut best = (0, 0, 0);
    for (at_left, l) in left.iter().enumerate() {
        for at_right in (0..right.len()).rev() {
            lengths[at_right + 1] = if *l == right[at_right] {
                lengths[at_right] + 1
            } else {
                0
            };
            let length = lengths[at_right + 1];
            let start = (at_left + 1 - length.min(at_left + 1), at_right + 1 - length);
            if length > best.2 || (length == best.2 && length > 0 && start < (best.0, best.1)) {
                best = (start.0, start.1, length);
            }
        }
        lengths[0] = 0;
    }
    if best.2 == 0 {
        return Vec::new();
    }
    let (at_left, at_right, length) = best;
    let mut runs = matching_runs(&left[..at_left], &right[..at_right]);
    runs.push(best);
    runs.extend(
        matching_runs(&left[at_left + length..], &right[at_right + length..])
            .into_iter()
            .map(|(a, b, size)| (a + at_left + length, b + at_right + length, size)),
    );
    runs
}

fn shared_readings(left: &[char], right: &[char]) -> usize {
    matching_runs(left, right)
        .iter()
        .map(|run| run.2)
        .filter(|size| *size >= SHORTEST_RUN)
        .sum()
}

/// Share of `track`'s readings also said in `other`.
fn support(track: &[char], other: &[char]) -> f64 {
    shared_readings(track, other) as f64 / track.len().max(1) as f64
}

/// Share of the shorter text that the longer one also says.
fn same_share(left: &[char], right: &[char]) -> f64 {
    shared_readings(left, right) as f64 / left.len().min(right.len()).max(1) as f64
}

fn is_only_stock_phrase(track: &[char], segmenter: &Segmenter) -> AppResult<bool> {
    for phrase in STOCK_PHRASES.iter().chain(&STOCK_ONLY_EXTRAS) {
        let phrase = sounds(phrase, segmenter)?;
        if track.len() <= phrase.len() + STOCK_PHRASE_SLACK
            && track.windows(phrase.len()).any(|part| part == phrase)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// How much of `text` is one unit said again and again: three times, or twice when the unit is long.
fn loop_share(text: &[char]) -> f64 {
    let mut longest = 0;
    for unit in 1..=LONGEST_UNIT_READINGS {
        let mut at = 0;
        while at + unit * 2 <= text.len() {
            let mut count = 1;
            while text[at..at + unit]
                == *text
                    .get(at + count * unit..at + (count + 1) * unit)
                    .unwrap_or_default()
            {
                count += 1;
            }
            if count >= LOOP_MINIMUM_REPEATS || (count >= 2 && unit >= LONG_UNIT_READINGS) {
                longest = longest.max(count * unit);
            }
            at += if count == 1 { 1 } else { count * unit };
        }
    }
    longest as f64 / text.len().max(1) as f64
}

fn loops(text: &[char]) -> bool {
    loop_share(text) >= LOOP_SHARE
}

/// One track of one range, as Whisper heard it.
#[derive(Clone)]
struct Track {
    words: Vec<Word>,
    sound: Vec<char>,
}

/// Which tracks become rows, and what happened to the others.
struct Choice {
    rows: Vec<Track>,
    dropped: usize,
    same: usize,
    unsure: bool,
}

fn choose_rows(
    tracks: Vec<Track>,
    first_sound: &[char],
    deep: bool,
    segmenter: &Segmenter,
) -> AppResult<Choice> {
    // A first pass stuck in a loop says nothing about the tracks, and is no text to fall back on.
    let first_is_sound = !loops(first_sound);
    let by_support = first_is_sound && !deep;
    let count = tracks.len();
    let mut kept = Vec::with_capacity(count);
    for track in tracks {
        let keep = !loops(&track.sound)
            && if by_support {
                support(&track.sound, first_sound) >= FIRST_PASS_SUPPORT
            } else {
                track.sound.len() >= SHORTEST_DEEP_TRACK
                    && !is_only_stock_phrase(&track.sound, segmenter)?
            };
        if keep {
            kept.push(track);
        }
    }
    let dropped = count - kept.len();
    kept.sort_by_key(|track| std::cmp::Reverse(track.sound.len()));
    let mut rows: Vec<Track> = Vec::with_capacity(kept.len());
    let mut same = 0;
    for track in kept {
        if rows
            .iter()
            .any(|row| same_share(&row.sound, &track.sound) >= SAME_TEXT_SHARE)
        {
            same += 1;
        } else {
            rows.push(track);
        }
    }
    // One voice left, or a row that shares little with what was heard: the first pass reads better.
    let unsure = by_support
        && !rows.is_empty()
        && (rows.len() == 1
            || rows
                .iter()
                .any(|row| support(&row.sound, first_sound) < ROW_SUPPORT));
    if unsure || (first_is_sound && same > 0 && rows.len() == 1) {
        rows.clear();
    }
    Ok(Choice {
        rows,
        dropped,
        same,
        unsure,
    })
}

fn word_overlap(word: &Word, turn: &SpeakerTurn) -> f64 {
    (word.end.min(turn.end) - word.start.max(turn.start)).max(0.0)
}

fn speakers_of(turns: &[SpeakerTurn]) -> Vec<u32> {
    let mut speakers: Vec<u32> = turns.iter().map(|turn| turn.speaker).collect();
    speakers.sort_unstable();
    speakers.dedup();
    speakers
}

/// Seconds each row's words overlap each speaker's turns, per row and speaker.
fn row_overlaps(rows: &[Track], speakers: &[u32], turns: &[SpeakerTurn]) -> Vec<Vec<f64>> {
    rows.iter()
        .map(|row| {
            speakers
                .iter()
                .map(|speaker| {
                    turns
                        .iter()
                        .filter(|turn| turn.speaker == *speaker)
                        .flat_map(|turn| row.words.iter().map(move |word| word_overlap(word, turn)))
                        .sum()
                })
                .collect()
        })
        .collect()
}

/// The speaker per row with the largest total overlap, each speaker once while there are enough.
fn best_assignment(overlaps: &[Vec<f64>], speaker_count: usize) -> Vec<usize> {
    fn walk(
        overlaps: &[Vec<f64>],
        row: usize,
        taken: &mut Vec<usize>,
        total: f64,
        best: &mut (f64, Vec<usize>),
    ) {
        if row == overlaps.len() {
            if total > best.0 {
                *best = (total, taken.clone());
            }
            return;
        }
        let speakers = overlaps[row].len();
        let free: Vec<usize> = (0..speakers).filter(|at| !taken.contains(at)).collect();
        // More rows than speakers: a row left over shares the speaker it overlaps most.
        let choices = if free.is_empty() {
            (0..speakers).collect()
        } else {
            free
        };
        for choice in choices {
            taken.push(choice);
            walk(
                overlaps,
                row + 1,
                taken,
                total + overlaps[row][choice],
                best,
            );
            taken.pop();
        }
    }
    let mut best = (-1.0, Vec::new());
    if speaker_count > 0 {
        walk(overlaps, 0, &mut Vec::new(), 0.0, &mut best);
    }
    best.1
}

fn color_rows(rows: &mut [Track], turns: &[SpeakerTurn]) {
    let speakers = speakers_of(turns);
    let overlaps = row_overlaps(rows, &speakers, turns);
    let chosen = best_assignment(&overlaps, speakers.len());
    for (row, speaker) in rows.iter_mut().zip(chosen) {
        row.words
            .iter_mut()
            .for_each(|word| word.speaker = Some(speakers[speaker]));
    }
}

/// The speaker talking longest in a range that stays one row.
fn longest_speaker(range: Span, turns: &[SpeakerTurn]) -> Option<u32> {
    let mut totals: BTreeMap<u32, f64> = BTreeMap::new();
    for turn in turns {
        let seconds = (turn.end.min(range.end) - turn.start.max(range.start)).max(0.0);
        if seconds > 0.0 {
            *totals.entry(turn.speaker).or_default() += seconds;
        }
    }
    totals
        .into_iter()
        .max_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(speaker, _)| speaker)
}

/// Drops words whose whole reading the first pass already said just outside the range.
fn without_repeats(words: Vec<Word>, edge: &[ReadingChar], neighbour: &[char]) -> Vec<Word> {
    let sound: Vec<char> = edge.iter().map(|item| item.reading).collect();
    let matched: Vec<bool> = {
        let mut flags = vec![false; sound.len()];
        matching_runs(&sound, neighbour)
            .iter()
            .filter(|run| run.2 >= SHORTEST_RUN)
            .for_each(|run| flags[run.0..run.0 + run.2].fill(true));
        flags
    };
    let mut offset = 0;
    let mut kept = Vec::with_capacity(words.len());
    for word in words {
        let end = offset + word.text.chars().count();
        let inside = |item: &ReadingChar| (offset..end).contains(&item.offset);
        let mut own = edge
            .iter()
            .zip(&matched)
            .filter(|(item, _)| inside(item))
            .peekable();
        let repeated = own.peek().is_some() && own.all(|(_, flag)| *flag);
        if !repeated {
            kept.push(word);
        }
        offset = end;
    }
    kept
}

/// A row without what it repeats of the first pass just before and after the range; where they
/// disagree the row wins, and what only the first pass has is not added back.
fn stitched(
    words: Vec<Word>,
    before: &[char],
    after: &[char],
    segmenter: &Segmenter,
) -> AppResult<Vec<Word>> {
    let readings = segmenter.readings(&text_of(&words))?;
    let head = &readings[..readings.len().min(EDGE_READINGS)];
    let words = without_repeats(words, head, before);
    let readings = segmenter.readings(&text_of(&words))?;
    let tail = &readings[readings.len().saturating_sub(EDGE_READINGS)..];
    Ok(without_repeats(words, tail, after))
}

fn outside_sound(
    words: &[Word],
    from: f64,
    to: f64,
    segmenter: &Segmenter,
) -> AppResult<Vec<char>> {
    let near: Vec<Word> = words
        .iter()
        .filter(|word| (from..to).contains(&middle(word)))
        .cloned()
        .collect();
    sounds(&text_of(&near), segmenter)
}

fn clip_name(work: &Path, range: usize, track: &str) -> PathBuf {
    work.join(format!("lane-{range}-{track}.wav"))
}

fn tracks_of(work: &Path, range: usize, deep: bool) -> Vec<PathBuf> {
    let names: &[&str] = if deep {
        &["0-0", "0-1", "1-0", "1-1"]
    } else {
        &["0", "1"]
    };
    names
        .iter()
        .map(|name| clip_name(work, range, name))
        .collect()
}

fn second_pass(work: &Path, range: usize) -> [SeparationClip; FIRST_TRACKS] {
    ["0", "1"].map(|track| SeparationClip {
        input: clip_name(work, range, track),
        outputs: [
            clip_name(work, range, &format!("{track}-0")),
            clip_name(work, range, &format!("{track}-1")),
        ],
    })
}

/// Separates and transcribes every range where people talk at once, and returns the words with those
/// ranges' first-pass words replaced by one row per voice. Nothing overlapping means nothing runs.
pub fn with_rows(
    words: Vec<Word>,
    turns: &[SpeakerTurn],
    wav: &Path,
    duration: f64,
    work: &Path,
    segmenter: &Segmenter,
    engine: &mut LaneEngine<'_>,
) -> AppResult<(Vec<Word>, LaneStats)> {
    let together = spans_with(turns, 2);
    let overlaps = joined(together.clone(), JOIN_SECONDS);
    let mut stats = LaneStats::default();
    if overlaps.is_empty() {
        return Ok((words, stats));
    }
    let boundaries = boundary_times(&words, segmenter)?;
    let all_ranges = ranges_around(&overlaps, &boundaries, duration);
    let ranges: Vec<Span> = all_ranges
        .iter()
        .copied()
        .filter(|range| seconds_inside(*range, &together) >= SHORTEST_OVERLAP_SECONDS)
        .collect();
    stats.skipped_ranges = all_ranges.len() - ranges.len();
    if ranges.is_empty() {
        return Ok((words, stats));
    }
    let crowded = spans_with(turns, 3);
    let deep: Vec<bool> = ranges
        .iter()
        .map(|range| engine.deep && seconds_inside(*range, &crowded) > DEEP_SECONDS)
        .collect();
    stats.ranges = ranges.len();
    stats.deep_ranges = deep.iter().filter(|deep| **deep).count();
    stats.range_seconds = ranges.iter().map(|range| range.end - range.start).sum();

    let cuts: Vec<(f64, f64, PathBuf)> = ranges
        .iter()
        .enumerate()
        .map(|(at, range)| (range.start, range.end, clip_name(work, at, "mix")))
        .collect();
    cut_clips(wav, &cuts)?;
    let began = std::time::Instant::now();
    let clips: Vec<SeparationClip> = (0..ranges.len())
        .map(|at| SeparationClip {
            input: clip_name(work, at, "mix"),
            outputs: [clip_name(work, at, "0"), clip_name(work, at, "1")],
        })
        .chain(
            (0..ranges.len())
                .filter(|at| deep[*at])
                .flat_map(|at| second_pass(work, at)),
        )
        .collect();
    (engine.separate)(&clips)?;
    stats.separation_seconds = began.elapsed().as_secs_f64();

    let began = std::time::Instant::now();
    let track_files: Vec<Vec<PathBuf>> = (0..ranges.len())
        .map(|at| tracks_of(work, at, deep[at]))
        .collect();
    let whisper_clips: Vec<WhisperClip> = track_files
        .iter()
        .flatten()
        .map(|file| WhisperClip {
            wav: file.clone(),
            base: file.with_extension(""),
        })
        .collect();
    let mut heard = (engine.transcribe)(&whisper_clips)?.into_iter();
    let mut prepared = Vec::with_capacity(ranges.len());
    for (range, files) in ranges.iter().zip(&track_files) {
        let tracks = heard
            .by_ref()
            .take(files.len())
            .map(|mut said| {
                said.iter_mut().for_each(|word| {
                    word.start += range.start;
                    word.end += range.start;
                });
                Ok(Track {
                    sound: sounds(&text_of(&said), segmenter)?,
                    words: said,
                })
            })
            .collect::<AppResult<Vec<Track>>>()?;
        prepared.push(tracks);
    }
    stats.transcription_seconds = began.elapsed().as_secs_f64();

    let mut result: Vec<Word> = Vec::with_capacity(words.len());
    let mut rest = words.as_slice();
    for (at, (range, tracks)) in ranges.iter().zip(prepared).enumerate() {
        let inside_from = rest.partition_point(|word| middle(word) < range.start);
        result.extend_from_slice(&rest[..inside_from]);
        rest = &rest[inside_from..];
        let inside_to = rest.partition_point(|word| middle(word) < range.end);
        let first = &rest[..inside_to];
        let first_sound = sounds(&text_of(first), segmenter)?;
        let choice = choose_rows(tracks, &first_sound, deep[at], segmenter)?;
        stats.dropped_tracks += choice.dropped;
        stats.same_text += choice.same;
        stats.unsure_ranges += usize::from(choice.unsure);
        let mut rows = choice.rows;
        if rows.is_empty() {
            stats.first_pass_kept += 1;
            *stats.rows.entry(1).or_default() += 1;
            let speaker = longest_speaker(*range, turns);
            result.extend(first.iter().cloned().map(|word| Word {
                speaker: speaker.or(word.speaker),
                ..word
            }));
        } else {
            *stats.rows.entry(rows.len()).or_default() += 1;
            color_rows(&mut rows, turns);
            let before = outside_sound(
                &words,
                range.start - NEIGHBOUR_SECONDS,
                range.start,
                segmenter,
            )?;
            let after = outside_sound(&words, range.end, range.end + NEIGHBOUR_SECONDS, segmenter)?;
            for row in rows {
                result.extend(stitched(row.words, &before, &after, segmenter)?);
            }
        }
        rest = &rest[inside_to..];
    }
    result.extend_from_slice(rest);
    result.sort_by(|left, right| left.start.total_cmp(&right.start));
    Ok((result, stats))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use crate::morphology::test_segmenter;

    fn turn(start: f64, end: f64, speaker: u32) -> SpeakerTurn {
        SpeakerTurn {
            start,
            end,
            speaker,
        }
    }

    fn said(start: f64, end: f64, text: &str, speaker: u32) -> Word {
        Word {
            start,
            end,
            text: text.to_owned(),
            speaker: Some(speaker),
        }
    }

    fn span(start: f64, end: f64) -> Span {
        Span { start, end }
    }

    fn sound(text: &str) -> Vec<char> {
        sounds(text, test_segmenter()).unwrap()
    }

    fn track(text: &str) -> Track {
        Track {
            words: vec![said(0.0, 1.0, text, 1)],
            sound: sound(text),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target-test")
            .join(format!("lanes-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// One second of silence per 16 000 samples, as `extract_audio` writes it.
    fn audio(root: &Path, seconds: usize) -> PathBuf {
        let bytes = seconds * 32_000;
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(&(36 + bytes as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&16_000u32.to_le_bytes());
        wav.extend_from_slice(&32_000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(bytes as u32).to_le_bytes());
        wav.resize(wav.len() + bytes, 0);
        let file = root.join("audio.wav");
        std::fs::write(&file, wav).unwrap();
        file
    }

    /// Runs `with_rows` with the given speech per track file name; separation only records its clips.
    fn run_rows(
        name: &str,
        words: Vec<Word>,
        turns: &[SpeakerTurn],
        heard: &[(&str, &str)],
    ) -> (Vec<Word>, LaneStats, Vec<Vec<SeparationClip>>) {
        let root = scratch(name);
        let wav = audio(&root, 40);
        let mut batches = Vec::new();
        let mut separate = |clips: &[SeparationClip]| {
            batches.push(clips.to_vec());
            Ok(())
        };
        let mut transcribe = |clips: &[WhisperClip]| {
            Ok(clips
                .iter()
                .map(|clip| {
                    let stem = clip.wav.file_stem().unwrap().to_str().unwrap().to_owned();
                    let text = heard
                        .iter()
                        .find(|(name, _)| *name == stem)
                        .map_or("", |pair| pair.1);
                    if text.is_empty() {
                        Vec::new()
                    } else {
                        vec![Word {
                            start: 0.5,
                            end: 1.5,
                            text: text.to_owned(),
                            speaker: None,
                        }]
                    }
                })
                .collect())
        };
        let (out, stats) = with_rows(
            words,
            turns,
            &wav,
            40.0,
            &root,
            test_segmenter(),
            &mut LaneEngine {
                deep: true,
                separate: &mut separate,
                transcribe: &mut transcribe,
            },
        )
        .unwrap();
        (out, stats, batches)
    }

    fn summary(words: &[Word]) -> Vec<(Option<u32>, String)> {
        words
            .iter()
            .map(|word| (word.speaker, word.text.clone()))
            .collect()
    }

    #[test]
    fn nobody_at_once_runs_no_model_and_leaves_the_words() {
        let words = vec![said(0.0, 1.0, "はい", 1), said(2.0, 3.0, "いいえ", 2)];
        let (out, stats, batches) = run_rows(
            "none",
            words.clone(),
            &[turn(0.0, 1.0, 1), turn(2.0, 3.0, 2)],
            &[],
        );
        assert_eq!(out, words);
        assert_eq!(stats, LaneStats::default());
        assert_eq!(batches, Vec::<Vec<SeparationClip>>::new());
    }

    #[test]
    fn spans_are_where_enough_different_people_talk() {
        let turns = [
            turn(0.0, 3.0, 1),
            turn(1.0, 4.0, 2),
            turn(2.0, 2.5, 3),
            turn(3.5, 5.0, 2),
        ];
        assert_eq!(spans_with(&turns, 2), vec![span(1.0, 3.0)]);
        assert_eq!(spans_with(&turns, 3), vec![span(2.0, 2.5)]);
        assert_eq!(
            spans_with(&[turn(0.0, 3.0, 1), turn(1.0, 2.0, 1)], 2),
            vec![]
        );
        assert_eq!(spans_with(&[], 2), vec![]);
    }

    #[test]
    fn overlaps_close_together_join() {
        assert_eq!(
            joined(vec![span(0.0, 1.0), span(3.0, 4.0), span(6.1, 7.0)], 2.0),
            vec![span(0.0, 4.0), span(6.1, 7.0)]
        );
    }

    #[test]
    fn a_range_reaches_the_boundaries_around_its_span_and_touching_ranges_merge() {
        let boundaries = [1.0, 2.0, 5.0, 6.0, 9.0];
        assert_eq!(
            ranges_around(&[span(2.5, 4.5)], &boundaries, 20.0),
            vec![span(2.0, 5.0)]
        );
        assert_eq!(
            ranges_around(&[span(0.5, 0.8), span(9.5, 10.0)], &boundaries, 15.0),
            vec![span(0.0, 1.0), span(9.0, 15.0)]
        );
        assert_eq!(
            ranges_around(&[span(2.5, 4.5), span(4.6, 5.5)], &boundaries, 20.0),
            vec![span(2.0, 6.0)]
        );
    }

    #[test]
    fn a_long_range_is_divided_at_a_boundary_or_at_the_limit_without_one() {
        assert_eq!(
            divided(span(0.0, 20.0), &[3.0, 7.0, 11.0, 15.0]),
            vec![span(0.0, 7.0), span(7.0, 15.0), span(15.0, 20.0)]
        );
        assert_eq!(
            divided(span(0.0, 10.0), &[]),
            vec![span(0.0, 8.0), span(8.0, 10.0)]
        );
        assert_eq!(divided(span(0.0, 5.0), &[]), vec![span(0.0, 5.0)]);
    }

    #[test]
    fn boundaries_are_phrase_ends_in_japanese_and_word_ends_elsewhere() {
        let english = [said(0.0, 1.0, "Hello ", 1), said(1.0, 2.0, "there", 1)];
        assert_eq!(
            boundary_times(&english, test_segmenter()).unwrap(),
            vec![1.0, 2.0]
        );
        let japanese = [
            said(0.0, 1.0, "今日は", 1),
            said(1.0, 2.0, "いい", 1),
            said(2.0, 3.0, "天気です", 1),
        ];
        let times = boundary_times(&japanese, test_segmenter()).unwrap();
        assert!(times.contains(&1.0), "{times:?}");
        assert!(!times.is_empty());
    }

    #[test]
    fn runs_match_like_difflib_and_short_ones_do_not_count() {
        let left: Vec<char> = "abcxdefg".chars().collect();
        let right: Vec<char> = "defgyabc".chars().collect();
        assert_eq!(matching_runs(&left, &right), vec![(4, 0, 4)]);
        assert_eq!(shared_readings(&left, &right), 4);
        let short: Vec<char> = "ab".chars().collect();
        assert_eq!(shared_readings(&short, &short), 0);
        assert_eq!(matching_runs(&short, &[]), vec![]);
    }

    #[test]
    fn a_track_nothing_like_the_first_pass_is_dropped_and_the_one_left_is_not_trusted() {
        let first = sound("今日はいい天気ですね本当に");
        let choice = choose_rows(
            vec![track("今日はいい天気ですね"), track("全然違う言葉ばかり")],
            &first,
            false,
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(
            (
                choice.rows.len(),
                choice.dropped,
                choice.same,
                choice.unsure
            ),
            (0, 1, 0, true)
        );
    }

    #[test]
    fn two_rows_both_heard_in_the_first_pass_stay() {
        let first = sound("今日はいい天気ですね明日は雨が降るそうです");
        let choice = choose_rows(
            vec![
                track("今日はいい天気ですね"),
                track("明日は雨が降るそうです"),
            ],
            &first,
            false,
            test_segmenter(),
        )
        .unwrap();
        assert_eq!((choice.rows.len(), choice.unsure), (2, false));
    }

    #[test]
    fn a_row_little_of_it_heard_in_the_first_pass_leaves_the_range_to_the_first_pass() {
        let first = sound("今日はいい天気ですね明日は雨");
        let choice = choose_rows(
            vec![
                track("今日はいい天気ですね"),
                track("明日は雨が降るそうですが傘は持っていきません"),
            ],
            &first,
            false,
            test_segmenter(),
        )
        .unwrap();
        assert_eq!((choice.rows.len(), choice.unsure), (0, true));
    }

    #[test]
    fn a_first_pass_stuck_in_a_loop_gives_way_to_the_tracks() {
        let first = sound("説得力無い説得力無い説得力無い説得力無い");
        let choice = choose_rows(
            vec![
                track("今日はいい天気ですね"),
                track("ご視聴ありがとうございました"),
            ],
            &first,
            false,
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(
            (choice.rows.len(), choice.dropped, choice.unsure),
            (1, 1, false)
        );
        let same = choose_rows(
            vec![track("今日はいい天気ですね"), track("今日はいい天気です")],
            &first,
            false,
            test_segmenter(),
        )
        .unwrap();
        assert_eq!((same.rows.len(), same.same), (1, 1));
    }

    #[test]
    fn a_track_stuck_in_a_loop_is_no_row() {
        let first = sound("今日はいい天気ですね明日は雨が降るそうです");
        let choice = choose_rows(
            vec![
                track("今日はいい天気ですね"),
                track("うんうんうんうんうんうんうんうんうんうん"),
            ],
            &first,
            false,
            test_segmenter(),
        )
        .unwrap();
        assert_eq!(choice.dropped, 1);
    }

    #[test]
    fn a_loop_is_a_unit_said_again_and_again() {
        let share = |text: &str| -> f64 { loop_share(&text.chars().collect::<Vec<char>>()) };
        assert_eq!(share("ああああ"), 1.0);
        assert_eq!(share("abcabcabc"), 1.0);
        assert_eq!(share("abab"), 0.0);
        assert_eq!(share("abcdabcd"), 1.0);
        assert_eq!(share("xyzabcdefg"), 0.0);
        assert_eq!(share("xxxabcdef"), 3.0 / 9.0);
        assert_eq!(share(""), 0.0);
    }

    #[test]
    fn too_little_overlap_in_a_range_runs_no_model_and_leaves_the_words() {
        let words = vec![
            said(0.0, 1.0, "最初の言葉です", 1),
            said(2.0, 3.0, "最後の言葉です", 2),
        ];
        let (out, stats, batches) = run_rows(
            "short",
            words.clone(),
            &[turn(0.0, 2.2, 1), turn(2.0, 3.0, 2)],
            &[],
        );
        assert_eq!(out, words);
        assert_eq!((stats.ranges, stats.skipped_ranges), (0, 1));
        assert_eq!(batches, Vec::<Vec<SeparationClip>>::new());
    }

    #[test]
    fn the_same_words_on_two_tracks_fall_back_to_the_first_pass() {
        let first = sound("今日はいい天気ですね");
        let choice = choose_rows(
            vec![track("今日はいい天気ですね"), track("今日はいい天気です")],
            &first,
            false,
            test_segmenter(),
        )
        .unwrap();
        assert_eq!((choice.rows.len(), choice.dropped, choice.same), (0, 0, 1));
    }

    #[test]
    fn deep_tracks_lose_only_stock_phrases_short_ones_and_repeats() {
        let first = sound("全然関係ない");
        let choice = choose_rows(
            vec![
                track("今日はいい天気ですね"),
                track("ご視聴ありがとうございました"),
                track("あ"),
                track("明日は雨が降るそうです"),
                track("お疲れ様でした"),
            ],
            &first,
            true,
            test_segmenter(),
        )
        .unwrap();
        assert_eq!((choice.rows.len(), choice.dropped, choice.same), (2, 3, 0));
    }

    #[test]
    fn deep_tracks_repeating_each_other_keep_one_of_each_voice() {
        let choice = choose_rows(
            vec![
                track("今日はいい天気ですね"),
                track("今日はいい天気ですね"),
                track("明日は雨が降るそうです"),
            ],
            &sound("x"),
            true,
            test_segmenter(),
        )
        .unwrap();
        assert_eq!((choice.rows.len(), choice.same), (2, 1));
    }

    #[test]
    fn a_stock_phrase_with_more_said_around_it_stays() {
        let long = sound("今日はここまでご視聴ありがとうございました");
        assert!(!is_only_stock_phrase(&long, test_segmenter()).unwrap());
        let stock = sound("ご視聴ありがとうございました");
        assert!(is_only_stock_phrase(&stock, test_segmenter()).unwrap());
    }

    #[test]
    fn speakers_go_to_the_rows_for_the_largest_total_overlap() {
        // Row 0 alone would take speaker 1, but the total is larger with it on speaker 2.
        assert_eq!(
            best_assignment(&[vec![3.0, 2.0], vec![3.0, 0.0]], 2),
            vec![1, 0]
        );
        assert_eq!(best_assignment(&[vec![1.0], vec![2.0]], 1), vec![0, 0]);
        assert_eq!(best_assignment(&[vec![], vec![]], 0), Vec::<usize>::new());
        assert_eq!(
            best_assignment(
                &[
                    vec![4.0, 0.0, 0.0, 0.0],
                    vec![0.0, 3.0, 0.0, 0.0],
                    vec![0.0, 0.0, 2.0, 0.0],
                    vec![0.0, 0.0, 0.0, 1.0]
                ],
                4
            ),
            vec![0, 1, 2, 3]
        );
    }

    #[test]
    fn the_longest_speaker_of_a_range_counts_only_inside_it() {
        let turns = [turn(0.0, 100.0, 1), turn(4.0, 6.0, 2), turn(6.0, 8.0, 2)];
        assert_eq!(longest_speaker(span(5.0, 9.0), &turns), Some(1));
        assert_eq!(longest_speaker(span(5.0, 9.0), &turns[1..]), Some(2));
        assert_eq!(longest_speaker(span(20.0, 30.0), &turns[1..]), None);
    }

    #[test]
    fn a_rows_edge_repeating_the_first_pass_next_to_the_range_is_cut() {
        let words = vec![
            said(0.0, 1.0, "今日は", 1),
            said(1.0, 2.0, "いい天気", 1),
            said(2.0, 3.0, "明日は", 1),
            said(3.0, 4.0, "雨です", 1),
        ];
        let before = sound("そうです今日は");
        let after = sound("雨ですね");
        let kept = stitched(words, &before, &after, test_segmenter()).unwrap();
        assert_eq!(
            kept.iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>(),
            vec!["いい天気", "明日は"]
        );
    }

    #[test]
    fn a_row_sharing_nothing_with_its_neighbours_stays_whole() {
        let words = vec![said(0.0, 1.0, "今日は", 1)];
        let kept = stitched(words.clone(), &sound("全然"), &[], test_segmenter()).unwrap();
        assert_eq!(kept, words);
        assert_eq!(
            stitched(Vec::new(), &[], &[], test_segmenter()).unwrap(),
            vec![]
        );
    }

    #[test]
    fn two_voices_replace_the_first_pass_in_their_range_on_rows_of_their_own() {
        let words = vec![
            said(0.0, 1.0, "最初の言葉です", 1),
            said(10.0, 11.0, "今日はいい天気ですね明日は雨が降るそうです", 1),
            said(30.0, 31.0, "最後の言葉です", 2),
        ];
        let turns = [turn(0.0, 11.0, 1), turn(10.2, 11.0, 2), turn(30.0, 31.0, 2)];
        let (out, stats, batches) = run_rows(
            "two",
            words,
            &turns,
            &[
                ("lane-0-0", "今日はいい天気ですね"),
                ("lane-0-1", "明日は雨が降るそうです"),
            ],
        );
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 1);
        assert_eq!(stats.ranges, 1);
        assert_eq!(stats.deep_ranges, 0);
        assert_eq!(stats.rows, BTreeMap::from([(2, 1)]));
        let texts: Vec<String> = out.iter().map(|word| word.text.clone()).collect();
        assert!(
            texts.contains(&"今日はいい天気ですね".to_owned()),
            "{texts:?}"
        );
        assert!(
            texts.contains(&"明日は雨が降るそうです".to_owned()),
            "{texts:?}"
        );
        assert!(!texts
            .iter()
            .any(|text| text.contains("今日はいい天気ですね明日")));
        let mut speakers: Vec<Option<u32>> = out
            .iter()
            .filter(|word| word.text.contains("今日は") || word.text.contains("明日は"))
            .map(|word| word.speaker)
            .collect();
        speakers.sort_unstable();
        speakers.dedup();
        assert_eq!(speakers.len(), 2);
    }

    #[test]
    fn three_people_at_once_go_through_the_model_twice_for_four_tracks() {
        let words = vec![
            said(5.0, 8.5, "最初の言葉です", 1),
            said(9.5, 11.5, "全部が混ざった言葉です", 1),
            said(12.5, 14.0, "最後の言葉です", 1),
        ];
        let turns = [turn(9.0, 12.0, 1), turn(9.0, 12.0, 2), turn(9.0, 12.0, 3)];
        let (out, stats, batches) = run_rows(
            "deep",
            words,
            &turns,
            &[
                ("lane-0-0-0", "今日はいい天気ですね"),
                ("lane-0-0-1", "明日は雨が降るそうです"),
                ("lane-0-1-0", "昨日は風が強かったです"),
                ("lane-0-1-1", "ご視聴ありがとうございました"),
            ],
        );
        // One run of the model: the mix, then each of its tracks, in that order.
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 3);
        assert_eq!(batches[0][1].input, batches[0][0].outputs[0]);
        assert_eq!(stats.deep_ranges, 1);
        assert_eq!(stats.dropped_tracks, 1);
        assert_eq!(stats.rows, BTreeMap::from([(3, 1)]));
        let mut speakers: Vec<Option<u32>> = out
            .iter()
            .filter(|word| !word.text.contains("の言葉"))
            .map(|word| word.speaker)
            .collect();
        speakers.sort_unstable();
        assert_eq!(speakers, vec![Some(1), Some(2), Some(3)]);
    }

    #[test]
    fn all_deep_tracks_gone_keeps_the_first_pass_for_the_longest_speaker() {
        let words = vec![
            said(5.0, 7.5, "最初の言葉です", 1),
            said(9.5, 11.5, "全部が混ざった言葉です", 1),
            said(13.0, 14.0, "最後の言葉です", 1),
        ];
        let turns = [turn(9.0, 12.0, 1), turn(8.0, 12.5, 2), turn(9.0, 12.0, 3)];
        let (out, stats, _) = run_rows("deep-none", words, &turns, &[]);
        assert_eq!(stats.first_pass_kept, 1);
        assert_eq!(stats.rows, BTreeMap::from([(1, 1)]));
        assert!(
            summary(&out).contains(&(Some(2), "全部が混ざった言葉です".to_owned())),
            "{out:?}"
        );
    }

    #[test]
    fn without_the_memory_for_a_second_pass_three_people_stay_at_two_tracks() {
        let root = scratch("not-deep");
        let wav = audio(&root, 20);
        let turns = [turn(9.0, 12.0, 1), turn(9.0, 12.0, 2), turn(9.0, 12.0, 3)];
        let mut clips_seen: Vec<String> = Vec::new();
        let mut separate = |clips: &[SeparationClip]| {
            clips_seen = clips
                .iter()
                .map(|clip| clip.input.to_string_lossy().into_owned())
                .collect();
            Ok(())
        };
        let mut transcribe = |clips: &[WhisperClip]| Ok(vec![Vec::new(); clips.len()]);
        let (_, stats) = with_rows(
            vec![said(9.5, 11.5, "全部が混ざった言葉です", 1)],
            &turns,
            &wav,
            20.0,
            &root,
            test_segmenter(),
            &mut LaneEngine {
                deep: false,
                separate: &mut separate,
                transcribe: &mut transcribe,
            },
        )
        .unwrap();
        assert!(
            clips_seen.iter().all(|name| name.ends_with("-mix.wav")),
            "{clips_seen:?}"
        );
        assert_eq!(stats.deep_ranges, 0);
    }

    #[test]
    fn a_failure_of_either_step_stops_the_job() {
        let root = scratch("fail");
        let wav = audio(&root, 5);
        let turns = [turn(1.0, 3.0, 1), turn(1.0, 3.0, 2)];
        let words = vec![said(1.0, 2.0, "はい", 1)];
        let mut fail_separate = |_: &[SeparationClip]| Err(AppError::Cancelled);
        let mut unused = |_: &[WhisperClip]| Ok(Vec::new());
        assert!(matches!(
            with_rows(
                words.clone(),
                &turns,
                &wav,
                5.0,
                &root,
                test_segmenter(),
                &mut LaneEngine {
                    deep: true,
                    separate: &mut fail_separate,
                    transcribe: &mut unused,
                },
            ),
            Err(AppError::Cancelled)
        ));
        let mut fine = |_: &[SeparationClip]| Ok(());
        let mut fail_transcribe = |_: &[WhisperClip]| Err(AppError::Cancelled);
        assert!(matches!(
            with_rows(
                words,
                &turns,
                &wav,
                5.0,
                &root,
                test_segmenter(),
                &mut LaneEngine {
                    deep: true,
                    separate: &mut fine,
                    transcribe: &mut fail_transcribe,
                },
            ),
            Err(AppError::Cancelled)
        ));
    }

    #[test]
    fn audio_that_cannot_be_cut_is_an_error() {
        let root = scratch("no-audio");
        let turns = [turn(1.0, 3.0, 1), turn(1.0, 3.0, 2)];
        let mut unused_separate = |_: &[SeparationClip]| Ok(());
        let mut unused = |_: &[WhisperClip]| Ok(Vec::new());
        assert!(with_rows(
            vec![said(1.0, 2.0, "はい", 1)],
            &turns,
            &root.join("none.wav"),
            5.0,
            &root,
            test_segmenter(),
            &mut LaneEngine {
                deep: true,
                separate: &mut unused_separate,
                transcribe: &mut unused,
            },
        )
        .is_err());
    }
}
