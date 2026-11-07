//! Joins the words of two overlapping Whisper batches where both heard the same thing, so the join
//! falls between words and says nothing twice or not at all.

use crate::captions::Word;
use crate::engines::NOT_SPOKEN;

/// The same speech in two batches is at nearly the same time; a match further apart is another sentence.
const MOST_APART_SECONDS: f64 = 3.0;
/// Fewer characters in common can be chance: a filler said twice.
const SHORTEST_MATCH_CHARACTERS: usize = 6;

/// A spoken character and the word it belongs to.
#[derive(Debug)]
struct Spoken {
    character: char,
    word: usize,
    starts_word: bool,
    at: f64,
}

/// The longest stretch both sides say at about the same time, nearest the middle among equals.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SharedRun {
    left: usize,
    right: usize,
    length: usize,
    off_middle: f64,
}

fn spoken_characters(words: &[Word], wanted: impl Fn(&Word) -> bool) -> Vec<Spoken> {
    words
        .iter()
        .enumerate()
        .filter(|(_, word)| wanted(word))
        .flat_map(|(index, word)| {
            word.text
                .chars()
                .filter(|c| !c.is_whitespace() && !NOT_SPOKEN.contains(*c))
                .enumerate()
                .map(move |(place, character)| Spoken {
                    character,
                    word: index,
                    starts_word: place == 0,
                    at: word.start,
                })
        })
        .collect()
}

fn better(found: SharedRun, best: Option<SharedRun>) -> Option<SharedRun> {
    match best {
        Some(best)
            if best.length > found.length
                || (best.length == found.length && best.off_middle <= found.off_middle) =>
        {
            Some(best)
        }
        _ => Some(found),
    }
}

fn longest_shared_run(left: &[Spoken], right: &[Spoken], middle: f64) -> Option<SharedRun> {
    let mut best = None;
    let mut previous = vec![0_usize; right.len() + 1];
    for (at_left, one) in left.iter().enumerate() {
        let mut current = vec![0_usize; right.len() + 1];
        for (at_right, other) in right.iter().enumerate() {
            let same =
                one.character == other.character && (one.at - other.at).abs() <= MOST_APART_SECONDS;
            if !same {
                continue;
            }
            let length = previous[at_right] + 1;
            current[at_right + 1] = length;
            if length >= SHORTEST_MATCH_CHARACTERS {
                best = better(
                    SharedRun {
                        left: at_left + 1 - length,
                        right: at_right + 1 - length,
                        length,
                        off_middle: (one.at - middle).abs(),
                    },
                    best,
                );
            }
        }
        previous = current;
    }
    best
}

/// The words to keep of the earlier batch and the first to take of the later one: at a word start both
/// share inside their longest run, the one nearest its middle.
fn seam_in_shared_run(left: &[Spoken], right: &[Spoken], run: SharedRun) -> Option<(usize, usize)> {
    (0..run.length)
        .filter(|place| left[run.left + place].starts_word && right[run.right + place].starts_word)
        .min_by_key(|place| place.abs_diff(run.length / 2))
        .map(|place| (left[run.left + place].word, right[run.right + place].word))
}

fn is_before(word: &Word, time: f64) -> bool {
    (word.start + word.end) / 2.0 < time
}

/// `next`, in the same clock as `kept`, replaces it from where the two agree inside `overlap`. Where they
/// never agree, as over silence or a sentence one of them lost, each keeps its side of the middle.
#[must_use]
pub fn spliced_batches(mut kept: Vec<Word>, next: Vec<Word>, overlap: (f64, f64)) -> Vec<Word> {
    let middle = (overlap.0 + overlap.1) / 2.0;
    let left = spoken_characters(&kept, |word| !is_before(word, overlap.0));
    let right = spoken_characters(&next, |word| is_before(word, overlap.1));
    let seam = longest_shared_run(&left, &right, middle)
        .and_then(|run| seam_in_shared_run(&left, &right, run));
    let (keep, take) = seam.unwrap_or_else(|| {
        (
            kept.partition_point(|word| is_before(word, middle)),
            next.partition_point(|word| is_before(word, middle)),
        )
    });
    kept.truncate(keep);
    kept.extend(next.into_iter().skip(take));
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, start: f64) -> Word {
        Word {
            start,
            end: start + 1.0,
            text: text.to_owned(),
            speaker: None,
        }
    }

    fn texts(words: &[Word]) -> Vec<&str> {
        words.iter().map(|word| word.text.as_str()).collect()
    }

    #[test]
    fn the_batches_join_at_a_word_both_heard_with_nothing_twice() {
        let kept = vec![
            word("まえの", 90.0),
            word("ことば", 92.0),
            word("から", 94.0),
            word("おなじ", 96.0),
            word("はなしを", 98.0),
            word("しま", 100.0),
        ];
        let next = vec![
            word("ことば", 92.0),
            word("から", 94.0),
            word("おなじ", 96.0),
            word("はなしを", 98.0),
            word("します", 100.0),
            word("つぎの", 102.0),
        ];
        assert_eq!(
            texts(&spliced_batches(kept, next, (92.0, 102.0))),
            vec![
                "まえの",
                "ことば",
                "から",
                "おなじ",
                "はなしを",
                "します",
                "つぎの"
            ]
        );
    }

    #[test]
    fn differently_cut_words_join_where_both_start_a_word() {
        let kept = vec![
            word("きょうは", 90.0),
            word("いいてん", 92.0),
            word("きです", 94.0),
            word("ね", 96.0),
        ];
        let next = vec![
            word("きょう", 90.0),
            word("は", 91.0),
            word("いい", 92.0),
            word("てんきです", 93.0),
            word("ねえ", 96.0),
        ];
        assert_eq!(
            texts(&spliced_batches(kept, next, (90.0, 97.0))),
            vec!["きょうは", "いい", "てんきです", "ねえ"]
        );
    }

    #[test]
    fn the_same_words_said_again_later_are_not_a_match() {
        let kept = vec![word("おなじことばです", 90.0)];
        let next = vec![word("おなじことばです", 105.0)];
        assert_eq!(
            texts(&spliced_batches(kept, next, (88.0, 108.0))),
            vec!["おなじことばです", "おなじことばです"]
        );
    }

    #[test]
    fn with_nothing_in_common_each_keeps_its_side_of_the_middle() {
        let kept = vec![
            word("まえ", 90.0),
            word("まんなかまえ", 94.0),
            word("あと", 99.0),
        ];
        let next = vec![word("さき", 91.0), word("うしろ", 101.0)];
        assert_eq!(
            texts(&spliced_batches(kept, next, (90.0, 100.0))),
            vec!["まえ", "まんなかまえ", "うしろ"]
        );
    }

    #[test]
    fn silence_in_the_overlap_keeps_both_sides_whole() {
        let kept = vec![word("まえ", 50.0)];
        let next = vec![word("あと", 150.0)];
        assert_eq!(
            texts(&spliced_batches(kept, next, (90.0, 100.0))),
            vec!["まえ", "あと"]
        );
    }

    #[test]
    fn of_two_equal_runs_the_one_nearer_the_middle_decides() {
        let later_seam = better(
            SharedRun {
                left: 1,
                right: 1,
                length: 6,
                off_middle: 1.0,
            },
            Some(SharedRun {
                left: 0,
                right: 0,
                length: 6,
                off_middle: 5.0,
            }),
        );
        assert_eq!(later_seam.unwrap().left, 1);
    }

    #[test]
    fn a_run_with_no_shared_word_start_leaves_the_seam_to_the_clock() {
        let left = spoken_characters(&[word("あいうえおか", 90.0)], |_| true);
        let right = spoken_characters(&[word("あ", 90.0), word("いうえおか", 91.0)], |_| {
            true
        });
        let run = SharedRun {
            left: 1,
            right: 1,
            length: 5,
            off_middle: 0.0,
        };
        assert_eq!(seam_in_shared_run(&left, &right, run), None);
    }
}
