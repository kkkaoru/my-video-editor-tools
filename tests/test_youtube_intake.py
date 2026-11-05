"""YouTube intake: only a final original is bound and queued; one waiting video blocks no other."""

import json
import subprocess
from pathlib import Path
from types import SimpleNamespace

import pytest

from replay import edit_completion, video_config, youtube_intake, youtube_original
from replay.core import ReplayError

URL = "https://www.youtube.com/watch?v=abcdefghijk"
STEM = "d_JST_youtube-abcdefghijk"


def original(root: Path, *, provisional: bool) -> Path:
    directory = root / STEM
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "publication.json").write_text(
        json.dumps({"provisional": provisional}), encoding="utf-8"
    )
    movie = directory / f"{STEM}.mp4"
    movie.write_text("movie", encoding="utf-8")
    return movie


def test_a_provisional_original_is_replaced_before_it_counts(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    states = iter([True, False])
    waits: list[float] = []

    def download(**_kwargs: object) -> Path:
        return original(tmp_path, provisional=next(states))

    monkeypatch.setattr(youtube_original, "download_verified", download)
    found = youtube_intake.wait_for_original(
        URL, root=tmp_path, ffmpeg_directory=tmp_path, sleep=waits.append
    )
    assert found.name == "d_JST_youtube-abcdefghijk.mp4"
    assert waits == [900.0]


def test_a_final_original_downloads_nothing_and_failures_retry_then_stop(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    def refuse(**_kwargs: object) -> Path:
        raise ReplayError("A provisional original waits")

    monkeypatch.setattr(youtube_original, "download_verified", refuse)
    monkeypatch.setattr(youtube_intake, "ATTEMPTS", 2)
    empty = tmp_path / "empty"
    empty.mkdir()
    with pytest.raises(ReplayError, match="No final original"):
        youtube_intake.wait_for_original(
            URL, root=empty, ffmpeg_directory=tmp_path, sleep=lambda _s: None
        )
    original(tmp_path, provisional=False)
    found = youtube_intake.wait_for_original(
        URL, root=tmp_path, ffmpeg_directory=tmp_path, sleep=lambda _s: None
    )
    assert found.name == "d_JST_youtube-abcdefghijk.mp4"


def test_a_directory_without_its_movie_is_not_an_original(tmp_path: Path) -> None:
    original(tmp_path, provisional=False).unlink()
    assert youtube_intake.final_original(tmp_path, "abcdefghijk") is None


def test_a_provisional_copy_counts_only_when_accepted(tmp_path: Path) -> None:
    original(tmp_path, provisional=True)
    assert youtube_intake.final_original(tmp_path, "abcdefghijk") is None
    found = youtube_intake.final_original(tmp_path, "abcdefghijk", accept_provisional=True)
    assert found == tmp_path / "d_JST_youtube-abcdefghijk/d_JST_youtube-abcdefghijk.mp4"


def test_intake_binds_once_and_queues_and_main_reports_failures(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    original(tmp_path / "root", provisional=False)
    queue = tmp_path / "queue.json"
    queue.write_text(
        json.dumps({"version": 1, "slots": 2, "paused": False, "recordings": []}),
        encoding="utf-8",
    )
    checkout = tmp_path / "checkout"
    bound: list[str] = []
    monkeypatch.setattr(youtube_intake, "measured_config", lambda _original: {"version": 1})

    def bind(root: Path, *, source: Path, **_kwargs: object) -> Path:
        bound.append(source.name)
        (root / "edit-config.json").write_text("{}", encoding="utf-8")
        return root / "edit-config.json"

    monkeypatch.setattr(edit_completion, "create_youtube_config", bind)
    first = youtube_intake.intake(
        URL, root=tmp_path / "root", ffmpeg_directory=tmp_path, queue=queue, checkout=checkout
    )
    again = youtube_intake.intake(
        URL, root=tmp_path / "root", ffmpeg_directory=tmp_path, queue=queue, checkout=checkout
    )
    assert (first, again) == ("youtube-abcdefghijk", "youtube-abcdefghijk")
    assert bound == ["d_JST_youtube-abcdefghijk.mp4"]
    assert json.loads(queue.read_text(encoding="utf-8"))["recordings"] == ["youtube-abcdefghijk"]

    monkeypatch.chdir(checkout)
    arguments = ["--queue", str(queue), "--root", str(tmp_path / "root"),
                 "--ffmpeg-directory", str(tmp_path)]  # fmt: skip
    assert youtube_intake.main([*arguments, URL]) == 0

    def broken(_url: str, **_kwargs: object) -> str:
        raise ReplayError("broken")

    monkeypatch.setattr(youtube_intake, "intake", broken)
    assert youtube_intake.main([*arguments, URL]) == 1
    assert capsys.readouterr().err == "https://www.youtube.com/watch?v=abcdefghijk: broken\n"


@pytest.mark.parametrize(
    ("width", "height", "orientation"),
    [
        (1080, 1920, "source"),
        (1920, 1080, {"aspect": [9, 16], "follow": "character"}),
    ],
)
def test_settings_follow_the_measured_frame(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    width: int,
    height: int,
    orientation: object,
) -> None:
    movie = original(tmp_path, provisional=False)
    (movie.parent / "channel.json").write_text(json.dumps({"channel": "label"}), encoding="utf-8")
    output = json.dumps({"streams": [{"width": width, "height": height}]})
    monkeypatch.setattr(subprocess, "run", lambda *_a, **_k: SimpleNamespace(stdout=output))
    settings = youtube_intake.measured_config(movie)
    assert settings == {
        "version": 1,
        "orientation": orientation,
        "captions": {"layout": {
            "canvas": [1080, 1920], "align": "centre", "originX": 0,
            "regionTop": 1300, "regionBottom": 1800, "regionRight": 1080,
        }},
        "cut": {"nonSpeech": True},
        "cleanup": {"sourceCaptions": True, "commentBoxes": False},
        "channelLabel": "label",
    }  # fmt: skip
    video_config.validated(settings)  # raises when the settings are not valid
