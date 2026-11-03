import json
import subprocess
from collections.abc import Iterator
from contextlib import ExitStack, contextmanager
from pathlib import Path
from types import SimpleNamespace

import pytest

from replay import (
    caption_faces,
    caption_plan,
    gpu_lease,
    recording_caption_steps,
    recording_pipeline,
)
from replay import live_caption_delivery as live
from replay import pipeline_completion as completion
from replay.core import ReplayError

STAMP = "2026-11-02_18-30-45"


def put(path: Path, value: object) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(value if isinstance(value, str) else json.dumps(value), encoding="utf-8")
    return path


def recording(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> dict[str, Path]:
    """A finished burned recording: settings, every upstream receipt, the linked work inputs,
    and its burned delivery with SRT, summaries and excitement."""
    directory = tmp_path / "recordings" / "replay-1"
    work, steps = directory / "replay-1", directory / "steps"
    edit_root = tmp_path / "edits" / "replay-1"
    put(edit_root / "edit-config.json", "{}")
    storage = tmp_path / "library" / "replay-1"
    source = tmp_path / f"{STAMP}_JST_replay-1.mp4"
    monkeypatch.setattr(
        live, "load_config", lambda _root: SimpleNamespace(storage_root=storage, source=source)
    )
    pipeline = put(directory / "pipeline.json", {
        "recording": "1", "editRoot": str(edit_root), "work": str(work), "steps": str(steps),
        "python": "/py", "ffmpeg": "/ffmpeg", "ffprobe": "/ffprobe",
    })  # fmt: skip
    put(directory / "captions.json", {"mediaRoot": str(tmp_path / "media" / "recordings")})
    for name in sorted(set(live.COPIED) - live.PRODUCED):
        put(steps / f"{name}.json", {"step": name})
    for name in live.LINKED:
        put(work / name, name)
    rows = [{"cue": {"text": "あ", "start": 0, "end": 1}, "x": 540.0, "y": 100.0}]
    put(work / "placement-v1.json", {"candidates": rows})
    burned = storage / "revisions" / "y" / "delivery" / f"{STAMP}_JST_replay-1-review.mp4"
    put(burned, "burned")
    put(burned.with_suffix(".srt"), "1\n00:00:00,000 --> 00:00:01,000\nあ\n")
    put(burned.with_suffix(".srt.excitement.json"), "{}")
    put(burned.with_suffix(".srt.summaries") / "summary.json", "{}")
    put(steps / "youtube-master.json", {"outputs": {"master": {"path": str(burned)}}})
    return {"pipeline": pipeline, "steps": steps, "work": work, "burned": burned,
            "storage": storage}  # fmt: skip


def fake_steps(sibling_steps: Path, media: Path) -> None:
    """What the sibling's compose, finish and voice-remux steps would leave."""
    movie = put(media / "final-remux" / "candidate.mp4", "live movie")
    for name in ("compose", "finish"):
        put(sibling_steps / f"{name}.json", {"step": name})
    put(sibling_steps / "voice-remux.json", {"outputs": {"movie": {"path": str(movie)}}})


def test_prepare_copies_receipts_and_hard_links_inputs_once(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = recording(tmp_path, monkeypatch)
    sibling = live.prepare(paths["pipeline"])
    root = sibling.parent
    assert sibling == paths["pipeline"].parent / "live-v1" / "pipeline.json"
    settings = json.loads(sibling.read_text(encoding="utf-8"))
    assert (settings["work"], settings["steps"]) == (str(root / "work"), str(root / "steps"))
    captions = json.loads((root / "captions.json").read_text(encoding="utf-8"))
    assert captions["mediaRoot"] == str(tmp_path / "media" / "recordings-live")
    assert (root / "work" / "video-clock.json").samefile(paths["work"] / "video-clock.json")
    assert not (root / "work" / "video-clock.json").is_symlink()
    assert sorted(path.name for path in (root / "steps").glob("*.json")) == sorted(
        f"{name}.json" for name in set(live.COPIED) - live.PRODUCED
    )
    assert live.prepare(paths["pipeline"]) == sibling
    assert sorted(path.name for path in paths["steps"].iterdir()) == sorted(
        [*(f"{name}.json" for name in set(live.COPIED) - live.PRODUCED), "youtube-master.json"]
    )


def test_prepare_refuses_a_changed_sibling(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    paths = recording(tmp_path, monkeypatch)
    sibling = live.prepare(paths["pipeline"])
    (sibling.parent / "captions.json").write_text("{}", encoding="utf-8")
    with pytest.raises(ReplayError, match="exists with other content"):
        live.prepare(paths["pipeline"])


def test_prepare_refuses_a_missing_input(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    paths = recording(tmp_path, monkeypatch)
    (paths["work"] / "video-clock.json").unlink()
    with pytest.raises(ReplayError, match="video-clock.json is missing"):
        live.prepare(paths["pipeline"])


def test_a_link_target_of_another_file_is_refused(tmp_path: Path) -> None:
    source, other = put(tmp_path / "a", "a"), put(tmp_path / "b", "b")
    with pytest.raises(ReplayError, match="is not"):
        live.link(source, other)


def test_check_reports_without_writing(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    paths = recording(tmp_path, monkeypatch)
    (paths["work"] / "video-clock.json").unlink()
    report = live.check(paths["pipeline"])
    live_movie = tmp_path / "library" / "replay-1-live" / "delivery"
    assert report == {
        "sibling": str(paths["pipeline"].parent / "live-v1"),
        "missingInputs": ["video-clock.json"],
        "toRun": ["compose", "finish", "voice-remux"],
        "burnedDelivery": str(paths["burned"]),
        "liveDelivery": str(live_movie / f"{STAMP}_JST_replay-1-live-review.mp4"),
    }
    assert not (paths["pipeline"].parent / "live-v1").exists()


def test_produce_runs_each_missing_step_under_its_leases(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = recording(tmp_path, monkeypatch)
    sibling = live.prepare(paths["pipeline"])
    put(sibling.parent / "steps" / "compose.json", "{}")
    events: list[object] = []

    @contextmanager
    def lease(directory: Path, recording: str, **options: object) -> Iterator[None]:
        events.append((directory.name, recording, options["step"]))
        yield

    monkeypatch.setattr(gpu_lease, "lease", lease)
    monkeypatch.setattr(
        recording_pipeline, "serial_leases", lambda step: tmp_path / f"{step}-lease"
    )
    monkeypatch.setattr(subprocess, "run", lambda command, **_k: events.append(command[4:]))
    monkeypatch.delitem(recording_caption_steps.STEPS, "caption-render", raising=False)
    live.produce(sibling)
    assert events == [
        ("finish-lease", "1", "finish"), ["finish", str(sibling)], ["voice-remux", str(sibling)]
    ]  # fmt: skip


def test_leases_of_compose_are_the_serial_and_gpu_ones(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    taken: list[str] = []

    @contextmanager
    def lease(directory: Path, recording: str, **options: object) -> Iterator[None]:
        taken.append(directory.name)
        yield

    monkeypatch.setattr(gpu_lease, "lease", lease)
    monkeypatch.setattr(gpu_lease, "root", lambda: tmp_path / "gpu")
    monkeypatch.setattr(
        recording_pipeline, "serial_leases", lambda step: tmp_path / f"{step}-lease"
    )
    with ExitStack() as held:
        live.leases("compose", "1", held)
    assert taken == ["compose-lease", "gpu"]


def test_run_publishes_gates_and_carries_the_sidecars(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = recording(tmp_path, monkeypatch)
    monkeypatch.setattr(
        live,
        "produce",
        lambda sibling: fake_steps(
            sibling.parent / "steps", tmp_path / "media" / "recordings-live" / "replay-1"
        ),
    )
    built: list[Path] = []

    def plan(work: Path, movie: Path) -> tuple[dict[str, object], None]:
        built.append(movie)
        return {"plan": True}, None

    monkeypatch.setattr(caption_plan, "build", plan)
    monkeypatch.setattr(caption_plan, "write", lambda value, directory, icon: directory)
    monkeypatch.setattr(caption_faces, "build", lambda work, movie: {"faces": True})
    monkeypatch.setattr(caption_faces, "write", lambda value, movie: movie)
    monkeypatch.setattr(completion, "stage_failures", lambda index, **_k: {})
    result = live.run(paths["pipeline"])
    folder = tmp_path / "library" / "replay-1-live"
    delivery = folder / "delivery" / f"{STAMP}_JST_replay-1-live-review.mp4"
    assert result == {"delivery": str(delivery), "index": str(folder / "metadata" /
                      "pipeline-evidence.json"), "gate": "passed"}  # fmt: skip
    assert built == [delivery]
    assert delivery.read_text(encoding="utf-8") == "live movie"
    assert delivery.with_suffix(".srt").read_text(encoding="utf-8") == (
        paths["burned"].with_suffix(".srt").read_text(encoding="utf-8")
    )
    assert (delivery.with_suffix(".srt.summaries") / "summary.json").is_file()
    assert delivery.with_suffix(".srt.excitement.json").is_file()
    index = json.loads((folder / "metadata" / "pipeline-evidence.json").read_text("utf-8"))
    sibling_steps = paths["pipeline"].parent / "live-v1" / "steps"
    assert index["steps"]["compose"]["path"] == str(sibling_steps / "compose.json")
    assert "caption-render" not in index["steps"]
    published = json.loads((sibling_steps / "live-publish.json").read_text(encoding="utf-8"))
    assert published == {"outputs": {"delivery": str(delivery)}}
    verdict = json.loads((folder / "metadata" / "live-gate.json").read_text(encoding="utf-8"))
    assert verdict["passed"] is True
    assert paths["burned"].read_text(encoding="utf-8") == "burned"


def test_a_failed_gate_stops_the_run(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    paths = recording(tmp_path, monkeypatch)
    monkeypatch.setattr(
        live,
        "produce",
        lambda sibling: fake_steps(
            sibling.parent / "steps", tmp_path / "media" / "recordings-live" / "replay-1"
        ),
    )
    monkeypatch.setattr(caption_plan, "build", lambda work, movie: ({}, None))
    monkeypatch.setattr(caption_plan, "write", lambda value, directory, icon: directory)
    monkeypatch.setattr(caption_faces, "build", lambda work, movie: {})
    monkeypatch.setattr(caption_faces, "write", lambda value, movie: movie)
    monkeypatch.setattr(completion, "stage_failures",
                        lambda index, **_k: {"dynamic_captions": "no plan"})  # fmt: skip
    with pytest.raises(ReplayError, match="fails its completion gate"):
        live.run(paths["pipeline"])


def test_other_captions_than_the_burned_delivery_are_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = recording(tmp_path, monkeypatch)
    paths["burned"].with_suffix(".srt").write_text("other", encoding="utf-8")
    sibling = live.prepare(paths["pipeline"])
    fake_steps(sibling.parent / "steps", tmp_path / "media" / "recordings-live" / "replay-1")
    with pytest.raises(ReplayError, match="shows other captions"):
        live.publish(sibling, paths["burned"])


def test_the_publication_is_the_burned_delivery_without_a_master(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = recording(tmp_path, monkeypatch)
    (paths["steps"] / "youtube-master.json").unlink()
    put(paths["steps"] / "publish.json", {"outputs": {"delivery": str(paths["burned"])}})
    assert live.original_delivery(paths["steps"]) == paths["burned"]


def test_no_burned_delivery_on_disk_is_refused(tmp_path: Path) -> None:
    put(tmp_path / "publish.json", {"outputs": {"delivery": str(tmp_path / "gone.mp4")}})
    with pytest.raises(ReplayError, match="no burned delivery on disk"):
        live.original_delivery(tmp_path)


def test_an_unbound_storage_root_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = recording(tmp_path, monkeypatch)
    unbound = SimpleNamespace(storage_root=None, source=Path("/s"))
    monkeypatch.setattr(live, "load_config", lambda _root: unbound)
    with pytest.raises(ReplayError, match="storage root"):
        live.target(paths["pipeline"])


def test_the_cli_prints_the_check(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    paths = recording(tmp_path, monkeypatch)
    assert live.main(["check", str(paths["pipeline"])]) == 0
    assert json.loads(capsys.readouterr().out)["toRun"] == ["compose", "finish", "voice-remux"]


def test_produce_refuses_a_compose_that_still_burns(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setitem(recording_caption_steps.STEPS, "caption-render", lambda *_a: {})
    with pytest.raises(ReplayError, match="apply the pipeline switch first"):
        live.produce(tmp_path / "pipeline.json")
