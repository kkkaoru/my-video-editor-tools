"""A redo request restarts a recording from chosen steps, or only some of its cleanup chunks."""

import json
from pathlib import Path

import pytest

from replay import recording_redo as redo
from replay import recording_steps as steps
from replay.core import ReplayError


def test_a_request_is_stored_read_and_cleared(tmp_path: Path) -> None:
    queue = tmp_path / "queue.json"
    request = redo.validated(("compose", "paging", "compose"), ())
    assert request == redo.Request(("compose", "paging"))
    assert redo.write(queue, "3", request) == tmp_path / "redo/3.json"
    assert redo.pending(queue, "3") == request
    redo.clear(queue, "3")
    assert redo.pending(queue, "3") is None


@pytest.mark.parametrize(
    ("names", "chunks", "message"),
    [
        ((), (), "none given"),
        (("compos",), (), "unknown: compos"),
        (("compose",), (3,), "give it with --from cleanup"),
        (("cleanup",), (3, 3), "distinct chunk numbers"),
        (("cleanup",), (-1,), "distinct chunk numbers"),
    ],
)
def test_invalid_requests_are_refused_with_the_reason(
    names: tuple[str, ...], chunks: tuple[int, ...], message: str
) -> None:
    with pytest.raises(ReplayError, match=message):
        redo.validated(names, chunks)


def test_a_damaged_request_file_is_refused(tmp_path: Path) -> None:
    (tmp_path / "redo").mkdir()
    (tmp_path / "redo/3.json").write_text('{"version": 1, "from": "compose"}', "utf-8")
    with pytest.raises(ReplayError, match="is not a redo request"):
        redo.pending(tmp_path / "queue.json", "3")


def test_chunks_force_the_cleanups_consumers_not_the_cleanup() -> None:
    assert redo.forcing(redo.Request(("cleanup", "paging"), (4,))).names == (
        "retire-intermediates", "compose", "paging"
    )  # fmt: skip
    assert redo.forcing(redo.Request(("cleanup",))).names == ("cleanup",)


def cleaned(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> steps.Pipeline:
    """A recording whose cleanup ran three chunks (media kept) and joined the source."""
    monkeypatch.chdir(tmp_path)
    work, media = tmp_path / "clean", tmp_path / "media"
    pipeline = steps.Pipeline(
        tmp_path / "pipeline.json",
        {"work": str(tmp_path / "replay-3"), "steps": str(tmp_path / "steps"),
         "recording": "3"},
    )  # fmt: skip
    pipeline.description.parent.mkdir(parents=True)
    pipeline.description.write_text(
        json.dumps({"work": str(work), "mediaRoot": str(tmp_path), "recording": "media"}), "utf-8"
    )
    for name in ("chunk-00000", "chunk-00001", "chunk-00002"):
        (work / "configs").mkdir(parents=True, exist_ok=True)
        (work / "configs" / f"{name}.json").write_text("{}", "utf-8")
        (work / "state" / name).mkdir(parents=True)
        (media / "sampled" / name).mkdir(parents=True)
    (work / "logs").mkdir()
    (work / "logs/chunk-00001.log").write_text("log", "utf-8")
    (work / "consumer").mkdir()
    (tmp_path / "steps").mkdir()
    (tmp_path / "steps/cleanup.json").write_text("{}", "utf-8")
    return pipeline


def test_chosen_chunks_are_set_aside_with_the_join_and_logged(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    pipeline = cleaned(tmp_path, monkeypatch)
    log = tmp_path / "deletions.md"
    moved = redo.set_aside_chunks(pipeline, (1,), log, now=100)
    work, media = tmp_path / "clean", tmp_path / "media"
    assert moved == [
        (work / "state/chunk-00001", work / "interrupted/chunk-00001-redo-100"),
        (media / "sampled/chunk-00001", media / "interrupted/chunk-00001-redo-100"),
        (work / "logs/chunk-00001.log", work / "logs/chunk-00001.redo-100.log"),
        (work / "consumer", work / "interrupted/consumer-redo-100"),
        (tmp_path / "steps/cleanup.json", tmp_path / "steps/interrupted/cleanup-100.json"),
    ]
    assert (work / "state/chunk-00000").exists()
    assert "cleanup chunks 00001 of 3 set aside" in log.read_text("utf-8")


def test_a_set_aside_needs_chunks_with_their_media(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    pipeline = cleaned(tmp_path, monkeypatch)
    log = tmp_path / "deletions.md"
    with pytest.raises(ReplayError, match="No such cleanup chunks: chunk-00009"):
        redo.set_aside_chunks(pipeline, (9,), log)
    (tmp_path / "media/sampled/chunk-00002").rmdir()
    with pytest.raises(ReplayError, match="media of 1 other chunks was retired"):
        redo.set_aside_chunks(pipeline, (1,), log)
    pipeline.description.unlink()
    with pytest.raises(ReplayError, match="caption-free source"):
        redo.set_aside_chunks(pipeline, (1,), log)
