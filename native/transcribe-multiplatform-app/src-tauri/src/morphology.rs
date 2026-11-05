//! Phrase and morpheme boundaries for caption pages, from Rust Vibrato with the pinned IPADIC.
//!
//! Port of `replay.caption_linguistics`: conservative phrase candidates, not a dependency parse.
//! Offsets are character offsets, as in the Python. Text without Japanese breaks at spaces and
//! never loads the dictionary.

use crate::error::{io_context, AppError, AppResult};
use crate::paths::AppPaths;
use ruzstd::decoding::StreamingDecoder;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use unicode_segmentation::UnicodeSegmentation;
use vibrato::{Dictionary, Tokenizer};

const DICTIONARY_FILE: &str = "ipadic-mecab-2_7_0.dic.zst";
const DICTIONARY_SHA256: &str = "82a6da70bb4a17be70f20ff44f650f9ad1d2b0b4fcb2f39c17fc797f92d0ab75";
/// Unlimited unknown grouping can swallow whole clauses before POS handling sees them.
const MAX_GROUPING_LEN: usize = 1;
/// IPADIC gives dictionary words nine features; shorter rows are unknown words.
const DICTIONARY_FEATURES: usize = 9;
const DEPENDENT_PARTS: [&str; 2] = ["助詞", "助動詞"];
/// Marks that lean on the character before them (`DEPENDENT_KANA` in caption_linguistics).
pub(crate) const DEPENDENT_KANA: &str = "ぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮヵヶーﾞﾟ々ゝゞヽヾ";
/// Only repeated units with at least this many independent characters count as rhythm.
const RHYTHM_MINIMUM_UNIT: usize = 2;
/// One-character particles are too common at word starts to say anything.
const PARTICLE_PREFIX_MINIMUM: usize = 2;
const CLAUSE_MARKS: [char; 6] = [',', ';', ':', '、', '，', '。'];

/// What the text before a phrase end looks like; a page reads best when it ends at a strong one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BreakClass {
    /// After a mark or a space.
    Clause,
    /// After a final particle, or before an interjection, conjunction or filler (`semantic_break`).
    Closing,
    /// After a conjunctive particle such as けど or から.
    Connective,
    /// After a finished predicate (基本形), unless it modifies a noun next.
    Predicate,
    /// Between a case or topic particle and the verb or adjective it belongs to.
    Argument,
    /// Inside a run of nouns, which usually name one thing (実|食).
    Compound,
    /// Beside a word whose opening also reads as a particle (`からい` after て may be `から`).
    Ambiguous,
    /// After any other particle.
    Particle,
    /// A phrase end with nothing closing it.
    Plain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PhraseEnd {
    pub(crate) offset: usize,
    pub(crate) class: BreakClass,
}

/// Ascending candidate offsets, from most to least natural; each ends at the text's length.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Boundaries {
    pub(crate) phrases: Vec<PhraseEnd>,
    /// Phrase ends plus the end of a polite expression's interjection, for line breaks.
    pub(crate) line_ends: Vec<usize>,
    /// Morpheme ends without small kana, honorific suffixes or detached prefixes.
    pub(crate) atomic: Vec<usize>,
    /// Morpheme ends outside protected names (`boundaries()[1]`).
    pub(crate) morphemes: Vec<usize>,
    /// Every morpheme end that starts no dependent kana, for words too long to stay whole.
    pub(crate) overflow: Vec<usize>,
}

/// Loads the dictionary at most once per value: one value lives for one job.
pub struct Segmenter {
    dictionary: PathBuf,
    tokenizer: OnceLock<Tokenizer>,
}

struct Morpheme {
    start: usize,
    end: usize,
    surface: String,
    features: Vec<String>,
}

impl Morpheme {
    fn part(&self, index: usize) -> &str {
        self.features.get(index).map_or("", String::as_str)
    }

    fn is_known(&self) -> bool {
        self.features.len() >= DICTIONARY_FEATURES
    }
}

pub(crate) fn is_kana(c: char) -> bool {
    is_hiragana(c) || is_katakana(c)
}

fn is_hiragana(c: char) -> bool {
    matches!(c, '\u{3041}'..='\u{3096}' | '\u{309D}'..='\u{309F}')
}

fn is_katakana(c: char) -> bool {
    matches!(
        c,
        '\u{30A1}'..='\u{30FA}'
            | '\u{30FD}'..='\u{30FF}'
            | '\u{31F0}'..='\u{31FF}'
            | '\u{FF66}'..='\u{FF6F}'
            | '\u{FF71}'..='\u{FF9D}'
    )
}

fn is_han(c: char) -> bool {
    matches!(
        c,
        '々' | '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{20000}'..='\u{2FFFF}'
    )
}

/// `[\p{Script=Katakana}ーﾞﾟ]`
pub(crate) fn is_katakana_run(c: char) -> bool {
    is_katakana(c) || matches!(c, 'ー' | 'ﾞ' | 'ﾟ')
}

/// `[\p{Script=Latin}\p{Number}]`
pub(crate) fn is_latin_or_number(c: char) -> bool {
    c.is_numeric()
        || c.is_ascii_alphabetic()
        || matches!(c, '\u{00C0}'..='\u{00D6}' | '\u{00D8}'..='\u{00F6}' | '\u{00F8}'..='\u{024F}')
        || matches!(c, '\u{FF21}'..='\u{FF3A}' | '\u{FF41}'..='\u{FF5A}')
}

/// Text the dictionary applies to; everything else breaks at spaces.
pub(crate) fn is_japanese(text: &str) -> bool {
    text.chars()
        .any(|c| is_hiragana(c) || is_katakana(c) || is_han(c))
}

fn is_dependent_kana(c: char) -> bool {
    DEPENDENT_KANA.contains(c)
}

fn invalid_dictionary(error: impl Display) -> AppError {
    AppError::Invalid(format!("the caption dictionary could not be read: {error}"))
}

fn decode_dictionary(compressed: &[u8]) -> AppResult<Tokenizer> {
    let decoder = StreamingDecoder::new(compressed).map_err(invalid_dictionary)?;
    let dictionary = Dictionary::read(decoder).map_err(invalid_dictionary)?;
    let tokenizer = Tokenizer::new(dictionary)
        .ignore_space(false)
        .map_err(invalid_dictionary)?;
    Ok(tokenizer.max_grouping_len(MAX_GROUPING_LEN))
}

fn load_tokenizer(path: &Path) -> AppResult<Tokenizer> {
    let compressed = std::fs::read(path).map_err(io_context("read the caption dictionary"))?;
    let digest: String = Sha256::digest(&compressed)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if digest != DICTIONARY_SHA256 {
        return Err(invalid_dictionary("its checksum does not match"));
    }
    decode_dictionary(&compressed)
}

/// Keeps a lone hiragana function word with the kana name after it, as in `に|しの|さん`.
fn honorific_span(items: &[Morpheme], suffix: usize) -> Option<(usize, usize)> {
    if items[suffix].part(1) != "接尾" || items[suffix].part(2) != "人名" {
        return None;
    }
    let mut first = suffix;
    while first > 0 && is_kana_noun(&items[first - 1]) {
        first -= 1;
    }
    if first == 0 || first == suffix {
        return None;
    }
    let leading = &items[first - 1];
    let single_hiragana =
        leading.surface.chars().count() == 1 && leading.surface.chars().all(is_hiragana);
    if !DEPENDENT_PARTS.contains(&leading.part(0)) || !single_hiragana {
        return None;
    }
    let opens_phrase = first == 1
        || matches!(
            items[first - 2].part(0),
            "助詞" | "感動詞" | "記号" | "連体詞"
        );
    opens_phrase.then_some((leading.start, items[suffix].end))
}

fn is_kana_noun(item: &Morpheme) -> bool {
    item.part(0) == "名詞"
        && item.part(1) != "接尾"
        && item
            .surface
            .chars()
            .all(|c| is_hiragana(c) || is_katakana(c) || c == 'ー')
}

/// Character offsets where each grapheme ends, ascending.
pub(crate) fn grapheme_ends(text: &str) -> Vec<usize> {
    let mut end = 0;
    text.graphemes(true)
        .map(|grapheme| {
            end += grapheme.chars().count();
            end
        })
        .collect()
}

fn break_class(previous: &Morpheme, item: &Morpheme) -> BreakClass {
    if previous.part(0) == "記号" {
        return BreakClass::Clause;
    }
    // A conjunction or filler opens a new thought (でも, だから, まぁ), whatever came before.
    if matches!(item.part(0), "感動詞" | "接続詞" | "フィラー") || previous.part(1) == "終助詞"
    {
        return BreakClass::Closing;
    }
    if matches!(previous.part(0), "動詞" | "形容詞" | "助動詞")
        && previous.part(5) == "基本形"
        && item.part(0) != "名詞"
    {
        return BreakClass::Predicate;
    }
    if previous.part(1) == "接続助詞" {
        return BreakClass::Connective;
    }
    if matches!(previous.part(1), "格助詞" | "係助詞") && matches!(item.part(0), "動詞" | "形容詞")
    {
        return BreakClass::Argument;
    }
    if previous.part(0) == "助詞" {
        return BreakClass::Particle;
    }
    if previous.part(0) == "名詞" && item.part(0) == "名詞" && item.part(1) != "代名詞" {
        return BreakClass::Compound;
    }
    BreakClass::Plain
}

/// An adjective before a noun modifies it (今日は|青い空): no predicate is being cut off.
fn relaxed_modifier(class: BreakClass, item: &Morpheme, next: Option<&Morpheme>) -> BreakClass {
    let modifies = item.part(0) == "形容詞" && next.is_some_and(|word| word.part(0) == "名詞");
    if class == BreakClass::Argument && modifies {
        return BreakClass::Particle;
    }
    class
}

/// Whisper's space marks a segment edge, not grammar: it reads as a clause break, unless it falls
/// between a particle and the predicate it belongs to (が| いる).
fn spaced_break_class(before: &[Morpheme], item: &Morpheme) -> BreakClass {
    let class = before
        .last()
        .map_or(BreakClass::Plain, |last| break_class(last, item));
    let Some(word) = before
        .iter()
        .rev()
        .find(|earlier| earlier.part(1) != "空白")
    else {
        return class;
    };
    if break_class(word, item) == BreakClass::Argument {
        return BreakClass::Argument;
    }
    class
}

/// A dependent or prefix-bound morpheme never opens a phrase; 非自立 follows
/// `caption_phrase_fragments`, so 来て|くれて and 植え|ちゃった stay whole.
fn opens_phrase(previous: &Morpheme, item: &Morpheme) -> bool {
    let dependent = DEPENDENT_PARTS.contains(&item.part(0))
        || matches!(item.part(1), "接尾" | "非自立")
        || (item.part(0) == "記号" && item.part(1) != "括弧開");
    let after_prefix = previous.part(0) == "接頭詞" || previous.part(1) == "括弧開";
    // A light する belongs to what it verbalizes (勉強|する, もしか|したら), unless a particle
    // already closed that (本を|する).
    let light_verb = item.part(0) == "動詞"
        && item.part(6) == "する"
        && !matches!(previous.part(0), "助詞" | "記号");
    !dependent && !after_prefix && !light_verb
}

/// Inside a run of katakana or Latin, an uncertain name stays whole.
fn splits_uncertain_name(chars: &[char], previous: &Morpheme, item: &Morpheme) -> bool {
    let (before, after) = (chars[item.start - 1], chars[item.start]);
    let inside_run = (is_katakana_run(before) && is_katakana_run(after))
        || (is_latin_or_number(before) && is_latin_or_number(after));
    let uncertain = !item.is_known()
        || item.part(1) == "固有名詞"
        || !previous.is_known()
        || previous.part(1) == "固有名詞";
    inside_run && uncertain
}

/// The shortest unit a katakana run only repeats (`_rhythmic_ends`), such as ハッハッハ.
fn rhythm_unit(run: &[char]) -> Option<usize> {
    (RHYTHM_MINIMUM_UNIT..=run.len() / 2).find(|&size| {
        let independent = run[..size]
            .iter()
            .filter(|c| !is_dependent_kana(**c))
            .count();
        !is_dependent_kana(run[0])
            && independent >= RHYTHM_MINIMUM_UNIT
            && run
                .iter()
                .enumerate()
                .all(|(index, c)| *c == run[index % size])
    })
}

/// Repeated phonetic units are stronger evidence than incidental POS fragments inside them.
fn rhythmic_ends(chars: &[char], ends: BTreeSet<usize>) -> BTreeSet<usize> {
    let mut result = ends;
    let mut start = 0;
    while start < chars.len() {
        if !is_katakana_run(chars[start]) {
            start += 1;
            continue;
        }
        let run_len = chars[start..]
            .iter()
            .take_while(|c| is_katakana_run(**c))
            .count();
        let end = start + run_len;
        if let Some(size) = rhythm_unit(&chars[start..end]) {
            result.retain(|point| !(start < *point && *point < end));
            result.extend((1..run_len / size).map(|count| start + size * count));
            result.insert(end);
        }
        start = end;
    }
    result
}

fn dependent_or_suffix_start(chars: &[char], previous: &Morpheme, item: &Morpheme) -> bool {
    previous.part(0) == "接頭詞"
        || previous.part(1) == "括弧開"
        || item.part(1) == "接尾"
        || is_dependent_kana(chars[item.start])
}

fn analyze(text: &str, items: &[Morpheme]) -> Boundaries {
    let chars: Vec<char> = text.chars().collect();
    let graphemes: HashSet<usize> = grapheme_ends(text).into_iter().collect();
    let mut protected: HashSet<usize> = (1..chars.len())
        .filter(|point| !graphemes.contains(point))
        .collect();
    let names: Vec<(usize, usize)> = (0..items.len())
        .filter_map(|index| honorific_span(items, index))
        .collect();
    let name_starts: HashSet<usize> = names.iter().map(|(start, _)| *start).collect();
    protected.extend(names.iter().flat_map(|(start, end)| *start + 1..*end));
    let mut phrases: Vec<PhraseEnd> = Vec::new();
    let mut atomic: BTreeSet<usize> = BTreeSet::new();
    let mut polite: Vec<usize> = Vec::new();
    for (index, pair) in items.windows(2).enumerate() {
        let (previous, item) = (&pair[0], &pair[1]);
        if is_dependent_kana(chars[item.start]) || splits_uncertain_name(&chars, previous, item) {
            protected.insert(item.start);
        }
        if previous.part(0) == "感動詞" && item.part(0) == "助動詞" {
            polite.push(item.start);
        }
        if protected.contains(&item.start) {
            continue;
        }
        if name_starts.contains(&item.start) || opens_phrase(previous, item) {
            phrases.push(PhraseEnd {
                offset: item.start,
                class: relaxed_modifier(
                    spaced_break_class(&items[..=index], item),
                    item,
                    items.get(index + 2),
                ),
            });
        }
        if !dependent_or_suffix_start(&chars, previous, item) {
            atomic.insert(item.start);
        }
    }
    let length = chars.len();
    let phrase_points: BTreeSet<usize> = phrases.iter().map(|phrase| phrase.offset).collect();
    let line_points: BTreeSet<usize> = phrase_points
        .iter()
        .copied()
        .chain(
            polite
                .into_iter()
                .filter(|point| !protected.contains(point)),
        )
        .chain([length])
        .collect();
    atomic.insert(length);
    // A repetition is broken only when it cannot fit whole: ボロ|ボロ never ends a page by choice.
    let rhythm = rhythmic_ends(&chars, phrase_points);
    phrases.retain(|phrase| rhythm.contains(&phrase.offset));
    phrases.push(PhraseEnd {
        offset: length,
        class: BreakClass::Plain,
    });
    let line_ends = rhythmic_ends(&chars, line_points.clone())
        .intersection(&line_points)
        .copied()
        .collect();
    let morphemes = items
        .iter()
        .map(|item| item.end)
        .filter(|end| !protected.contains(end))
        .collect();
    let overflow = items
        .iter()
        .map(|item| item.end)
        .filter(|end| {
            graphemes.contains(end) && chars.get(*end).is_none_or(|c| !is_dependent_kana(*c))
        })
        .collect();
    Boundaries {
        phrases,
        line_ends,
        atomic: rhythmic_ends(&chars, atomic).into_iter().collect(),
        morphemes,
        overflow,
    }
}

/// Word boundaries of text without Japanese: after each run of spaces.
fn spaced_boundaries(text: &str) -> Boundaries {
    let chars: Vec<char> = text.chars().collect();
    let mut phrases: Vec<PhraseEnd> = chars
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0].is_whitespace() && !pair[1].is_whitespace())
        .map(|(index, _)| PhraseEnd {
            offset: index + 1,
            class: if index > 0 && CLAUSE_MARKS.contains(&chars[index - 1]) {
                BreakClass::Clause
            } else {
                BreakClass::Plain
            },
        })
        .collect();
    phrases.push(PhraseEnd {
        offset: chars.len(),
        class: BreakClass::Plain,
    });
    let offsets: Vec<usize> = phrases.iter().map(|phrase| phrase.offset).collect();
    Boundaries {
        phrases,
        line_ends: offsets.clone(),
        atomic: offsets.clone(),
        morphemes: offsets.clone(),
        overflow: offsets,
    }
}

impl Segmenter {
    #[must_use]
    pub fn new(dictionary: PathBuf) -> Self {
        Self {
            dictionary,
            tokenizer: OnceLock::new(),
        }
    }

    /// The segmenter for one job, reading the dictionary bundled with the app.
    #[must_use]
    pub fn for_app(paths: &AppPaths) -> Self {
        Self::new(paths.resources.join("resources").join(DICTIONARY_FILE))
    }

    fn tokenizer(&self) -> AppResult<&Tokenizer> {
        match self.tokenizer.get() {
            Some(tokenizer) => Ok(tokenizer),
            None => {
                let loaded = load_tokenizer(&self.dictionary)?;
                Ok(self.tokenizer.get_or_init(|| loaded))
            }
        }
    }

    fn morphemes(&self, text: &str) -> AppResult<Vec<Morpheme>> {
        let mut worker = self.tokenizer()?.new_worker();
        worker.reset_sentence(text);
        worker.tokenize();
        Ok((0..worker.num_tokens())
            .map(|index| {
                let token = worker.token(index);
                let range = token.range_char();
                Morpheme {
                    start: range.start,
                    end: range.end,
                    surface: token.surface().to_owned(),
                    features: token.feature().split(',').map(str::to_owned).collect(),
                }
            })
            .collect())
    }

    /// Whether `text` opens with something that cannot stand alone; text without Japanese never does.
    pub(crate) fn leans_on_previous(&self, text: &str) -> AppResult<bool> {
        if !is_japanese(text) {
            return Ok(false);
        }
        Ok(self.morphemes(text)?.first().is_some_and(|item| {
            DEPENDENT_PARTS.contains(&item.part(0))
                || matches!(item.part(1), "接尾" | "非自立")
                || item.surface.chars().next().is_some_and(is_dependent_kana)
        }))
    }

    /// Whether a hiragana word opens with something that reads as a particle by itself.
    fn opens_with_particle(&self, item: &Morpheme) -> AppResult<bool> {
        let chars: Vec<char> = item.surface.chars().collect();
        if !chars.first().is_some_and(|c| is_hiragana(*c)) {
            return Ok(false);
        }
        for length in PARTICLE_PREFIX_MINIMUM..chars.len() {
            let prefix: String = chars[..length].iter().collect();
            let parts = self.morphemes(&prefix)?;
            if parts.len() == 1 && parts[0].part(0) == "助詞" {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Candidate break offsets of `text`. Only text with Japanese loads the dictionary.
    pub(crate) fn boundaries(&self, text: &str) -> AppResult<Boundaries> {
        if !is_japanese(text) {
            return Ok(spaced_boundaries(text));
        }
        let items = self.morphemes(text)?;
        let mut boundaries = analyze(text, &items);
        for phrase in &mut boundaries.phrases {
            // A misread word makes both of its edges unreliable.
            let touching = items
                .iter()
                .filter(|item| item.start == phrase.offset || item.end == phrase.offset);
            if !matches!(phrase.class, BreakClass::Clause | BreakClass::Closing) {
                for item in touching {
                    if self.opens_with_particle(item)? {
                        phrase.class = BreakClass::Ambiguous;
                    }
                }
            }
        }
        Ok(boundaries)
    }
}

#[cfg(test)]
pub(crate) fn test_segmenter() -> &'static Segmenter {
    static SHARED: OnceLock<Segmenter> = OnceLock::new();
    SHARED.get_or_init(|| {
        Segmenter::new(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("resources")
                .join(DICTIONARY_FILE),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phrase_offsets(text: &str) -> Vec<usize> {
        test_segmenter()
            .boundaries(text)
            .unwrap()
            .phrases
            .iter()
            .map(|phrase| phrase.offset)
            .collect()
    }

    #[test]
    fn the_pinned_dictionary_reads_with_its_recorded_morphology() {
        let items = test_segmenter()
            .morphemes("今日は青い空を見ながら、お茶を飲みました。")
            .unwrap();
        assert_eq!(items[0].surface, "今日");
        assert_eq!(
            items[0].features.join(","),
            "名詞,副詞可能,*,*,*,*,今日,キョウ,キョー"
        );
        assert_eq!(items[2].surface, "青い");
        assert_eq!(
            items[2].features.join(","),
            "形容詞,自立,*,*,形容詞・アウオ段,基本形,青い,アオイ,アオイ"
        );
        assert_eq!((items[4].start, items[4].end), (6, 7));
    }

    #[test]
    fn phrases_end_after_their_particles() {
        let boundaries = test_segmenter()
            .boundaries("今日は青い空を見ながら、お茶を飲みました。")
            .unwrap();
        assert_eq!(
            boundaries.phrases,
            vec![
                PhraseEnd {
                    offset: 3,
                    class: BreakClass::Particle
                },
                PhraseEnd {
                    offset: 5,
                    class: BreakClass::Plain
                },
                PhraseEnd {
                    offset: 7,
                    class: BreakClass::Argument
                },
                PhraseEnd {
                    offset: 12,
                    class: BreakClass::Clause
                },
                PhraseEnd {
                    offset: 15,
                    class: BreakClass::Argument
                },
                PhraseEnd {
                    offset: 21,
                    class: BreakClass::Plain
                },
            ]
        );
        assert_eq!(boundaries.morphemes[..3], [2, 3, 5]);
    }

    #[test]
    fn the_end_of_a_clause_with_a_conjunction_or_final_particle_is_classified() {
        let boundaries = test_segmenter()
            .boundaries("行くけど雨だよね明日は")
            .unwrap();
        assert_eq!(
            boundaries.phrases[..3].to_vec(),
            vec![
                PhraseEnd {
                    offset: 4,
                    class: BreakClass::Connective
                },
                PhraseEnd {
                    offset: 8,
                    class: BreakClass::Closing
                },
                PhraseEnd {
                    offset: 11,
                    class: BreakClass::Plain
                },
            ]
        );
    }

    #[test]
    fn an_interjection_opens_a_closing_break() {
        let boundaries = test_segmenter().boundaries("雨だありがとう").unwrap();
        assert_eq!(boundaries.phrases[0].offset, 2);
        assert_eq!(boundaries.phrases[0].class, BreakClass::Closing);
    }

    #[test]
    fn auxiliary_verbs_and_formal_nouns_stay_with_their_verb() {
        assert_eq!(phrase_offsets("来てくれてありがとう"), vec![5, 10]);
        assert_eq!(phrase_offsets("植えちゃった"), vec![6]);
    }

    #[test]
    fn small_kana_never_starts_a_phrase_or_ends_a_morpheme() {
        let boundaries = test_segmenter()
            .boundaries("お腹いっぱいなんだけど")
            .unwrap();
        assert!(!boundaries.morphemes.contains(&3));
        assert_eq!(phrase_offsets("お腹いっぱいなんだけど"), vec![11]);
    }

    #[test]
    fn an_uncertain_katakana_or_latin_name_stays_whole() {
        let boundaries = test_segmenter().boundaries("ニシノヤスタカさん").unwrap();
        assert_eq!(boundaries.phrases[0].offset, 9);
        let latin = test_segmenter().boundaries("TikTokの話").unwrap();
        assert!(!latin.morphemes.contains(&3));
    }

    #[test]
    fn a_kana_name_keeps_its_leading_function_word_homograph() {
        let boundaries = test_segmenter().boundaries("へえにしのさん").unwrap();
        assert_eq!(phrase_offsets("へえにしのさん"), vec![2, 7]);
        assert_eq!(boundaries.phrases[0].class, BreakClass::Plain);
    }

    #[test]
    fn a_name_after_a_verb_keeps_its_ordinary_phrases() {
        assert_eq!(phrase_offsets("行くにしのさん"), vec![3, 7]);
    }

    #[test]
    fn repeated_units_break_only_when_they_cannot_fit() {
        let boundaries = test_segmenter().boundaries("ボロボロになって").unwrap();
        assert_eq!(boundaries.phrases[0].offset, 5);
        let laugh = test_segmenter().boundaries("ハッハッハッハッ").unwrap();
        assert_eq!(laugh.atomic, vec![4, 8]);
        assert_eq!(laugh.line_ends, vec![4, 8]);
    }

    #[test]
    fn a_polite_expression_may_wrap_after_its_interjection() {
        let boundaries = test_segmenter().boundaries("ありがとうございます").unwrap();
        assert_eq!(boundaries.phrases[0].offset, 10);
        assert_eq!(boundaries.line_ends, vec![5, 10]);
    }

    #[test]
    fn a_prefix_stays_with_its_word() {
        let boundaries = test_segmenter().boundaries("お名前を書く").unwrap();
        assert_eq!(boundaries.atomic, vec![3, 4, 6]);
        let opening = test_segmenter().boundaries("（今日）の話").unwrap();
        assert_eq!(opening.phrases[0].offset, 5);
    }

    #[test]
    fn text_without_japanese_breaks_after_spaces_and_clauses() {
        let boundaries = spaced_boundaries("Hello, big world");
        assert_eq!(
            boundaries.phrases,
            vec![
                PhraseEnd {
                    offset: 7,
                    class: BreakClass::Clause
                },
                PhraseEnd {
                    offset: 11,
                    class: BreakClass::Plain
                },
                PhraseEnd {
                    offset: 16,
                    class: BreakClass::Plain
                },
            ]
        );
        assert_eq!(boundaries.line_ends, vec![7, 11, 16]);
        assert_eq!(boundaries.overflow, vec![7, 11, 16]);
    }

    #[test]
    fn only_kana_and_kanji_count_as_japanese() {
        assert!(is_japanese("abcあ"));
        assert!(is_japanese("カタカナ"));
        assert!(is_japanese("漢"));
        assert!(!is_japanese("hello 123"));
        assert!(is_latin_or_number('Ａ'));
        assert!(is_latin_or_number('é'));
        assert!(!is_latin_or_number('×'));
    }

    #[test]
    fn a_grapheme_cluster_is_never_split() {
        assert_eq!(grapheme_ends("か\u{3099}あ").len(), 2);
        let boundaries = test_segmenter().boundaries("か\u{3099}あ").unwrap();
        assert!(!boundaries.morphemes.contains(&1));
    }

    #[test]
    fn the_app_finds_the_dictionary_beside_its_other_resources() {
        let paths = AppPaths {
            data: PathBuf::from("/d"),
            resources: PathBuf::from("/r"),
            hf_hub: None,
        };
        assert_eq!(
            Segmenter::for_app(&paths).dictionary,
            PathBuf::from("/r/resources/ipadic-mecab-2_7_0.dic.zst")
        );
    }

    #[test]
    fn a_dictionary_with_the_wrong_checksum_is_refused() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-test-wrong-dictionary");
        std::fs::write(&path, b"not a dictionary").unwrap();
        let error = Segmenter::new(path.clone()).boundaries("はい").unwrap_err();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            error.to_string(),
            "the caption dictionary could not be read: its checksum does not match"
        );
    }

    #[test]
    fn a_missing_dictionary_names_the_failed_step() {
        let error = Segmenter::new(PathBuf::from("/nonexistent/dictionary"))
            .boundaries("はい")
            .unwrap_err();
        assert!(error
            .to_string()
            .starts_with("read the caption dictionary: "));
    }

    #[test]
    fn bytes_that_are_not_zstd_are_refused() {
        let error = decode_dictionary(b"not zstd").err().unwrap();
        assert!(error
            .to_string()
            .starts_with("the caption dictionary could not be read: "));
    }

    #[test]
    fn a_zstd_stream_that_is_no_dictionary_is_refused() {
        // A raw zstd frame holding the single byte "x".
        let frame = [0x28, 0xB5, 0x2F, 0xFD, 0x20, 0x01, 0x09, 0x00, 0x00, b'x'];
        let error = decode_dictionary(&frame).err().unwrap();
        assert!(error
            .to_string()
            .starts_with("the caption dictionary could not be read: "));
    }

    #[test]
    fn a_morpheme_without_features_reads_as_empty() {
        let item = Morpheme {
            start: 0,
            end: 1,
            surface: "あ".to_owned(),
            features: Vec::new(),
        };
        assert_eq!(item.part(3), "");
        assert!(!item.is_known());
    }

    #[test]
    fn a_light_verb_stays_with_what_it_verbalizes() {
        assert_eq!(phrase_offsets("もしかしたら私"), vec![6, 7]);
        assert_eq!(phrase_offsets("勉強します"), vec![5]);
    }

    #[test]
    fn a_light_verb_after_a_particle_keeps_its_phrase() {
        assert_eq!(phrase_offsets("本をする"), vec![2, 4]);
    }

    #[test]
    fn a_word_that_opens_like_a_particle_is_an_ambiguous_break_on_both_sides() {
        let boundaries = test_segmenter()
            .boundaries("振ってからいある入れた")
            .unwrap();
        assert_eq!(boundaries.phrases[0].offset, 3);
        assert_eq!(boundaries.phrases[0].class, BreakClass::Ambiguous);
        assert_eq!(boundaries.phrases[1].offset, 6);
        assert_eq!(boundaries.phrases[1].class, BreakClass::Ambiguous);
    }

    #[test]
    fn a_finished_predicate_is_a_strong_break_unless_it_modifies_a_noun() {
        let finished = test_segmenter().boundaries("行くもうすぐ").unwrap();
        assert_eq!(finished.phrases[0].class, BreakClass::Predicate);
        let modifying = test_segmenter().boundaries("行く人").unwrap();
        assert_eq!(modifying.phrases[0].class, BreakClass::Plain);
    }

    #[test]
    fn a_particle_and_its_predicate_are_a_weak_break_even_across_a_space() {
        let plain = test_segmenter().boundaries("犬がいる").unwrap();
        assert_eq!(plain.phrases[0].class, BreakClass::Argument);
        let spaced = test_segmenter().boundaries("犬が いる").unwrap();
        assert_eq!(spaced.phrases[0].class, BreakClass::Argument);
        let clause = test_segmenter().boundaries("犬 猫").unwrap();
        assert_eq!(clause.phrases[0].class, BreakClass::Clause);
    }

    #[test]
    fn a_text_of_only_a_space_before_a_word_has_no_earlier_word() {
        let boundaries = test_segmenter().boundaries(" 犬").unwrap();
        assert_eq!(boundaries.phrases.last().unwrap().offset, 2);
        let item = Morpheme {
            start: 1,
            end: 2,
            surface: "犬".to_owned(),
            features: vec!["名詞".to_owned(), "一般".to_owned()],
        };
        let space = Morpheme {
            start: 0,
            end: 1,
            surface: " ".to_owned(),
            features: vec!["記号".to_owned(), "空白".to_owned()],
        };
        assert_eq!(spaced_break_class(&[space], &item), BreakClass::Clause);
        assert_eq!(spaced_break_class(&[], &item), BreakClass::Plain);
    }

    #[test]
    fn dependent_openings_lean_on_the_word_before() {
        assert!(test_segmenter().leans_on_previous("ってなる").unwrap());
        assert!(test_segmenter().leans_on_previous("ですね").unwrap());
        assert!(!test_segmenter().leans_on_previous("そうです").unwrap());
        assert!(!test_segmenter().leans_on_previous("hello").unwrap());
    }

    #[test]
    fn a_run_of_nouns_is_a_weak_break_unless_a_pronoun_opens_it() {
        let compound = test_segmenter().boundaries("学校図書館").unwrap();
        assert_eq!(compound.phrases[0].class, BreakClass::Compound);
        let pronoun = test_segmenter().boundaries("明日私").unwrap();
        assert_eq!(pronoun.phrases[0].class, BreakClass::Plain);
    }

    #[test]
    fn a_conjunction_or_filler_opens_a_new_thought() {
        let conjunction = test_segmenter().boundaries("行くしかし雨").unwrap();
        assert_eq!(conjunction.phrases[0].offset, 2);
        assert_eq!(conjunction.phrases[0].class, BreakClass::Closing);
    }
}
