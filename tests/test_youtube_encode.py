"""YouTube encodes: one Compressor setting, each orientation at its own size, in order."""

import json
import subprocess
import sys
from collections.abc import Iterator, Sequence
from contextlib import contextmanager
from pathlib import Path

import pytest

from replay import gpu_lease
from replay import youtube_encode as youtube
from replay.compressor_batch import COMPRESSOR
from replay.core import ReplayError

FFMPEG = "ffmpeg"
FFPROBE = "ffprobe"


def movie(path: Path, size: str) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        [FFMPEG, "-v", "error", "-f", "lavfi", "-i", f"color=size={size}:d=0.2", "-c:v",
         "libx264", str(path)],
        check=True,
    )  # fmt: skip
    return path


def test_orientation_follows_the_delivery_size(tmp_path: Path) -> None:
    assert youtube.orientation(movie(tmp_path / "v.mp4", "1080x1920"), FFPROBE) == "vertical"
    assert youtube.orientation(movie(tmp_path / "h.mp4", "1920x1080"), FFPROBE) == "landscape"
    assert youtube.orientation(movie(tmp_path / "v7.mp4", "720x1280"), FFPROBE) == "vertical"
    assert youtube.orientation(movie(tmp_path / "h7.mp4", "1280x720"), FFPROBE) == "landscape"
    with pytest.raises(ReplayError, match="not a 1080p or 720p frame"):
        youtube.orientation(movie(tmp_path / "s.mp4", "640x360"), FFPROBE)
    assert youtube.allowed((720, 1280)) is True
    assert youtube.allowed((640, 360)) is False


def test_the_output_sits_beside_the_delivery_folder() -> None:
    delivery = Path("/l/rec/revisions/p/delivery/S_JST_rec-review.mp4")
    assert youtube.output_of(delivery) == Path(
        "/l/rec/revisions/p/youtube/S_JST_rec-review-youtube.mov"
    )
    assert youtube.job_name(delivery) == "S_JST_rec"


def test_pending_movies_are_encoded_in_order_and_checked(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    first = movie(tmp_path / "a/r/delivery/A_JST_a-review.mp4", "1080x1920")
    second = movie(tmp_path / "b/r/delivery/B_JST_b-review.mp4", "1920x1080")
    movie(youtube.output_of(first), "1080x1920")
    calls: list[list[str]] = []
    real_run = subprocess.run

    def run(command: Sequence[str], **options: object) -> object:
        if "replay.compressor_batch" not in command:
            return real_run(command, capture_output=True, text=True, check=True)
        calls.append(list(command))
        movie(youtube.output_of(second), "1920x1080")
        return None

    monkeypatch.setattr(subprocess, "run", run)
    state = tmp_path / "state"
    assert youtube.main([str(first), str(second), "--state", str(state)]) == 0
    assert capsys.readouterr().out == f"{youtube.output_of(first)}\n{youtube.output_of(second)}\n"
    assert json.loads((state / "jobs.json").read_text(encoding="utf-8")) == {
        "jobs": [
            {
                "name": "B_JST_b",
                "source": str(second),
                "preset": str(youtube.SETTINGS),
                "output": str(youtube.output_of(second)),
            }
        ]
    }
    assert calls == [
        [sys.executable, "-B", "-m", "replay.compressor_batch", str(state / "jobs.json"),
         "--state", str(state / "state"), "--batch-name", "YouTube", "--in-flight", "1",
         "--executable", str(COMPRESSOR)],
    ]  # fmt: skip


def test_an_output_of_the_wrong_size_is_refused(tmp_path: Path) -> None:
    source = movie(tmp_path / "r/delivery/A_JST_a-review.mp4", "1080x1920")
    movie(youtube.output_of(source), "1920x1080")
    with pytest.raises(ReplayError, match="is not 1080x1920"):
        youtube.encode([source], tmp_path / "s", python="py", ffprobe=FFPROBE)


def test_a_lease_is_held_for_each_movie(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    source = movie(tmp_path / "r/delivery/A_JST_a-review.mp4", "1080x1920")
    movie(youtube.output_of(source), "1080x1920")
    held: list[tuple[Path, str, str]] = []

    @contextmanager
    def lease(directory: Path, replay_id: str, *, step: str) -> Iterator[None]:
        held.append((directory, replay_id, step))
        yield

    monkeypatch.setattr(gpu_lease, "lease", lease)
    leases = tmp_path / "leases"
    assert youtube.main([str(source), "--state", str(tmp_path / "s"), "--lease", str(leases)]) == 0
    assert held == [(leases, "A_JST_a", "youtube-encode")]
    assert capsys.readouterr().out == f"{youtube.output_of(source)}\n"


def test_a_delivery_replaced_by_its_master_is_skipped(tmp_path: Path) -> None:
    gone = tmp_path / "r/delivery/A_JST_a-review.mp4"
    assert youtube.encode([gone], tmp_path / "s", python="py", ffprobe=FFPROBE) == []
