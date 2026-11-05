//! Holds the diarizer's turns to the number of speakers the recording really has.

use crate::captions::SpeakerTurn;

/// In automatic mode a "speaker" with less speech than this is a diarizer mistake, not a person.
const MIN_SPEAKER_SHARE: f64 = 0.02;

fn speech_seconds(turns: &[SpeakerTurn], speaker: u32) -> f64 {
    turns
        .iter()
        .filter(|turn| turn.speaker == speaker)
        .map(|turn| turn.end - turn.start)
        .sum()
}

fn first_appearance(turns: &[SpeakerTurn]) -> Vec<u32> {
    let mut speakers: Vec<u32> = Vec::new();
    for turn in turns {
        if !speakers.contains(&turn.speaker) {
            speakers.push(turn.speaker);
        }
    }
    speakers
}

/// The speakers to keep: the `wanted` who talk most, or in automatic mode (`wanted` 0) all with real speech.
fn kept_speakers(turns: &[SpeakerTurn], wanted: u32) -> Vec<u32> {
    let speakers = first_appearance(turns);
    let total: f64 = turns.iter().map(|turn| turn.end - turn.start).sum();
    if wanted == 0 {
        let real: Vec<u32> = speakers
            .iter()
            .copied()
            .filter(|speaker| speech_seconds(turns, *speaker) >= total * MIN_SPEAKER_SHARE)
            .collect();
        return real;
    }
    let mut ranked = speakers.clone();
    ranked.sort_by(|left, right| {
        speech_seconds(turns, *right).total_cmp(&speech_seconds(turns, *left))
    });
    ranked.truncate(wanted as usize);
    speakers
        .into_iter()
        .filter(|speaker| ranked.contains(speaker))
        .collect()
}

fn gap_between(left: &SpeakerTurn, right: &SpeakerTurn) -> f64 {
    (right.start - left.end)
        .max(left.start - right.end)
        .max(0.0)
}

/// A dropped speaker's turn goes to whichever kept speaker talks closest to it in time.
fn nearest_kept(turn: &SpeakerTurn, turns: &[SpeakerTurn], kept: &[u32]) -> Option<u32> {
    turns
        .iter()
        .filter(|other| kept.contains(&other.speaker))
        .min_by(|left, right| gap_between(turn, left).total_cmp(&gap_between(turn, right)))
        .map(|other| other.speaker)
}

/// Reassigns turns so that at most `wanted` speakers remain (`wanted` 0: automatic) and numbers them by
/// who speaks first, so speaker 1 and its colour always belong to the first voice.
#[must_use]
pub fn limit_speakers(turns: &[SpeakerTurn], wanted: u32) -> Vec<SpeakerTurn> {
    let kept = kept_speakers(turns, wanted);
    let reassigned: Vec<SpeakerTurn> = turns
        .iter()
        .filter_map(|turn| {
            let speaker = if kept.contains(&turn.speaker) {
                Some(turn.speaker)
            } else {
                nearest_kept(turn, turns, &kept)
            };
            speaker.map(|speaker| SpeakerTurn { speaker, ..*turn })
        })
        .collect();
    let order = first_appearance(&reassigned);
    reassigned
        .iter()
        .map(|turn| {
            let rank = order
                .iter()
                .position(|speaker| *speaker == turn.speaker)
                .unwrap_or(0);
            SpeakerTurn {
                speaker: u32::try_from(rank + 1).unwrap_or(1),
                ..*turn
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn turn(start: f64, end: f64, speaker: u32) -> SpeakerTurn {
        SpeakerTurn {
            start,
            end,
            speaker,
        }
    }

    #[test]
    fn a_diarizer_mistake_merges_into_the_nearest_speaker_in_automatic_mode() {
        let turns = [
            turn(0.0, 100.0, 3),
            turn(100.0, 100.5, 5),
            turn(101.0, 200.0, 3),
        ];
        assert_eq!(
            limit_speakers(&turns, 0),
            vec![
                turn(0.0, 100.0, 1),
                turn(100.0, 100.5, 1),
                turn(101.0, 200.0, 1)
            ]
        );
    }

    #[test]
    fn real_speakers_stay_in_automatic_mode_and_are_numbered_by_first_voice() {
        let turns = [turn(0.0, 10.0, 7), turn(10.0, 20.0, 2), turn(20.0, 30.0, 7)];
        assert_eq!(
            limit_speakers(&turns, 0),
            vec![turn(0.0, 10.0, 1), turn(10.0, 20.0, 2), turn(20.0, 30.0, 1)]
        );
    }

    #[test]
    fn a_requested_count_keeps_the_speakers_who_talk_most() {
        let turns = [turn(0.0, 3.0, 1), turn(3.0, 30.0, 2), turn(30.0, 60.0, 3)];
        assert_eq!(
            limit_speakers(&turns, 2),
            vec![turn(0.0, 3.0, 1), turn(3.0, 30.0, 1), turn(30.0, 60.0, 2)]
        );
    }

    #[test]
    fn asking_for_more_speakers_than_detected_invents_nobody() {
        let turns = [turn(0.0, 3.0, 4), turn(3.0, 6.0, 9)];
        assert_eq!(
            limit_speakers(&turns, 5),
            vec![turn(0.0, 3.0, 1), turn(3.0, 6.0, 2)]
        );
    }

    #[test]
    fn no_turns_stay_no_turns() {
        assert_eq!(limit_speakers(&[], 0), Vec::<SpeakerTurn>::new());
    }
}
