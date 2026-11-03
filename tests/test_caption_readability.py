"""A recording's captions are readable only when they show their whole text long enough."""

import json
from fractions import Fraction
from pathlib import Path

import pytest

from replay import caption_readability as readability
from replay.core import ReplayError


def part(directory: Path, name: str, titles: list[dict[str, object]]) -> None:
    directory.mkdir(exist_ok=True)
    (directory / f"{name}.reveal.json").write_text(
        json.dumps({"offsetFrame": 0, "titles": titles}), encoding="utf-8"
    )


def test_titles_replaced_as_their_last_grapheme_appears_are_refused(tmp_path: Path) -> None:
    part(tmp_path, "w000-part-0000", [
        {"text": "あい", "startFrame": 0, "endFrame": 12, "graphemeFrames": [0, 9]},
        {"text": "うえ", "startFrame": 12, "endFrame": 20, "graphemeFrames": [12, 18]},
    ])  # fmt: skip
    with pytest.raises(ReplayError, match="too briefly"):
        readability.check(tmp_path, 30)
    assert readability.main([str(tmp_path)]) == 1


def test_titles_that_stay_after_their_text_is_complete_pass(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    part(tmp_path, "w000-part-0000", [
        {"text": "あい", "startFrame": 0, "endFrame": 40, "graphemeFrames": [0, 9]},
        {"text": "うえ", "startFrame": 40, "endFrame": 80, "graphemeFrames": [40, 50]},
    ])  # fmt: skip
    part(tmp_path, "w001-part-0000", [
        {"text": "お", "startFrame": 0, "endFrame": 30, "graphemeFrames": [0]},
    ])  # fmt: skip
    assert readability.check(tmp_path, 30) == {
        "titles": 3, "medianSeconds": 1.0, "flashShare": 0.0, "readable": True,
    }  # fmt: skip
    assert readability.main([str(tmp_path)]) == 0
    assert capsys.readouterr().out == (
        '{"titles": 3, "medianSeconds": 1.0, "flashShare": 0.0, "readable": true}\n'
    )


def test_a_missing_or_malformed_record_is_refused(tmp_path: Path) -> None:
    with pytest.raises(ReplayError, match="No caption reveal records"):
        readability.readable(tmp_path, 30)
    (tmp_path / "w000-part-0000.reveal.json").write_text("{}", encoding="utf-8")
    with pytest.raises(ReplayError, match="has no titles"):
        readability.readable(tmp_path, 30)
    part(tmp_path, "w000-part-0000", [{"text": "あ", "endFrame": 3, "graphemeFrames": []}])
    with pytest.raises(ReplayError, match="without its frames"):
        readability.readable(tmp_path, 30)


def test_last_words_carried_by_the_next_title_count_its_length(tmp_path: Path) -> None:
    part(tmp_path, "w000-part-0000", [
        # Ends on frame 12 with "い" at 9: the next title, two frames on, leads with "い".
        {"text": "あ\nい", "startFrame": 0, "endFrame": 12, "graphemeFrames": [0, 0, 9]},
        {"text": "い\nうえ", "startFrame": 14, "endFrame": 44, "graphemeFrames": [14, 14, 20, 30]},
        # Three frames on, or leading without the last grapheme: no carry.
        {"text": "えお", "startFrame": 47, "endFrame": 50, "graphemeFrames": [47, 48]},
        {"text": "かき", "startFrame": 50, "endFrame": 53, "graphemeFrames": [50, 51]},
        {"text": "かく", "startFrame": 53, "endFrame": 59, "graphemeFrames": [53, 54]},
    ])  # fmt: skip
    assert readability.readable(tmp_path, 30) == [
        Fraction(33, 30), Fraction(14, 30), Fraction(2, 30), Fraction(2, 30), Fraction(5, 30),
    ]  # fmt: skip
    assert readability.carries("あい", "いう") is True
    assert readability.carries("あい", "あう") is False


def test_a_title_without_its_text_or_start_is_refused(tmp_path: Path) -> None:
    part(tmp_path, "w000-part-0000", [{"endFrame": 3, "startFrame": 0, "graphemeFrames": [0]}])
    with pytest.raises(ReplayError, match="without its frames"):
        readability.readable(tmp_path, 30)
