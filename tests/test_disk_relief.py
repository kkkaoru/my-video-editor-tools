"""Disk relief removes old regenerable files first and keeps what it must."""

import json
import os
import subprocess
from collections.abc import Sequence
from pathlib import Path

import pytest

from replay import delivery_offload, disk_budget, r2_worker
from replay import disk_relief as relief
from replay.core import ReplayError

OLD = 1000.0
NOW = 1000.0 + 30 * 86400


def media(path: Path, size: int = relief.MIN_BYTES, modified: float = OLD) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("wb") as output:
        output.truncate(size)
    os.utime(path, (modified, modified))
    return path


def keep(*recordings: str, inodes: tuple[Path, ...] = ()) -> relief.Keep:
    found = frozenset(item for item in map(relief.inode, inodes) if item is not None)
    return relief.Keep(frozenset(recordings), found, NOW - relief.KEEP_DAYS * 86400)


class Room:
    def __init__(self, free: int) -> None:
        self.free = free


def test_only_old_large_media_and_old_build_trees_are_candidates(tmp_path: Path) -> None:
    media(tmp_path / "a/old.MP4")
    media(tmp_path / "a/new.mp4", modified=NOW)
    media(tmp_path / "a/small.mov", size=10)
    media(tmp_path / "a/settings.json")
    media(tmp_path / "replay-1/running.mp4")
    catalogued = media(tmp_path / "a/delivered.mp4")
    (tmp_path / "a/link.mp4").symlink_to(catalogued)
    tree = tmp_path / "a/.build"
    media(tree / "release/tool", size=5)
    os.utime(tree, (OLD, OLD))
    fresh = tmp_path / "b/.build"
    media(fresh / "tool", size=5)
    os.utime(fresh, (NOW, NOW))
    found = {item.path: item.bytes for item in relief.regenerable(
        tmp_path, keep("replay-1", inodes=(catalogued,)))}  # fmt: skip
    assert found == {tmp_path / "a/old.MP4": 1048576, tmp_path / "a/.build": 5}


def test_a_hold_covers_derived_folders_named_after_the_recording(tmp_path: Path) -> None:
    media(tmp_path / "clean-replay-1-p1/chunk.mp4")
    assert list(relief.regenerable(tmp_path, keep("replay-1"))) == []


def test_clearing_goes_oldest_first_stops_at_the_target_and_a_dry_run_removes_nothing(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    first, second = media(tmp_path / "a.mp4"), media(tmp_path / "b.mp4", modified=OLD + 1)
    tree = tmp_path / "t/.build"
    media(tree / "tool")
    candidates = [
        relief.Candidate(second, 10, OLD + 1),
        relief.Candidate(first, 10, OLD),
        relief.Candidate(tree, 10, OLD + 2),
    ]
    log = tmp_path / "log.jsonl"
    monkeypatch.setattr(disk_budget, "measure", lambda _volume: Room(0))

    def clear(items: Sequence[relief.Candidate], *, apply: bool) -> int:
        return relief.clear_oldest(items, volume=tmp_path, target=15, log=log, apply=apply)

    assert clear(candidates, apply=False) == 20
    assert first.exists() and second.exists() and not log.exists()
    frees = iter([0, 0, 100])
    monkeypatch.setattr(disk_budget, "measure", lambda _volume: Room(next(frees)))
    assert clear(candidates, apply=True) == 20
    assert not first.exists() and not second.exists() and (tree / "tool").exists()
    logged = [json.loads(line)["path"] for line in log.read_text(encoding="utf-8").splitlines()]
    assert logged == [str(tmp_path / "a.mp4"), str(tmp_path / "b.mp4")]
    monkeypatch.setattr(disk_budget, "measure", lambda _volume: Room(0))
    gone = relief.Candidate(tmp_path / "missing.mp4", 10, OLD)
    assert clear([gone, relief.Candidate(tree, 7, OLD)], apply=True) == 7
    assert not tree.exists()


def test_unreferenced_work_excludes_what_code_or_configuration_names(tmp_path: Path) -> None:
    work = tmp_path / "edits/work"
    (work / "named-by-code").mkdir(parents=True)
    (work / "named-by-config").mkdir()
    (work / "recordings").mkdir()
    (work / "pytest-default").mkdir()
    (work / "orphan").mkdir()
    source = tmp_path / "src/module.py"
    source.parent.mkdir()
    source.write_text('ROOT = "edits/work/named-by-code/x"\n', encoding="utf-8")
    config = tmp_path / "defaults.json"
    config.write_text('{"a": "/r/edits/work/named-by-config/tool"}', encoding="utf-8")
    named = relief.referenced_work(tmp_path, [config, tmp_path / "missing.json"])
    assert relief.unreferenced_work(work, named) == [work / "orphan"]


def test_catalog_and_edit_configs_name_what_tier_one_protects(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    catalog = tmp_path / "catalog.json"
    rows = [
        {"broadcastStartJST": "2026-10-02", "localPath": "/b.mp4"},
        {"broadcastStartJST": "2026-10-01", "localPath": "/a.mp4",
         "original": {"localPath": "/a-original.mp4"}},
    ]  # fmt: skip
    catalog.write_text(json.dumps({"videos": rows}), encoding="utf-8")
    assert list(relief.catalogued_paths(relief.videos(catalog))) == [
        Path("/a.mp4"), Path("/a-original.mp4"), Path("/b.mp4"),
    ]  # fmt: skip
    config = tmp_path / "edits/replay-1/edit-config.json"
    config.parent.mkdir(parents=True)
    config.write_text(json.dumps({"source": "/source.mp4"}), encoding="utf-8")
    broken = tmp_path / "edits/replay-2/edit-config.json"
    broken.parent.mkdir(parents=True)
    broken.write_text("{", encoding="utf-8")
    assert list(relief.source_paths(tmp_path / "edits")) == [Path("/source.mp4")]
    assert "edits/replay-2/edit-config.json" in capsys.readouterr().out


def video(tmp_path: Path, number: int) -> dict[str, object]:
    movie = media(tmp_path / f"m{number}.mp4", size=10)
    original = media(tmp_path / f"o{number}.mp4", size=20)
    return {
        "id": f"2026-10-01_JST_replay-{number}-review", "movie": movie.name,
        "localPath": str(movie),
        "original": {"movie": original.name, "localPath": str(original)},
    }  # fmt: skip


def test_only_videos_beyond_the_newest_are_offloaded_and_only_when_proven(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(relief, "KEEP_VIDEOS", 1)
    rows: list[dict[str, object]] = [
        {"id": "no recording here"},
        video(tmp_path, 0),
        video(tmp_path, 1),
        video(tmp_path, 2),
        video(tmp_path, 3),
    ]
    rows[1]["localPath"] = str(tmp_path / "already-offloaded.mp4")
    assert [(row[0], row[1], row[3]) for row in relief.offload_rows(rows)] == [
        ("replay-0", "original", "media-original/o0.mp4"),
        ("replay-1", "delivery", "media/m1.mp4"),
        ("replay-1", "original", "media-original/o1.mp4"),
        ("replay-2", "delivery", "media/m2.mp4"),
        ("replay-2", "original", "media-original/o2.mp4"),
    ]

    class Session:
        def __enter__(self) -> str:
            return "store"

        def __exit__(self, *_arguments: object) -> None:
            return None

    calls: list[str] = []

    def offload(_root: Path, *, key: str, path: Path, **_kwargs: object) -> None:
        calls.append(key)
        if key == "media/m1.mp4":
            raise ReplayError("differs")
        path.unlink()

    monkeypatch.setattr(r2_worker, "session", lambda _bucket: Session())
    monkeypatch.setattr(delivery_offload, "offload", offload)
    monkeypatch.setattr(delivery_offload, "uploaded", lambda _key: None)
    monkeypatch.setattr(delivery_offload, "edit_root", lambda name: tmp_path / name)
    frees = iter([0, 0, 0, 100])
    monkeypatch.setattr(disk_budget, "measure", lambda _volume: Room(next(frees)))
    log = tmp_path / "log.jsonl"
    freed = relief.offload_oldest(rows, volume=tmp_path, target=50, keep=keep("replay-0"), log=log)
    assert calls == ["media/m1.mp4", "media-original/o1.mp4"]
    assert freed == 20
    assert (tmp_path / "m1.mp4").is_file()
    logged = [json.loads(line)["tier"] for line in log.read_text(encoding="utf-8").splitlines()]
    assert logged == ["offload"]


def test_main_runs_one_pass_or_keeps_watching(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr(relief, "relieve", lambda _checkout, *, apply, now: {"boot": int(apply)})
    assert relief.main([]) == 0
    assert json.loads(capsys.readouterr().out) == {"apply": False, "boot": 0}

    def stop(_seconds: float) -> None:
        raise KeyboardInterrupt

    with pytest.raises(KeyboardInterrupt):
        relief.main(["--apply", "--watch"], sleep=stop)
    assert json.loads(capsys.readouterr().out) == {"apply": True, "boot": 1}


def test_a_failed_pass_ends_one_run_but_not_the_watch(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.chdir(tmp_path)

    def fail(_checkout: Path, *, apply: bool, now: float) -> dict[str, int]:
        raise OSError(f"gone apply={apply} now={now > 0}")

    monkeypatch.setattr(relief, "relieve", fail)
    with pytest.raises(OSError, match="gone"):
        relief.main([])

    def stop(_seconds: float) -> None:
        raise KeyboardInterrupt

    with pytest.raises(KeyboardInterrupt):
        relief.main(["--apply", "--watch"], sleep=stop)
    assert json.loads(capsys.readouterr().out) == {"failed": "OSError: gone apply=True now=True"}


def test_a_full_pass_reads_its_settings_and_offloads_only_when_still_short(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work = tmp_path / "edits/work"
    (work / "recording-pipeline").mkdir(parents=True)
    (work / "recordings").mkdir()
    (tmp_path / "web-viewer").mkdir()
    old = media(work / "orphan/old.mp4")
    idle = media(work / "recordings/replay-1/vocals.wav", modified=NOW - 2 * 86400)
    today = media(work / "recordings/replay-1/new.wav", modified=NOW)
    (work / "recording-pipeline/DEFAULTS-v1.json").write_text(
        json.dumps({
            "deliveryRoot": str(tmp_path / "deliveries"),
            "captions": {"mediaRoot": str(tmp_path / "scratch/recordings")},
        }),
        encoding="utf-8",
    )  # fmt: skip
    (work / "recording-pipeline/RETIREMENT-v1.json").write_text(
        '{"retirementHold": ["replay-9"]}', encoding="utf-8"
    )
    (tmp_path / "web-viewer/catalog.published.json").write_text('{"videos": []}', encoding="utf-8")
    monkeypatch.setattr(relief, "running_recordings", set)
    monkeypatch.setattr(disk_budget, "measure", lambda _volume: Room(0))
    offloads: list[int] = []

    def offload(_rows: object, *, target: int, **_kwargs: object) -> int:
        offloads.append(target)
        return 3

    monkeypatch.setattr(relief, "offload_oldest", offload)
    assert relief.relieve(tmp_path, apply=False, now=NOW) == {"boot": 2097152, "ssd": 0}
    assert old.exists() and offloads == []
    assert relief.relieve(tmp_path, apply=True, now=NOW) == {
        "boot": 2097152, "ssd": 0, "offload": 3,
    }  # fmt: skip
    assert not old.exists() and not idle.exists() and today.exists()
    assert offloads == [429496729600]


def test_running_recordings_come_from_the_process_list(monkeypatch: pytest.MonkeyPatch) -> None:
    listed = (
        "python recording_pipeline run 123 --defaults d\n"
        "x recording_pipeline run youtube-abcdefghijk --d\n"
        "sh -c 'recording_pipeline run {id} '\n"
    )
    monkeypatch.setattr(subprocess, "run", lambda *_a, **_k: type("R", (), {"stdout": listed})())
    assert relief.running_recordings() == {"replay-123", "youtube-abcdefghijk"}
