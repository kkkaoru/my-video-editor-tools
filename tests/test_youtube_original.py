import json
import subprocess
import sys
from pathlib import Path

import pytest

from replay.core import ReplayError
from replay.youtube_original import audio_formats, download_verified, expected_duration, main

URL = "https://www.youtube.com/watch?v=abcdefghijk"
STEM = "2024-01-01_09-00-00_JST_youtube-abcdefghijk"
CHANNEL = {"channel_id": "UCxxxxxxxxxxxxxxxxxxxxxx", "channel": "かしすみるく", "uploader_id": "@c"}
FORMATS = [
    {"format_id": "139", "vcodec": "none", "acodec": "mp4a.40.5"},
    {"format_id": "140", "vcodec": "none", "acodec": "mp4a.40.2"},
    {"format_id": "399", "vcodec": "av01", "acodec": "none"},
]


def _metadata(status: str) -> str:
    return json.dumps(
        {
            "id": "abcdefghijk",
            "release_timestamp": 1704067200,
            "timestamp": 1,
            "live_status": status,
            "duration": 5553,
            "formats": FORMATS,
            **CHANNEL,
        }
    )


def _fake(
    monkeypatch: pytest.MonkeyPatch,
    *,
    status: str,
    probe: str,
    refused: str = "",
    progress: int = 1,
    recovers: bool = False,
    extra: str | None = None,
) -> list[list[str]]:
    calls: list[list[str]] = []

    def run(args: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
        calls.append(args)
        if "--dump-single-json" in args:
            return subprocess.CompletedProcess(args, 0, _metadata(status))
        if args[0] == "/ffmpeg/ffprobe":
            return subprocess.CompletedProcess(args, 0, probe)
        target = Path(args[-2].replace("%(ext)s", "mp4"))
        part = target.parent / f"{STEM}.f{refused}.m4a.part"
        if refused and args[args.index("-f") + 1] == f"bv*+{refused}":
            # Each refused attempt resumes; it adds bytes until the stated number of attempts.
            if len(part.read_bytes() if part.exists() else b"") < progress:
                part.write_bytes(part.read_bytes() + b"p" if part.exists() else b"p")
                raise subprocess.CalledProcessError(1, args)
            if not recovers:
                raise subprocess.CalledProcessError(1, args)
            part.unlink()
        target.write_bytes(b"media")
        if extra is not None:
            (target.parent / extra).write_bytes(b"part")
        return subprocess.CompletedProcess(args, 0, "")

    monkeypatch.setattr(subprocess, "run", run)
    return calls


def _probe(video: str, audio: str) -> str:
    return json.dumps(
        {
            "streams": [
                {"codec_type": "video", "duration": video},
                {"codec_type": "audio", "duration": audio},
            ]
        }
    )


def test_post_live_is_kept_provisionally_with_the_audio_that_downloads(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    calls = _fake(monkeypatch, status="post_live", probe=_probe("5525.1", "5525.0"), refused="140")
    directory = download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg"))
    assert directory == tmp_path / STEM
    assert sorted(p.name for p in directory.iterdir()) == [
        "2024-01-01_09-00-00_JST_youtube-abcdefghijk.mp4",
        "channel.json",
        "publication.json",
    ]
    assert json.loads((directory / "publication.json").read_text(encoding="utf-8")) == {
        "id": "abcdefghijk",
        "release_timestamp": 1704067200,
        "timestamp": 1,
        "live_status": "post_live",
        "duration": 5553.0,
        "provisional": True,
        "streams": {"video": 5525.1, "audio": 5525.0},
        "audio_format": "139",
    }
    assert json.loads((directory / "channel.json").read_text(encoding="utf-8")) == CHANNEL
    assert calls[1][calls[1].index("-f") + 1] == "bv*+140"
    assert calls[2][calls[2].index("-f") + 1] == "bv*+140"
    assert calls[3][calls[3].index("-f") + 1] == "bv*+139"
    assert "--abort-on-unavailable-fragments" in calls[3]
    assert calls[3][calls[3].index("--concurrent-fragments") + 1] == "8"
    assert calls[3][-2] == str(tmp_path / f".{STEM}.post_live.partial" / f"{STEM}.%(ext)s")
    assert calls[4][-1] == str(tmp_path / f".{STEM}.post_live.partial" / f"{STEM}.mp4")
    assert sorted(p.name for p in tmp_path.iterdir()) == [
        ".youtube-abcdefghijk.lock",
        "2024-01-01_09-00-00_JST_youtube-abcdefghijk",
    ]
    with pytest.raises(ReplayError, match="provisional original waits"):
        download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg"))


def test_a_format_is_resumed_while_attempts_make_progress(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    calls = _fake(
        monkeypatch,
        status="post_live",
        probe=_probe("5525.1", "5525.0"),
        refused="140",
        progress=3,
        recovers=True,
    )
    download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg"))
    assert [call[call.index("-f") + 1] for call in calls[1:5]] == [
        "bv*+140",
        "bv*+140",
        "bv*+140",
        "bv*+140",
    ]
    assert (
        json.loads((tmp_path / STEM / "publication.json").read_text(encoding="utf-8"))[
            "audio_format"
        ]
        == "140"
    )


def test_the_processed_archive_replaces_the_provisional_original(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    original = tmp_path / STEM
    original.mkdir()
    (original / f"{STEM}.mp4").write_bytes(b"provisional")
    (original / "publication.json").write_text('{"provisional": true}', encoding="utf-8")
    calls = _fake(monkeypatch, status="was_live", probe=_probe("5553.033", "5552.600"))
    assert download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg")) == original
    assert calls[1][calls[1].index("-f") + 1] == "bv*+ba/b"
    assert (original / f"{STEM}.mp4").read_bytes() == b"media"
    assert json.loads((original / "publication.json").read_text(encoding="utf-8")) == {
        "id": "abcdefghijk",
        "release_timestamp": 1704067200,
        "timestamp": 1,
        "live_status": "was_live",
        "duration": 5553.0,
        "provisional": False,
        "streams": {"video": 5553.033, "audio": 5552.6},
    }
    assert sorted(p.name for p in tmp_path.iterdir()) == [
        ".youtube-abcdefghijk.lock",
        "2024-01-01_09-00-00_JST_youtube-abcdefghijk",
    ]
    with pytest.raises(ReplayError, match="already exists"):
        download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg"))


def test_a_provisional_original_under_another_name_is_not_replaced(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    other = tmp_path / "2024-01-02_09-00-00_JST_youtube-abcdefghijk"
    other.mkdir()
    (other / "publication.json").write_text('{"provisional": true}', encoding="utf-8")
    _fake(monkeypatch, status="was_live", probe=_probe("5553.0", "5553.0"))
    with pytest.raises(ReplayError, match="provisional original waits"):
        download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg"))


def test_a_short_processed_archive_stays_in_staging(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _fake(monkeypatch, status="was_live", probe=_probe("5553.0", "5540.0"))
    with pytest.raises(ReplayError, match="audio lasts 5540.000s, YouTube reports 5553s"):
        download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg"))
    assert sorted(p.name for p in tmp_path.iterdir()) == [
        ".2024-01-01_09-00-00_JST_youtube-abcdefghijk.was_live.partial",
        ".youtube-abcdefghijk.lock",
    ]


@pytest.mark.parametrize("video", ["0", "5560.0"])
def test_a_provisional_stream_must_be_present_and_not_longer(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, video: str
) -> None:
    _fake(monkeypatch, status="post_live", probe=_probe(video, "5500.0"))
    with pytest.raises(ReplayError, match="video lasts"):
        download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg"))


def test_no_audio_format_downloads(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    calls: list[list[str]] = []

    def run(args: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
        calls.append(args)
        if "--dump-single-json" in args:
            return subprocess.CompletedProcess(args, 0, _metadata("post_live"))
        raise subprocess.CalledProcessError(1, args)

    monkeypatch.setattr(subprocess, "run", run)
    with pytest.raises(ReplayError, match="No audio format"):
        download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg"))
    assert len(calls) == 3


def test_a_failed_attempt_is_resumed_per_status(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    (tmp_path / f".{STEM}.was_live.partial").mkdir()
    (tmp_path / f".{STEM}.post_live.partial").mkdir()
    _fake(monkeypatch, status="was_live", probe=_probe("5553.0", "5553.0"))
    assert download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg")) == (
        tmp_path / STEM
    )
    assert sorted(p.name for p in tmp_path.iterdir()) == [
        ".2024-01-01_09-00-00_JST_youtube-abcdefghijk.post_live.partial",
        ".youtube-abcdefghijk.lock",
        "2024-01-01_09-00-00_JST_youtube-abcdefghijk",
    ]


def test_leftover_files_block_publication(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    _fake(monkeypatch, status="was_live", probe=_probe("5553.0", "5553.0"), extra="x.f399.mp4")
    with pytest.raises(ReplayError, match="Unexpected files"):
        download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg"))
    assert not (tmp_path / STEM).exists()


@pytest.mark.parametrize(
    "probe,message",
    [
        (json.dumps({}), "no streams"),
        (json.dumps({"streams": [{"codec_type": "audio", "duration": "1"}]}), "one video"),
        (json.dumps({"streams": [{"codec_type": "video"}]}), "video stream has no duration"),
    ],
)
def test_bad_probe_is_rejected(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, probe: str, message: str
) -> None:
    _fake(monkeypatch, status="was_live", probe=probe)
    with pytest.raises(ReplayError, match=message):
        download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg"))


def test_upcoming_stream_downloads_nothing(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    calls = _fake(monkeypatch, status="is_upcoming", probe="")
    with pytest.raises(ReplayError, match="not a finished upload or archive: is_upcoming"):
        download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg"))
    assert len(calls) == 1
    assert sorted(p.name for p in tmp_path.iterdir()) == [".youtube-abcdefghijk.lock"]


@pytest.mark.parametrize("duration", [None, True, 0, float("inf")])
def test_duration_is_required(duration: object) -> None:
    with pytest.raises(ReplayError, match="no duration"):
        expected_duration({"live_status": "was_live", "duration": duration})


def test_audio_formats_are_best_first_and_required() -> None:
    assert audio_formats({"formats": FORMATS}) == ["140", "139"]
    with pytest.raises(ReplayError, match="no formats"):
        audio_formats({})


def test_existing_originals_stop_before_network(tmp_path: Path) -> None:
    with pytest.raises(ReplayError, match="canonical"):
        download_verified(
            url="https://www.youtube.com/live/abcdefghijk",
            root=tmp_path,
            ffmpeg_directory=Path("/ffmpeg"),
        )
    (tmp_path / STEM).mkdir()
    with pytest.raises(ReplayError, match="already exists"):
        download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg"))
    (tmp_path / STEM / "publication.json").write_text("[]", encoding="utf-8")
    with pytest.raises(ReplayError, match="already exists"):
        download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg"))


def test_main(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.setattr(sys, "argv", ["youtube_original"])
    with pytest.raises(SystemExit, match="Usage"):
        main()
    monkeypatch.setattr(sys, "argv", ["youtube_original", "bad", str(tmp_path), "/ffmpeg"])
    with pytest.raises(SystemExit, match="canonical"):
        main()
    _fake(monkeypatch, status="was_live", probe=_probe("5553.0", "5553.0"))
    monkeypatch.setattr(sys, "argv", ["youtube_original", URL, str(tmp_path), "/ffmpeg", "chrome"])
    main()
    assert capsys.readouterr().out == f"{tmp_path / STEM}\n"


WAS_LIVE_EVIDENCE = {
    "id": "abcdefghijk",
    "release_timestamp": 1704067200,
    "timestamp": 1,
    "live_status": "was_live",
    "duration": 5553.0,
    "provisional": False,
    "streams": {"video": 5553.0, "audio": 5553.0},
}


def test_an_unmounted_volume_root_is_refused_before_any_write(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    calls = _fake(monkeypatch, status="was_live", probe="")
    with pytest.raises(ReplayError, match="/Volumes/no-such-volume-x is not mounted"):
        download_verified(
            url=URL,
            root=Path("/Volumes/no-such-volume-x/movie/youtube"),
            ffmpeg_directory=Path("/ffmpeg"),
        )
    assert calls == []


def test_a_missing_root_is_refused(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    calls = _fake(monkeypatch, status="was_live", probe="")
    with pytest.raises(ReplayError, match="does not exist"):
        download_verified(url=URL, root=tmp_path / "gone", ffmpeg_directory=Path("/ffmpeg"))
    assert calls == []
    assert not (tmp_path / "gone").exists()


def test_a_staging_that_went_away_keeps_the_format_for_a_rerun(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    calls: list[list[str]] = []

    def run(args: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
        calls.append(args)
        if "--dump-single-json" in args:
            return subprocess.CompletedProcess(args, 0, _metadata("post_live"))
        (tmp_path / f".{STEM}.post_live.partial").rmdir()
        raise subprocess.CalledProcessError(1, args)

    monkeypatch.setattr(subprocess, "run", run)
    with pytest.raises(ReplayError, match="went away during the download"):
        download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg"))
    assert len(calls) == 2


def test_a_verified_staging_cut_before_its_rename_is_published_by_the_rerun(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    staging = tmp_path / f".{STEM}.was_live.partial"
    staging.mkdir()
    (staging / f"{STEM}.mp4").write_bytes(b"media")
    (staging / "publication.json").write_text(json.dumps(WAS_LIVE_EVIDENCE), encoding="utf-8")
    _fake(monkeypatch, status="was_live", probe=_probe("5553.0", "5553.0"))
    assert download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg")) == (
        tmp_path / STEM
    )
    assert sorted(p.name for p in tmp_path.iterdir()) == [
        ".youtube-abcdefghijk.lock",
        "2024-01-01_09-00-00_JST_youtube-abcdefghijk",
    ]
    assert sorted(p.name for p in (tmp_path / STEM).iterdir()) == [
        "2024-01-01_09-00-00_JST_youtube-abcdefghijk.mp4",
        "channel.json",
        "publication.json",
    ]


def test_different_staged_evidence_is_not_overwritten(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    staging = tmp_path / f".{STEM}.was_live.partial"
    staging.mkdir()
    (staging / f"{STEM}.mp4").write_bytes(b"media")
    (staging / "publication.json").write_text('{"provisional": true}', encoding="utf-8")
    _fake(monkeypatch, status="was_live", probe=_probe("5553.0", "5553.0"))
    with pytest.raises(ReplayError, match="records a different download"):
        download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg"))
    assert (staging / "publication.json").read_text(encoding="utf-8") == '{"provisional": true}'


def test_a_replacement_cut_between_its_renames_is_finished(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    replaced = tmp_path / f".{STEM}.provisional.replaced"
    replaced.mkdir()
    (replaced / f"{STEM}.mp4").write_bytes(b"provisional")
    (replaced / "publication.json").write_text('{"provisional": true}', encoding="utf-8")
    staging = tmp_path / f".{STEM}.was_live.partial"
    staging.mkdir()
    (staging / f"{STEM}.mp4").write_bytes(b"media")
    (staging / "publication.json").write_text(json.dumps(WAS_LIVE_EVIDENCE), encoding="utf-8")
    _fake(monkeypatch, status="was_live", probe=_probe("5553.0", "5553.0"))
    assert download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg")) == (
        tmp_path / STEM
    )
    assert (tmp_path / STEM / f"{STEM}.mp4").read_bytes() == b"media"
    assert sorted(p.name for p in tmp_path.iterdir()) == [
        ".youtube-abcdefghijk.lock",
        "2024-01-01_09-00-00_JST_youtube-abcdefghijk",
    ]


def test_a_replacement_cut_before_its_cleanup_is_cleaned(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    (tmp_path / f".{STEM}.provisional.replaced").mkdir()
    (tmp_path / STEM).mkdir()
    (tmp_path / STEM / "publication.json").write_text(
        json.dumps(WAS_LIVE_EVIDENCE), encoding="utf-8"
    )
    _fake(monkeypatch, status="was_live", probe="")
    with pytest.raises(ReplayError, match="already exists"):
        download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg"))
    assert sorted(p.name for p in tmp_path.iterdir()) == [
        ".youtube-abcdefghijk.lock",
        "2024-01-01_09-00-00_JST_youtube-abcdefghijk",
    ]


def test_a_verified_provisional_staging_keeps_its_recorded_audio_format(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    staging = tmp_path / f".{STEM}.post_live.partial"
    staging.mkdir()
    (staging / f"{STEM}.mp4").write_bytes(b"media")
    evidence = {
        "id": "abcdefghijk",
        "release_timestamp": 1704067200,
        "timestamp": 1,
        "live_status": "post_live",
        "duration": 5553.0,
        "provisional": True,
        "streams": {"video": 5525.1, "audio": 5525.0},
        "audio_format": "139",
    }
    (staging / "publication.json").write_text(json.dumps(evidence), encoding="utf-8")
    calls = _fake(monkeypatch, status="post_live", probe=_probe("5525.1", "5525.0"))
    assert download_verified(url=URL, root=tmp_path, ffmpeg_directory=Path("/ffmpeg")) == (
        tmp_path / STEM
    )
    assert [call[0] for call in calls[1:]] == ["/ffmpeg/ffprobe"]
    assert json.loads((tmp_path / STEM / "publication.json").read_text(encoding="utf-8")) == {
        "id": "abcdefghijk",
        "release_timestamp": 1704067200,
        "timestamp": 1,
        "live_status": "post_live",
        "duration": 5553.0,
        "provisional": True,
        "streams": {"video": 5525.1, "audio": 5525.0},
        "audio_format": "139",
    }
