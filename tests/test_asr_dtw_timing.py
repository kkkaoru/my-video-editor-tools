import json
import subprocess
import wave
from pathlib import Path

import pytest

from replay import asr_dtw_timing as timing
from replay.core import ReplayError
from replay.stages import file_digest


def token(text: str, ticks: int, start: int = 0, end: int = 0) -> dict[str, object]:
    return {"text": text, "t_dtw": ticks, "offsets": {"from": start, "to": end}}


def whisper(*segments: dict[str, object]) -> bytes:
    return json.dumps({"transcription": list(segments)}, ensure_ascii=False).encode("utf-8")


def segment(first: int, last: int, *tokens: dict[str, object]) -> dict[str, object]:
    return {"offsets": {"from": first, "to": last}, "tokens": list(tokens)}


def test_a_kanji_takes_two_beats_and_a_small_kana_none() -> None:
    assert timing.spoken_beats("今日") == 4
    assert timing.spoken_beats("きょう") == 2
    assert timing.spoken_beats("あ") == 1


def test_warped_time_inside_its_segment_is_kept() -> None:
    assert timing.warped_end(150, (1.0, 2.0), None) == 1.5


def test_warped_time_before_its_segment_comes_to_its_start() -> None:
    assert timing.warped_end(50, (2.0, 3.0), None) == 2.0


def test_warped_time_just_outside_its_coarse_segment_is_kept() -> None:
    assert timing.warped_end(50, (1.0, 2.0), None) == 0.5
    assert timing.warped_end(250, (1.0, 2.0), None) == 2.5


def test_warped_time_after_its_segment_follows_the_token_before() -> None:
    assert timing.warped_end(900, (1.0, 2.0), 1.5) == 1.75
    assert timing.warped_end(900, (1.0, 2.0), 1.9) == 2.0
    assert timing.warped_end(900, (1.0, 2.0), None) == 2.0


def test_control_tokens_are_left_out_and_the_warped_time_is_each_tokens_end() -> None:
    raw = whisper(segment(0, 2000, token("[_BEG_]", 0), token("あ", 100), token("い", 120)))
    assert timing.parse_tokens(raw) == [
        timing.TimedToken("あ", 0.8, 1.0, True),
        timing.TimedToken("い", 1.0, 1.2, True),
    ]


def test_a_character_split_across_tokens_is_merged() -> None:
    whole = "あ".encode()
    pieces = [whole[:1].decode("latin-1"), whole[1:].decode("latin-1")]
    raw = json.dumps(
        {"transcription": [segment(0, 2000, token(pieces[0], 100), token(pieces[1], 110))]}
    ).encode("utf-8")
    assert [item.text for item in timing.parse_tokens(raw)] == ["あ"]


def test_tokens_without_warped_times_keep_their_own_span() -> None:
    raw = whisper(segment(0, 2000, token("あ", -1, 100, 300)))
    assert timing.parse_tokens(raw) == [timing.TimedToken("あ", 0.1, 0.3, False)]


def test_json_without_a_transcription_is_refused() -> None:
    with pytest.raises(ReplayError):
        timing.parse_tokens(b"{}")


def test_a_segment_without_tokens_is_refused() -> None:
    with pytest.raises(ReplayError):
        timing.segment_tokens({"offsets": {"from": 0, "to": 1}, "tokens": None})


def test_a_token_after_a_pause_starts_at_the_local_rate_before_its_end() -> None:
    tokens = [
        timing.TimedToken("か", 1.0, 1.0, True),
        timing.TimedToken("き", 1.1, 1.1, True),
        timing.TimedToken("く", 1.2, 1.2, True),
        timing.TimedToken("けこ", 5.0, 5.0, True),
    ]
    assert timing.opened(tokens)[3] == timing.TimedToken("けこ", 4.8, 5.0, True)


def test_the_rate_defaults_to_the_slowest_when_nothing_is_near() -> None:
    assert timing.local_beat_seconds([(1.0, 1)], 0) == 0.2


def test_characters_get_times_across_their_token() -> None:
    assert timing.timed_characters([timing.TimedToken("あ い", 1.0, 2.0, True)]) == [
        ("あ", 1.0),
        ("い", 1.5),
    ]


def test_matching_finds_the_common_characters_in_order() -> None:
    assert timing.matched_characters("あいう", "あうえ") == [0, None, 1]


def test_matching_with_nothing_in_common_finds_nothing() -> None:
    assert timing.matched_characters("あい", "かき") == [None, None]


def test_matching_prefers_the_other_texts_skipped_character() -> None:
    assert timing.matched_characters("あう", "あいう") == [0, 2]


def test_time_between_anchors_is_shared() -> None:
    assert timing.time_at(1, [(0, 1.0), (2, 2.0)]) == 1.5


def test_time_on_an_anchor_is_its_own() -> None:
    assert timing.time_at(2, [(0, 1.0), (2, 2.0)]) == 2.0


def test_time_past_the_anchors_is_the_nearest_one() -> None:
    assert timing.time_at(5, [(0, 1.0), (2, 2.0)]) == 2.0
    assert timing.time_at(0, [(2, 2.0)]) == 2.0


def test_time_without_anchors_is_unknown() -> None:
    assert timing.time_at(0, []) is None


def test_labels_never_step_back_or_end_empty_and_lose_their_old_snap() -> None:
    labels = [
        {"text": "あ", "start": 9.0, "end": 9.1, "provenance": "p:0", "onsetSnap": {}},
        {"text": "い", "start": 9.1, "end": 9.2, "provenance": "p:1"},
        {"text": "う", "start": 9.2, "end": 9.3, "provenance": "p:2"},
    ]
    assert timing.closed(labels, [2.0, 1.0, 5.0], 60.0) == [
        {"text": "あ", "start": 2.0, "end": 2.01, "provenance": "p:0"},
        {"text": "い", "start": 2.01, "end": 2.26, "provenance": "p:1"},
        {"text": "う", "start": 5.0, "end": 5.25, "provenance": "p:2"},
    ]


def test_retimed_labels_keep_their_text_and_take_the_matched_times() -> None:
    labels = [
        {"text": "あい", "start": 0.0, "end": 1.0, "provenance": "p:0"},
        {"text": "う", "start": 1.0, "end": 2.0, "provenance": "p:1"},
    ]
    tokens = [timing.TimedToken("あい", 3.0, 3.5, True), timing.TimedToken("う", 4.0, 4.2, True)]
    assert timing.retimed(labels, tokens, 60.0) == (
        [
            {"text": "あい", "start": 3.0, "end": 3.5, "provenance": "p:0"},
            {"text": "う", "start": 4.0, "end": 4.25, "provenance": "p:1"},
        ],
        3,
        3,
    )


def test_labels_stay_inside_the_audio() -> None:
    labels = [
        {"text": "ああ", "start": 0.0, "end": 1.0, "provenance": "p:0"},
        {"text": "い", "start": 1.0, "end": 2.0, "provenance": "p:1"},
    ]
    assert timing.closed(labels, [9.0, 10.5], 10.2) == [
        {"text": "ああ", "start": 9.0, "end": 9.5, "provenance": "p:0"},
        {"text": "い", "start": 10.19, "end": 10.2, "provenance": "p:1"},
    ]


def test_a_recognition_without_text_is_refused() -> None:
    with pytest.raises(ReplayError):
        timing.retimed([{"text": "あ"}], [], 60.0)


def test_labels_matching_nothing_are_refused() -> None:
    with pytest.raises(ReplayError):
        timing.retimed([{"text": "あ"}], [timing.TimedToken("か", 1.0, 1.1, True)], 60.0)


def fake_whisper(path: Path) -> Path:
    body = whisper(segment(0, 2000, token("あ", 100))).decode("utf-8")
    path.write_text(
        '#!/bin/bash\nwhile [ "$#" -gt 0 ]; do\n  if [ "$1" = -of ]; then out="$2"; fi\n  shift\n'
        f"done\ncat > \"$out.json\" <<'JSON'\n{body}\nJSON\n",
        encoding="utf-8",
    )
    path.chmod(0o700)
    return path


def test_the_command_writes_timed_labels_beside_the_originals(tmp_path: Path) -> None:
    labels = tmp_path / "labels.json"
    labels.write_text(
        json.dumps({"version": 1, "labels": [{"text": "あ", "start": 0.0, "end": 0.5}]}),
        encoding="utf-8",
    )
    binary, model = fake_whisper(tmp_path / "whisper"), tmp_path / "model.bin"
    model.write_bytes(b"model")
    output = tmp_path / "out.json"
    with wave.open(str(tmp_path / "a.wav"), "wb") as stream:
        stream.setnchannels(1)
        stream.setsampwidth(2)
        stream.setframerate(16000)
        stream.writeframes(b"\0\0" * 32000)
    code = timing.main([
        str(labels), str(tmp_path / "a.wav"),
        str(binary), file_digest(binary),
        str(model), file_digest(model),
        str(tmp_path / "work"), str(output),
    ])  # fmt: skip
    written = json.loads(output.read_text(encoding="utf-8"))
    assert code == 0
    assert written["labels"] == [{"text": "あ", "start": 0.8, "end": 1.05}]
    assert written["tokenTiming"]["matchedCharacters"] == 1


def test_a_finished_recognition_is_reused(tmp_path: Path) -> None:
    work = tmp_path / "work"
    work.mkdir()
    (work / "dtw.json").write_bytes(b"kept")
    binary = tmp_path / "absent"
    assert timing.recognize(tmp_path / "a.wav", binary, tmp_path / "m", work) == b"kept"


def test_a_failing_recognition_stops(tmp_path: Path) -> None:
    binary = tmp_path / "whisper"
    binary.write_text("#!/bin/bash\nexit 3\n", encoding="utf-8")
    binary.chmod(0o700)
    with pytest.raises(subprocess.CalledProcessError):
        timing.recognize(tmp_path / "a.wav", binary, tmp_path / "m", tmp_path / "work")


def test_a_file_other_than_the_pinned_one_is_refused(tmp_path: Path) -> None:
    path = tmp_path / "tool"
    path.write_bytes(b"x")
    with pytest.raises(ReplayError):
        timing.verified(path, "0" * 64)


def test_labels_that_are_no_list_are_refused(tmp_path: Path) -> None:
    labels = tmp_path / "labels.json"
    labels.write_text('{"labels": 1}', encoding="utf-8")
    with pytest.raises(ReplayError):
        timing.write(labels, whisper(), 60.0, "0", tmp_path / "out.json")
