import json
import subprocess
from pathlib import Path

import pytest

from replay import caption_voice_onset as voice
from replay.core import ReplayError
from replay.stages import file_digest

SPANS = [(1.0, 2.0), (3.0, 4.0)]


def test_the_tools_centiseconds_become_seconds() -> None:
    output = (
        "\nDetected 2 speech segments:\nSpeech segment 0: start = 0.00, end = 80.00\n"
        "Speech segment 1: start = 211.00, end = 349.50\n"
    )
    assert voice.parse_spans(output) == [(0.0, 0.8), (2.11, 3.495)]


def test_a_time_in_silence_moves_up_to_a_near_voice() -> None:
    assert voice.voiced_from(2.5, SPANS) == 3.0


def test_a_time_inside_a_voice_stays() -> None:
    assert voice.voiced_from(1.5, SPANS) == 1.5


def test_a_voice_further_than_the_reach_says_nothing() -> None:
    assert voice.voiced_from(2.3, SPANS) == 2.3


def test_a_time_after_the_last_voice_stays() -> None:
    assert voice.voiced_from(9.0, SPANS) == 9.0


def test_no_voice_at_all_changes_nothing() -> None:
    assert voice.voiced_from(2.5, []) == 2.5


def test_a_moved_token_keeps_its_anchor_and_never_ends_before_it_starts() -> None:
    tokens = [
        {"text": "あ", "start": 2.5, "end": 2.6, "anchor": 2.5},
        {"text": "い", "start": 2.6, "end": 3.2, "anchor": 2.5},
        {"text": "う", "start": 3.2, "end": 3.3, "anchor": 3.2},
    ]
    assert voice.moved(tokens, SPANS) == [
        {"text": "あ", "start": 3.0, "end": 3.0, "anchor": 2.5},
        {"text": "い", "start": 3.0, "end": 3.2, "anchor": 2.5},
        {"text": "う", "start": 3.2, "end": 3.3, "anchor": 3.2},
    ]


def fake_tool(path: Path, body: str) -> Path:
    path.write_text(f"#!/bin/bash\n{body}\n", encoding="utf-8")
    path.chmod(0o700)
    return path


def test_the_command_writes_moved_tokens_and_what_moved(tmp_path: Path) -> None:
    onsets = tmp_path / "onsets.json"
    onsets.write_text(
        json.dumps(
            {"version": 1, "tokens": [{"text": "あ", "start": 0.5, "end": 0.6, "anchor": 0.5}]}
        ),
        encoding="utf-8",
    )
    ffmpeg = fake_tool(tmp_path / "ffmpeg", 'touch "${@: -1}"')
    binary = fake_tool(tmp_path / "vad", "echo 'Speech segment 0: start = 80.00, end = 200.00'")
    model = tmp_path / "model.bin"
    model.write_bytes(b"model")
    output = tmp_path / "out.json"
    code = voice.main([
        str(onsets), str(tmp_path / "a.wav"), str(ffmpeg),
        str(binary), file_digest(binary),
        str(model), file_digest(model),
        str(tmp_path / "work"), str(output),
    ])  # fmt: skip
    written = json.loads(output.read_text(encoding="utf-8"))
    assert code == 0
    assert written["tokens"] == [{"text": "あ", "start": 0.8, "end": 0.8, "anchor": 0.5}]
    assert written["voiceOnset"]["movedTokens"] == 1
    assert list((tmp_path / "work").iterdir()) == []


def test_a_failing_detector_stops_and_removes_its_audio(tmp_path: Path) -> None:
    ffmpeg = fake_tool(tmp_path / "ffmpeg", 'touch "${@: -1}"')
    binary = fake_tool(tmp_path / "vad", "exit 3")
    with pytest.raises(subprocess.CalledProcessError):
        voice.detect(tmp_path / "a.wav", str(ffmpeg), binary, tmp_path / "m", tmp_path / "work")
    assert list((tmp_path / "work").iterdir()) == []


def test_a_document_without_tokens_is_refused(tmp_path: Path) -> None:
    onsets = tmp_path / "onsets.json"
    onsets.write_text("{}", encoding="utf-8")
    with pytest.raises(ReplayError):
        voice.write(onsets, "", "0", tmp_path / "out.json")


def test_a_file_other_than_the_pinned_one_is_refused(tmp_path: Path) -> None:
    path = tmp_path / "tool"
    path.write_bytes(b"x")
    with pytest.raises(ReplayError):
        voice.verified(path, "0" * 64)
