//! Moves recognized word clocks onto the voice measured in the audio.
//!
//! Whisper's word times stick to the previous word's end and span silences, so a caption timed by
//! them alone appears before anyone speaks. The energy rules are the repository's: a window is
//! voiced above a local threshold (`caption_energy_audit`) and a speech floor derived from the
//! whole file (`asr_hallucination`). A word with no voice near it cannot have been spoken.

use crate::captions::Word;
use std::path::Path;

const WINDOW_SECONDS: f64 = 0.01;
/// `caption_energy_audit`: the local range around a window, and how far above its floor voice is.
const LOCAL_SECONDS: f64 = 0.75;
const LOCAL_FLOOR_QUANTILE: f64 = 0.1;
const LOCAL_PEAK_QUANTILE: f64 = 0.95;
const RELATIVE_THRESHOLD: f64 = 0.2;
const SILENT_RANGE: f64 = 1e-4;
const MINIMUM_ACTIVE_WINDOWS: usize = 3;
/// The local threshold is recomputed this often; it changes slowly and a 90-minute file is long.
const THRESHOLD_STEP_WINDOWS: usize = 25;
/// `asr_hallucination`: the speech floor below the loud part of the file.
const FLOOR_QUANTILE: f64 = 0.9;
const SPEECH_FLOOR_BELOW_P90_DB: f64 = 25.0;
const ABSOLUTE_FLOOR_DB: f64 = -70.0;
/// `asr_hallucination.ALIGNMENT_TOLERANCE_SECONDS`: voice further than this is not this word's.
const ALIGNMENT_TOLERANCE_SECONDS: f64 = 1.0;
/// A stop consonant silences the voice this long inside one sound.
const BRIDGED_WINDOWS: usize = 12;
/// Voice this short is a click or the end of the word before, not where a word starts.
const REMNANT_SECONDS: f64 = 0.15;
/// One window, which is all the voice search can give a word with no span.
const LENGTHLESS_SECONDS: f64 = 0.015;
/// How far ahead of the voice Whisper's clock has been measured to run, within one recording.
const CLOCK_LEAD_REACH_SECONDS: f64 = 2.5;
/// A time computed from a window index must land back on that window, not just under it.
const WINDOW_ROUNDING: f64 = 1e-6;
/// Speech stays above about five syllables a second; piled words are not stretched past that.
const LONGEST_CHARACTER_SECONDS: f64 = 0.2;
const PCM_FORMAT: u16 = 1;
const PCM_CHANNELS: u16 = 1;
const PCM_BITS: u16 = 16;
const FULL_SCALE: f64 = 32768.0;
const RIFF_HEADER_BYTES: usize = 12;
const CHUNK_HEADER_BYTES: usize = 8;
const FORMAT_CHUNK_BYTES: usize = 16;

/// Which 10 ms windows of a recording hold voice.
#[derive(Clone, Debug, PartialEq)]
pub struct SpeechMap {
    voiced: Vec<bool>,
}

fn quantile(sorted: &[f64], fraction: f64) -> f64 {
    let index = ((sorted.len() - 1) as f64 * fraction).round() as usize;
    sorted[index]
}

fn sorted(values: &[f64]) -> Vec<f64> {
    let mut copy = values.to_vec();
    copy.sort_by(f64::total_cmp);
    copy
}

fn rms_windows(samples: &[i16], rate: u32) -> Vec<f64> {
    let step = ((f64::from(rate) * WINDOW_SECONDS).round() as usize).max(1);
    samples
        .chunks(step)
        .map(|chunk| {
            let total: f64 = chunk
                .iter()
                .map(|sample| (f64::from(*sample) / FULL_SCALE).powi(2))
                .sum();
            (total / chunk.len() as f64).sqrt()
        })
        .collect()
}

/// The level above which a window near `center` is voice, or `None` over flat sound, where only the
/// speech floor can tell (a held note has no local rise to measure).
fn local_threshold(energy: &[f64], center: usize) -> Option<f64> {
    let reach = (LOCAL_SECONDS / WINDOW_SECONDS) as usize;
    let last = (center + reach).min(energy.len());
    let local = sorted(&energy[center.saturating_sub(reach)..last]);
    let (floor, peak) = (
        quantile(&local, LOCAL_FLOOR_QUANTILE),
        quantile(&local, LOCAL_PEAK_QUANTILE),
    );
    (peak - floor >= SILENT_RANGE).then_some(floor + (peak - floor) * RELATIVE_THRESHOLD)
}

/// Only sustained runs count, and a short closure inside a sound does not end it.
fn sustained(active: &[bool]) -> Vec<bool> {
    let mut voiced = vec![false; active.len()];
    let mut start = 0;
    while start < active.len() {
        let length = active[start..]
            .iter()
            .take_while(|on| **on == active[start])
            .count();
        if active[start] && length >= MINIMUM_ACTIVE_WINDOWS {
            voiced[start..start + length].fill(true);
        }
        start += length;
    }
    let mut last_voice: Option<usize> = None;
    for index in 0..voiced.len() {
        if !voiced[index] {
            continue;
        }
        if let Some(previous) = last_voice.filter(|previous| index - previous <= BRIDGED_WINDOWS) {
            voiced[previous..index].fill(true);
        }
        last_voice = Some(index);
    }
    voiced
}

fn pcm_samples(bytes: &[u8]) -> Option<(Vec<i16>, u32)> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..RIFF_HEADER_BYTES)? != b"WAVE" {
        return None;
    }
    let mut rate = None;
    let mut position = RIFF_HEADER_BYTES;
    loop {
        let id = bytes.get(position..position + 4)?;
        let size = u32::from_le_bytes(bytes.get(position + 4..position + 8)?.try_into().ok()?);
        let body = position + CHUNK_HEADER_BYTES;
        let end = body.saturating_add(size as usize).min(bytes.len());
        if id == b"fmt " {
            let format = bytes.get(body..body + FORMAT_CHUNK_BYTES)?;
            let shape = [&format[0..2], &format[2..4], &format[14..16]]
                .map(|field| u16::from_le_bytes([field[0], field[1]]));
            if shape != [PCM_FORMAT, PCM_CHANNELS, PCM_BITS] {
                return None;
            }
            rate = Some(u32::from_le_bytes(format[4..8].try_into().ok()?));
        }
        if id == b"data" {
            let samples = bytes[body..end]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| i16::from_le_bytes(*pair))
                .collect();
            return Some((samples, rate?));
        }
        position = end + (size as usize & 1);
    }
}

/// A word with no length carries no timing of its own: it is said with the word after it, even
/// when several in a row wait for that one.
fn pull_lengthless_words(words: &mut [Word]) {
    for index in (0..words.len().saturating_sub(1)).rev() {
        let next_start = words[index + 1].start;
        let word = &mut words[index];
        let waits = next_start - word.start <= ALIGNMENT_TOLERANCE_SECONDS;
        if word.end - word.start <= LENGTHLESS_SECONDS && waits {
            word.start = next_start;
            word.end = next_start;
        }
    }
}

impl SpeechMap {
    #[must_use]
    pub fn from_samples(samples: &[i16], rate: u32) -> Self {
        let energy = rms_windows(samples, rate);
        if energy.is_empty() {
            return Self { voiced: Vec::new() };
        }
        let all = sorted(&energy);
        let floor_db = (20.0 * (quantile(&all, FLOOR_QUANTILE) + 1e-12).log10()
            - SPEECH_FLOOR_BELOW_P90_DB)
            .max(ABSOLUTE_FLOOR_DB);
        let floor = 10f64.powf(floor_db / 20.0);
        let thresholds: Vec<Option<f64>> = (0..energy.len())
            .step_by(THRESHOLD_STEP_WINDOWS)
            .map(|center| local_threshold(&energy, center))
            .collect();
        let active: Vec<bool> = energy
            .iter()
            .enumerate()
            .map(|(index, level)| {
                *level >= floor
                    && thresholds[index / THRESHOLD_STEP_WINDOWS].is_none_or(|gate| *level >= gate)
            })
            .collect();
        Self {
            voiced: sustained(&active),
        }
    }

    /// The map of a mono 16-bit WAV. The job's own extraction writes that; anything else leaves the
    /// words on Whisper's clock rather than failing a transcript that is otherwise fine.
    #[must_use]
    pub fn from_wav(path: &Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        let (samples, rate) = pcm_samples(&bytes)?;
        Some(Self::from_samples(&samples, rate))
    }

    fn window(seconds: f64) -> usize {
        (seconds.max(0.0) / WINDOW_SECONDS + WINDOW_ROUNDING) as usize
    }

    /// The start of the first voiced window in `start..end`.
    #[must_use]
    pub fn first_voiced(&self, start: f64, end: f64) -> Option<f64> {
        let (first, last) = (
            Self::window(start),
            Self::window(end).min(self.voiced.len()),
        );
        (first..last)
            .find(|index| self.voiced[*index])
            .map(|index| index as f64 * WINDOW_SECONDS)
    }

    /// The end of the last voiced window in `start..end`.
    #[must_use]
    pub fn last_voiced(&self, start: f64, end: f64) -> Option<f64> {
        let (first, last) = (
            Self::window(start),
            Self::window(end).min(self.voiced.len()),
        );
        (first..last)
            .rev()
            .find(|index| self.voiced[*index])
            .map(|index| (index + 1) as f64 * WINDOW_SECONDS)
    }

    fn is_voiced_at(&self, seconds: f64) -> bool {
        self.voiced
            .get(Self::window(seconds))
            .copied()
            .unwrap_or(false)
    }

    /// The bounds of the voiced run holding `seconds`, which must be voiced.
    fn run_at(&self, seconds: f64) -> (f64, f64) {
        let index = Self::window(seconds);
        let before = self.voiced[..index]
            .iter()
            .rev()
            .take_while(|on| **on)
            .count();
        let after = self.voiced[index..].iter().take_while(|on| **on).count();
        (
            (index - before) as f64 * WINDOW_SECONDS,
            (index + after) as f64 * WINDOW_SECONDS,
        )
    }

    /// The start of the next run long enough to be speech: a shorter one is a click or the tail of
    /// the word before, unless nothing longer follows, when it is all the word has.
    fn next_speech(&self, from: f64, limit: f64) -> Option<f64> {
        let first = self.first_voiced(from, limit)?;
        let mut cursor = first;
        while let Some(onset) = self.first_voiced(cursor, limit) {
            let (_, end) = self.run_at(onset);
            if end - onset >= REMNANT_SECONDS {
                return Some(onset);
            }
            cursor = end;
        }
        Some(first)
    }

    /// Where a word starts, searched from its own recognized time so that no correction pushes the
    /// next: inside long-enough voice it stays (at the voice's start when it opens it), otherwise it
    /// moves to the next speech within the distance Whisper's clock has been seen to run ahead.
    fn heard_start(&self, raw: f64, previous_end: f64) -> Option<f64> {
        let anchor = raw.max(previous_end);
        if self.is_voiced_at(anchor) {
            let (onset, end) = self.run_at(anchor);
            if end - onset >= REMNANT_SECONDS {
                let opens_run = previous_end <= onset && raw < onset;
                return Some(if opens_run { onset } else { anchor });
            }
        }
        self.next_speech(anchor, raw + CLOCK_LEAD_REACH_SECONDS)
    }

    /// Words piled on one instant were said in the voice that follows: they share it by length, up
    /// to the next word, so a caption made of them lasts and never starts in silence.
    fn spread_piled_words(&self, words: &mut [Word]) {
        let mut first = 0;
        while first < words.len() {
            let start = words[first].start;
            let piled = words[first..]
                .iter()
                .take_while(|word| {
                    word.start == start && word.end - word.start <= LENGTHLESS_SECONDS
                })
                .count();
            let Some(next) = words.get(first + piled).map(|word| word.start) else {
                return;
            };
            let voice_end = if self.is_voiced_at(start) {
                self.run_at(start).1
            } else {
                start
            };
            let characters: usize = words[first..first + piled]
                .iter()
                .map(|word| word.text.chars().count())
                .sum();
            let room = (next.min(voice_end) - start)
                .min(characters as f64 * LONGEST_CHARACTER_SECONDS)
                .max(0.0);
            let share = room / characters.max(1) as f64;
            let mut said = 0;
            for word in &mut words[first..first + piled] {
                word.start = start + share * said as f64;
                said += word.text.chars().count();
                word.end = start + share * said as f64;
            }
            first += piled.max(1);
        }
    }

    /// The words on the voice's clock, in order, without the ones nobody can have spoken. Words
    /// recognized before the voice they belong to wait at its start until their own time comes.
    /// Marks make no sound of their own and only stay in order.
    #[must_use]
    pub fn align(&self, words: Vec<Word>) -> Vec<Word> {
        let mut aligned: Vec<Word> = Vec::with_capacity(words.len());
        for word in words {
            let previous_end = aligned.last().map_or(0.0, |earlier| earlier.end);
            let spoken = word.text.chars().any(char::is_alphanumeric);
            let heard_near = self
                .first_voiced(
                    (word.start - ALIGNMENT_TOLERANCE_SECONDS).max(0.0),
                    word.end + CLOCK_LEAD_REACH_SECONDS,
                )
                .is_some();
            if spoken && !heard_near {
                continue;
            }
            let found = spoken.then(|| self.heard_start(word.start, previous_end));
            let start = found.flatten().unwrap_or(word.start.max(previous_end));
            let end = if spoken {
                self.last_voiced(start, word.end).unwrap_or(start)
            } else {
                word.end
            };
            aligned.push(Word {
                start,
                end: end.max(start),
                ..word
            });
        }
        pull_lengthless_words(&mut aligned);
        self.spread_piled_words(&mut aligned);
        aligned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(start: f64, end: f64, text: &str) -> Word {
        Word {
            start,
            end,
            text: text.to_owned(),
            speaker: None,
        }
    }

    /// A 100 Hz square tone during each span of an otherwise near-silent 1 kHz recording.
    fn tone(seconds: f64, spans: &[(f64, f64)]) -> Vec<i16> {
        (0..(seconds * 1000.0) as usize)
            .map(|index| {
                let time = index as f64 / 1000.0;
                let on = spans
                    .iter()
                    .any(|(start, end)| (*start..*end).contains(&time));
                match (on, index % 10 < 5) {
                    (true, true) => 8000,
                    (true, false) => -8000,
                    _ => 1,
                }
            })
            .collect()
    }

    #[test]
    fn voice_is_found_where_the_tone_is() {
        let map = SpeechMap::from_samples(&tone(4.0, &[(1.0, 2.0)]), 1000);
        assert_eq!(map.first_voiced(0.0, 4.0), Some(1.0));
        assert_eq!(map.last_voiced(0.0, 4.0), Some(2.0));
        assert_eq!(map.first_voiced(2.5, 4.0), None);
    }

    #[test]
    fn a_word_starting_in_silence_moves_to_its_voice() {
        let map = SpeechMap::from_samples(&tone(4.0, &[(1.0, 2.0)]), 1000);
        let aligned = map.align(vec![word(0.2, 1.5, "はい")]);
        assert_eq!((aligned[0].start, aligned[0].end), (1.0, 1.5));
    }

    #[test]
    fn a_word_is_trimmed_back_from_the_silence_after_it() {
        let map = SpeechMap::from_samples(&tone(4.0, &[(1.0, 2.0)]), 1000);
        let aligned = map.align(vec![word(1.2, 3.5, "はい")]);
        assert_eq!((aligned[0].start, aligned[0].end), (1.2, 2.0));
    }

    #[test]
    fn a_word_never_starts_before_the_voice_it_was_recognized_in() {
        let map = SpeechMap::from_samples(&tone(4.0, &[(1.0, 2.0)]), 1000);
        let aligned = map.align(vec![word(1.5, 1.8, "はい")]);
        assert_eq!((aligned[0].start, aligned[0].end), (1.5, 1.8));
    }

    #[test]
    fn a_zero_length_word_at_the_previous_words_end_waits_for_its_own_voice() {
        let map = SpeechMap::from_samples(&tone(6.0, &[(1.0, 2.0), (2.7, 4.0)]), 1000);
        let aligned = map.align(vec![
            word(1.0, 1.95, "この"),
            word(1.95, 1.95, "農"),
            word(2.7, 3.5, "園"),
        ]);
        assert_eq!((aligned[1].start, aligned[1].end), (2.7, 2.7));
        assert_eq!((aligned[2].start, aligned[2].end), (2.7, 3.5));
    }

    #[test]
    fn a_chain_of_lengthless_words_waits_for_the_first_word_with_voice() {
        let map = SpeechMap::from_samples(&tone(6.0, &[(1.0, 2.2), (2.7, 4.0)]), 1000);
        let aligned = map.align(vec![
            word(1.0, 1.9, "よ"),
            word(1.9, 1.9, "お"),
            word(1.9, 1.9, "茶"),
            word(2.7, 3.5, "の"),
        ]);
        assert_eq!(
            aligned
                .iter()
                .map(|word| ((word.start * 1000.0).round(), (word.end * 1000.0).round()))
                .collect::<Vec<_>>(),
            vec![
                (1000.0, 1900.0),
                (2700.0, 2700.0),
                (2700.0, 2700.0),
                (2700.0, 3500.0)
            ]
        );
    }

    #[test]
    fn a_lengthless_word_far_from_the_next_keeps_its_place() {
        let mut words = vec![word(1.0, 1.0, "は"), word(5.0, 5.5, "い")];
        pull_lengthless_words(&mut words);
        assert_eq!((words[0].start, words[0].end), (1.0, 1.0));
    }

    #[test]
    fn words_recognized_before_their_voice_start_with_it() {
        let map = SpeechMap::from_samples(&tone(8.0, &[(2.0, 4.0)]), 1000);
        let aligned = map.align(vec![
            word(1.0, 1.4, "出"),
            word(1.4, 1.8, "勤"),
            word(1.8, 2.2, "中"),
            word(2.2, 2.6, "の"),
        ]);
        assert_eq!(
            aligned
                .iter()
                .map(|word| ((word.start * 100.0).round(), (word.end * 100.0).round()))
                .collect::<Vec<_>>(),
            vec![
                (200.0, 200.0),
                (200.0, 200.0),
                (200.0, 220.0),
                (220.0, 260.0)
            ]
        );
    }

    #[test]
    fn piled_words_share_the_time_up_to_the_next_word_by_length() {
        let map = SpeechMap::from_samples(&tone(4.0, &[(1.0, 3.0)]), 1000);
        let mut words = vec![
            word(1.0, 1.0, "あい"),
            word(1.0, 1.0, "う"),
            word(1.3, 2.0, "え"),
        ];
        map.spread_piled_words(&mut words);
        assert_eq!(
            words
                .iter()
                .map(|word| ((word.start * 100.0).round(), (word.end * 100.0).round()))
                .collect::<Vec<_>>(),
            vec![(100.0, 120.0), (120.0, 130.0), (130.0, 200.0)]
        );
    }

    #[test]
    fn piled_words_stay_inside_the_voice_they_were_said_in() {
        let map = SpeechMap::from_samples(&tone(10.0, &[(1.0, 1.1), (9.0, 9.5)]), 1000);
        let mut words = vec![word(1.0, 1.0, "あい"), word(9.0, 9.5, "う")];
        map.spread_piled_words(&mut words);
        assert_eq!(
            (
                (words[0].start * 100.0).round(),
                (words[0].end * 100.0).round()
            ),
            (100.0, 110.0)
        );
    }

    #[test]
    fn piled_words_in_silence_stay_where_they_are() {
        let map = SpeechMap::from_samples(&tone(10.0, &[(5.0, 6.0)]), 1000);
        let mut words = vec![word(1.0, 1.0, "あ"), word(5.0, 5.5, "い")];
        map.spread_piled_words(&mut words);
        assert_eq!((words[0].start, words[0].end), (1.0, 1.0));
    }

    #[test]
    fn a_pile_with_nothing_after_it_stays_where_it_is() {
        let map = SpeechMap::from_samples(&tone(4.0, &[(1.0, 3.0)]), 1000);
        let mut words = vec![word(1.0, 1.0, "あ")];
        map.spread_piled_words(&mut words);
        assert_eq!((words[0].start, words[0].end), (1.0, 1.0));
    }

    #[test]
    fn a_new_run_of_voice_measures_the_lead_again() {
        let map = SpeechMap::from_samples(&tone(10.0, &[(2.0, 3.0), (5.0, 6.0)]), 1000);
        let aligned = map.align(vec![word(1.0, 1.5, "は"), word(4.8, 5.3, "い")]);
        assert_eq!((aligned[0].start, aligned[1].start), (2.0, 5.0));
    }

    #[test]
    fn corrections_never_accumulate_over_a_recording() {
        // Every stretch of voice starts 0.5 s after its word; the delay must stay 0.5 s.
        let spans: Vec<(f64, f64)> = (0..20)
            .map(|index| (f64::from(index) * 3.0 + 0.5, f64::from(index) * 3.0 + 1.5))
            .collect();
        let map = SpeechMap::from_samples(&tone(62.0, &spans), 1000);
        let words: Vec<Word> = (0..20)
            .map(|index| word(f64::from(index) * 3.0, f64::from(index) * 3.0 + 1.0, "は"))
            .collect();
        let aligned = map.align(words);
        let delays: Vec<i64> = aligned
            .iter()
            .enumerate()
            .map(|(index, word)| ((word.start - index as f64 * 3.0) * 100.0).round() as i64)
            .collect();
        assert_eq!(delays, vec![50; 20]);
    }

    #[test]
    fn a_word_outside_the_clock_lead_reach_is_not_moved_to_far_voice() {
        let map = SpeechMap::from_samples(&tone(10.0, &[(1.0, 1.5), (7.0, 8.0)]), 1000);
        let aligned = map.align(vec![word(2.0, 2.2, "は")]);
        assert_eq!(aligned[0].start, 2.0);
    }

    #[test]
    fn a_click_is_not_where_a_word_starts_when_speech_follows() {
        let map = SpeechMap::from_samples(&tone(6.0, &[(1.0, 1.05), (2.0, 3.0)]), 1000);
        let aligned = map.align(vec![word(0.8, 2.5, "今")]);
        assert_eq!(aligned[0].start, 2.0);
    }

    #[test]
    fn a_short_sound_is_used_when_nothing_longer_follows() {
        let map = SpeechMap::from_samples(&tone(6.0, &[(1.0, 1.05)]), 1000);
        let aligned = map.align(vec![word(0.8, 1.0, "え")]);
        assert_eq!(aligned[0].start, 1.0);
    }

    #[test]
    fn a_word_keeps_a_long_voice_already_running_when_it_starts() {
        let map = SpeechMap::from_samples(&tone(6.0, &[(1.0, 3.0), (4.0, 5.0)]), 1000);
        let aligned = map.align(vec![word(1.5, 4.5, "はい")]);
        assert_eq!(aligned[0].start, 1.5);
    }

    #[test]
    fn a_word_whose_only_voice_is_the_last_ones_tail_keeps_that_voice() {
        let map = SpeechMap::from_samples(&tone(6.0, &[(1.0, 2.0)]), 1000);
        let aligned = map.align(vec![word(1.95, 2.5, "は")]);
        assert_eq!(aligned[0].start, 1.95);
    }

    #[test]
    fn a_silent_word_moves_to_the_voice_just_after_it() {
        let map = SpeechMap::from_samples(&tone(4.0, &[(2.0, 3.0)]), 1000);
        let aligned = map.align(vec![word(1.0, 1.5, "はい")]);
        assert_eq!((aligned[0].start, aligned[0].end), (2.0, 2.0));
    }

    #[test]
    fn a_word_just_after_the_voice_keeps_its_place() {
        let map = SpeechMap::from_samples(&tone(4.0, &[(1.0, 2.0)]), 1000);
        let aligned = map.align(vec![word(2.5, 2.8, "はい")]);
        assert_eq!((aligned[0].start, aligned[0].end), (2.5, 2.5));
    }

    #[test]
    fn a_word_far_from_any_voice_was_never_spoken() {
        let map = SpeechMap::from_samples(&tone(8.0, &[(1.0, 2.0)]), 1000);
        assert_eq!(
            map.align(vec![word(5.0, 5.5, "ご視聴")]),
            Vec::<Word>::new()
        );
    }

    #[test]
    fn marks_keep_their_place_and_words_stay_in_order() {
        let map = SpeechMap::from_samples(&tone(4.0, &[(1.0, 2.0)]), 1000);
        let aligned = map.align(vec![
            word(1.0, 2.5, "はい"),
            word(3.0, 3.1, "。"),
            word(1.5, 1.7, "ね"),
        ]);
        assert_eq!(
            aligned
                .iter()
                .map(|word| (word.start, word.end))
                .collect::<Vec<_>>(),
            vec![(1.0, 2.0), (3.0, 3.1), (3.1, 3.1)]
        );
    }

    #[test]
    fn a_zero_length_word_still_finds_its_voice() {
        let map = SpeechMap::from_samples(&tone(4.0, &[(1.0, 2.0)]), 1000);
        let aligned = map.align(vec![word(1.5, 1.5, "は")]);
        assert_eq!(aligned[0].start, 1.5);
    }

    #[test]
    fn a_blip_is_not_voice_and_a_short_closure_does_not_end_it() {
        let map =
            SpeechMap::from_samples(&tone(4.0, &[(0.5, 0.51), (1.0, 2.0), (2.05, 3.0)]), 1000);
        assert_eq!(map.first_voiced(0.0, 0.9), None);
        assert_eq!(map.first_voiced(2.0, 2.05), Some(2.0));
    }

    #[test]
    fn steady_sound_is_judged_by_the_speech_floor_alone() {
        let loud = SpeechMap::from_samples(&[1000; 4000], 1000);
        assert_eq!(loud.first_voiced(0.0, 4.0), Some(0.0));
        let silent = SpeechMap::from_samples(&[0; 4000], 1000);
        assert_eq!(silent.first_voiced(0.0, 4.0), None);
    }

    #[test]
    fn no_samples_make_an_empty_map() {
        let map = SpeechMap::from_samples(&[], 1000);
        assert_eq!(map.first_voiced(0.0, 1.0), None);
    }

    fn wav_bytes(channels: u16) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF\0\0\0\0WAVE");
        bytes.extend_from_slice(b"fmt \x10\0\0\0");
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&16000u32.to_le_bytes());
        bytes.extend_from_slice(&32000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"LIST\x03\0\0\0abc\0");
        bytes.extend_from_slice(b"data\x04\0\0\0");
        bytes.extend_from_slice(&1i16.to_le_bytes());
        bytes.extend_from_slice(&(-2i16).to_le_bytes());
        bytes
    }

    #[test]
    fn a_wav_is_read_past_chunks_it_does_not_need() {
        assert_eq!(pcm_samples(&wav_bytes(1)), Some((vec![1, -2], 16000)));
    }

    #[test]
    fn audio_in_another_shape_is_not_read() {
        assert_eq!(pcm_samples(&wav_bytes(2)), None);
        assert_eq!(pcm_samples(b"not a wav file"), None);
        assert_eq!(pcm_samples(b"RIFF\0\0\0\0WAVE"), None);
        assert_eq!(pcm_samples(b"RIFF\0\0\0\0WAVEdata\x02\0\0\0\x01\0"), None);
    }

    #[test]
    fn a_missing_file_gives_no_map() {
        assert_eq!(
            SpeechMap::from_wav(Path::new("/nonexistent/audio.wav")),
            None
        );
    }

    #[test]
    fn a_file_on_disk_gives_a_map() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test-speech.wav");
        std::fs::write(&path, wav_bytes(1)).unwrap();
        let map = SpeechMap::from_wav(&path);
        std::fs::remove_file(&path).unwrap();
        assert!(map.is_some());
    }
}
