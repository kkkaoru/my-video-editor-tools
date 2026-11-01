import json
from collections.abc import Sequence
from pathlib import Path

import numpy as np
import pytest

from replay import cleanup_learning, gpu_lease, memory_admission
from replay import yolo_learning as learning
from replay.core import ReplayError
from replay.subtitle_inpaint import Image
from replay.subtitle_yolo_regions import YoloSettings

FACE_WINDOW = json.dumps([{"method": "detection", "faces": [{"region": {}}]}])
PASS = {"face": True, "caption": True, "speed": True, "joint": True}


def _result(glyph: float, face: float, joint: bool = True) -> dict[str, object]:
    return {
        "caption": {"glyphCoverage": glyph},
        "face": {"detectionRate": face},
        "pass": {**PASS, "joint": joint},
    }


def _settings(tmp_path: Path) -> YoloSettings:
    pinned = tmp_path / "models" / "pinned.pt"
    pinned.parent.mkdir(parents=True)
    pinned.write_bytes(b"pinned")
    return YoloSettings(pinned, "p" * 64, tmp_path / "cfg", tmp_path)


def test_a_promoted_intact_model_replaces_the_pinned_one_unless_braked(tmp_path: Path) -> None:
    settings = _settings(tmp_path)
    models = settings.weights.parent
    assert learning.promoted(settings, models, tmp_path) == settings
    best = tmp_path / "best.pt"
    best.write_bytes(b"learned")
    weights = learning.promote(best, models)
    swapped = learning.promoted(settings, models, tmp_path)
    assert swapped.weights == weights
    assert (weights.parent / "weights.pt.sha256").read_text().startswith(swapped.sha256)
    (tmp_path / cleanup_learning.BRAKE).write_text("{}", encoding="utf-8")
    assert learning.promoted(settings, models, tmp_path) == settings
    (tmp_path / cleanup_learning.BRAKE).unlink()
    weights.write_bytes(b"changed")
    assert learning.promoted(settings, models, tmp_path) == settings


def test_face_windows_are_requested_once_per_moment_and_reused(tmp_path: Path) -> None:
    calls: list[Sequence[str]] = []

    def run(command: Sequence[str]) -> int:
        calls.append(command)
        Path(command[-1][1:]).write_text("[]", encoding="utf-8")
        return 0

    source = tmp_path / "s.mp4"
    first = learning.label(source, [10.2, 10.2, 0.1], tmp_path / "w", native=Path("/n"), run=run)
    again = learning.label(source, [10.2], tmp_path / "w", native=Path("/n"), run=run)
    assert [path.parent.name for path in first] == ["window-000000000100", "window-000000010200"]
    assert again == first[1:]
    assert len(calls) == 2
    request = json.loads((first[0].parent / "request.json").read_text(encoding="utf-8"))
    assert request["plan"]["startSeconds"] == 0.0
    with pytest.raises(ReplayError, match="face window failed"):
        learning.label(source, [99.0], tmp_path / "w", native=Path("/n"), run=lambda _c: 1)


def test_the_data_yaml_lists_every_training_file_and_the_validation_list(tmp_path: Path) -> None:
    out = learning.data_yaml(
        [Path("/new/train.txt"), Path("/r.txt")], Path("/v.txt"), tmp_path / "d.yaml"
    )
    assert out.read_text(encoding="utf-8").splitlines()[1:3] == [
        "train: [/new/train.txt, /r.txt]",
        "val: /v.txt",
    ]


def test_sampling_spreads_lines_evenly() -> None:
    assert learning.sampled(["a", "b", "c", "d"], 2) == ["a", "c"]
    assert learning.sampled(["a"], 3) == ["a"]
    assert learning.sampled(["a"], 0) == []


def test_new_face_windows_stop_at_the_deadline_while_cached_ones_still_count(
    tmp_path: Path,
) -> None:
    def run(command: Sequence[str]) -> int:
        Path(command[-1][1:]).write_text("[]", encoding="utf-8")
        return 0

    source = tmp_path / "s.mp4"
    cached = learning.label(source, [1.0], tmp_path / "w", native=Path("/n"), run=run)
    later = learning.label(
        source,
        [1.0, 2.0],
        tmp_path / "w",
        native=Path("/n"),
        run=run,
        deadline=0.0,
        clock=lambda: 5.0,
    )
    assert later == cached


def test_promotion_needs_every_gate_and_at_least_the_pinned_numbers() -> None:
    pinned = {"recordings": {"oct07": _result(0.9888, 1.0)}}
    good = {"recordings": {"oct07": _result(0.99, 1.0), "reg": _result(0.99, 1.0)}}
    assert learning.decide(good, pinned, ["oct07"], ["reg"]) == (True, "meets the pinned bar")
    lower = {"recordings": {"oct07": _result(0.98, 1.0), "reg": _result(0.99, 1.0)}}
    assert learning.decide(lower, pinned, ["oct07"], ["reg"])[0] is False
    failing = {"recordings": {"oct07": _result(0.99, 1.0), "reg": _result(0.99, 1.0, False)}}
    assert learning.decide(failing, pinned, ["oct07"], ["reg"]) == (False, "reg fails a gate")
    glyph = {"pass": {"caption": True, "joint": False}}
    with_glyph = {"recordings": {**good["recordings"], "glyph": glyph}}
    assert learning.decide(with_glyph, pinned, ["oct07"], ["reg"], ["glyph"])[0] is True
    missed = {"recordings": {**good["recordings"], "glyph": {"pass": {"caption": False}}}}
    assert learning.decide(missed, pinned, ["oct07"], ["reg"], ["glyph"]) == (
        False,
        "glyph fails a gate",
    )


def _config(tmp_path: Path, store: Path) -> dict[str, object]:
    base = tmp_path / "base"
    (base / "images" / "oct07").mkdir(parents=True)
    (base / "labels" / "oct07").mkdir(parents=True)
    (base / "images" / "oct07" / "oct07_0000000.jpg").write_bytes(b"j")
    (base / "labels" / "oct07" / "oct07_0000000.txt").write_text("", encoding="utf-8")
    (base / "train.txt").write_text("/base/a.jpg\n/base/b.jpg\n", encoding="utf-8")
    pinned_eval = tmp_path / "pinned-eval.json"
    pinned_eval.write_text(json.dumps({"recordings": {"oct07": _result(0.98, 0.99)}}), "utf-8")
    return {
        "work": str(tmp_path / "learning"),
        "native": "/native",
        "python": "python",
        "recordings": [{"name": "r1", "source": str(tmp_path / "s.mp4"), "store": str(store)}],
        "regression": [
            {"name": "reg", "source": str(tmp_path / "s.mp4"), "seconds": [5.0]},
            {
                "name": "glyph",
                "source": str(tmp_path / "s.mp4"),
                "seconds": [6.0],
                "glyphOnly": True,
            },
        ],
        "baseDataset": str(base),
        "pinned": {
            "weights": str(tmp_path / "pinned.pt"),
            "sha256": "a" * 64,
            "eval": str(pinned_eval),
        },
        "configDirectory": str(tmp_path / "cfg"),
        "checkoutWork": str(tmp_path),
        "frozen": ["oct07"],
        "models": str(tmp_path / "models"),
        "minRows": 1,
    }


def _store(tmp_path: Path) -> Path:
    store = tmp_path / "clean-replay-1-p1"
    receipt: dict[str, object] = {
        "accepted": "attempt-full",
        "attempts": [
            {"directory": "attempt-sampled", "residualLines": [{"frame": 0, "box": [1, 1, 9, 9]}]},
            {"directory": "attempt-full"},
        ],
    }
    cleanup_learning.record(store, "chunk-00000", receipt, [12.0])
    return store


def test_a_cycle_labels_builds_trains_evaluates_and_promotes(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.setattr(gpu_lease, "root", lambda: tmp_path / "gpu-leases")
    monkeypatch.setattr(memory_admission, "root", lambda: tmp_path / "reservations")
    store = _store(tmp_path)
    config = _config(tmp_path, store)
    assert learning.due(config) is True
    modules: list[str] = []

    def run(command: Sequence[str]) -> int:
        if command[1] == "measure-media":
            Path(command[-1][1:]).write_text(FACE_WINDOW, encoding="utf-8")
            return 0
        module = command[3]
        modules.append(module)
        if module == "replay.yolo_joint_dataset":
            output = Path(json.loads(Path(command[4]).read_text("utf-8"))["output"])
            (output / "images" / "reg").mkdir(parents=True)
            (output / "images" / "reg" / "reg_0000000.jpg").write_bytes(b"j")
            (output / "data.yaml").write_text("x", encoding="utf-8")
            (output / "train.txt").write_text("/new/a.jpg\n", encoding="utf-8")
        elif module == "replay.yolo_train":
            best = Path(command[5]) / "train" / "weights" / "best.pt"
            best.parent.mkdir(parents=True)
            best.write_bytes(b"best")
        else:
            names = [command[i + 1] for i, arg in enumerate(command) if arg == "--recording"]
            out = {"recordings": {name: _result(0.99, 1.0) for name in names}}
            Path(command[6]).write_text(json.dumps(out), encoding="utf-8")
        return 0

    outcome = learning.cycle(
        config,
        run=run,
        screening=lambda _s, items: ([a for a, _ in items], []),
        decode=lambda _s, _at: np.full((1920, 1080, 3), 200, dtype=np.uint8),
    )
    assert outcome["accepted"] is True
    assert Path(str(outcome["promoted"])).read_bytes() == b"best"
    assert modules == [
        "replay.yolo_joint_dataset",
        "replay.yolo_train",
        "replay.yolo_joint_eval",
        "replay.yolo_joint_eval",
        "replay.yolo_joint_eval",
        "replay.yolo_joint_eval",
    ]
    seconds = outcome["seconds"]
    assert isinstance(seconds, dict)
    assert set(seconds) == {"labels", "build", "train", "eval", "total"}
    phases = [line.split(" ", 3)[3] for line in capsys.readouterr().err.splitlines()]
    assert phases[0].startswith("cycle 1 start from ")
    for phase in (
        "screen ",
        "faces ",
        "dataset build ",
        "train start ",
        "eval frozen",
        "decision: accepted",
    ):
        assert any(line.startswith(phase) for line in phases), phase
    skipped = learning.cycle(
        config, run=run, screening=lambda _s, items: ([], [a for a, _ in items])
    )
    assert skipped["skipped"] == "no labelled new moment"
    assert learning.due(config) is False
    (tmp_path / cleanup_learning.BRAKE).write_text("{}", encoding="utf-8")
    assert learning.due({**config, "minRows": 0}) is False


def test_a_failed_training_stops_the_cycle(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(gpu_lease, "root", lambda: tmp_path / "gpu-leases")
    monkeypatch.setattr(memory_admission, "root", lambda: tmp_path / "reservations")
    config = _config(tmp_path, _store(tmp_path))

    def run(command: Sequence[str]) -> int:
        if command[1] == "measure-media":
            Path(command[-1][1:]).write_text(FACE_WINDOW, encoding="utf-8")
            return 0
        if command[3] == "replay.yolo_joint_dataset":
            return 1
        return 0

    with pytest.raises(ReplayError, match="dataset could not be built"):
        learning.cycle(
            config,
            run=run,
            screening=lambda _s, items: ([a for a, _ in items], []),
            decode=lambda _s, _at: np.full((8, 8, 3), 200, dtype=np.uint8),
        )


def test_the_shell_runs_at_nice_15_and_can_capture_standard_output(tmp_path: Path) -> None:
    target = tmp_path / "out.txt"
    assert learning.shell(["/bin/echo", "hello", f">{target}"]) == 0
    assert target.read_text(encoding="utf-8") == "hello\n"
    assert learning.shell(["/usr/bin/true", "x"]) == 0


def test_main_reports_whether_a_cycle_is_due(tmp_path: Path) -> None:
    config = _config(tmp_path, _store(tmp_path))
    path = tmp_path / "config.json"
    path.write_text(json.dumps({**config, "minRows": 5}), encoding="utf-8")
    assert learning.main(["due", str(path)]) == 1


def test_moments_spread_evenly_under_the_limit() -> None:
    assert learning._spread([3.0, 1.0, 2.0], 5) == [1.0, 2.0, 3.0]
    assert learning._spread([float(v) for v in range(10)], 3) == [0.0, 4.0, 9.0]


def test_screening_keeps_only_misses_and_one_of_near_identical_ones() -> None:
    plain = np.zeros((100, 100, 3), dtype=np.uint8)
    other = plain.copy()
    other[:, 50:] = 255
    seen = plain.copy()
    frames = {1.0: plain, 2.0: plain.copy(), 3.0: other, 4.0: seen}
    box = [10.0, 40.0, 90.0, 60.0]

    def detect(images: list[Image]) -> list[list[tuple[float, float, float, float]]]:
        return [[(0.0, 0.0, 100.0, 100.0)] if image is seen else [] for image in images]

    misses, hits = learning.screen(
        [(1.0, box), (2.0, box), (3.0, box), (4.0, box)], decode=frames.__getitem__, detect=detect
    )
    assert misses == [1.0, 3.0]
    assert hits == [2.0, 4.0]
    assert learning.covered([(0, 0, 50, 100)], box) is True
    assert learning.covered([(0, 0, 20, 100)], box) is False


def test_learning_waits_while_a_cleanup_chunk_waits_for_memory(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    waiter = gpu_lease.Waiter(tmp_path, "9", queue=learning.MEMORY_QUEUE)
    waiter.register()
    slept: list[float] = []

    def sleep(seconds: float) -> None:
        slept.append(seconds)
        waiter.close()

    learning.idle_cleanup(tmp_path, sleep=sleep)
    assert slept == [learning.LEASE_POLL_SECONDS]
    log = capsys.readouterr().err
    assert "paused: 1 cleanup chunks wait" in log and "resumed:" in log
    assert gpu_lease.waiting(tmp_path / "none") == 0


def test_a_frame_that_cannot_be_decoded_is_refused(tmp_path: Path) -> None:
    with pytest.raises(ReplayError, match="could not be decoded"):
        learning.decode_frame(tmp_path / "missing.mp4", 1.0, ffmpeg="/usr/bin/false")


def test_face_less_windows_are_not_dataset_frames(tmp_path: Path) -> None:
    empty, faced = tmp_path / "a.json", tmp_path / "b.json"
    empty.write_text(json.dumps([{"method": "missing", "faces": []}]), encoding="utf-8")
    faced.write_text(FACE_WINDOW, encoding="utf-8")
    assert learning.has_face(empty) is False
    assert learning.has_face(faced) is True


def test_a_cycle_whose_new_moments_have_no_face_is_skipped(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(gpu_lease, "root", lambda: tmp_path / "gpu-leases")
    config = _config(tmp_path, _store(tmp_path))

    def run(command: Sequence[str]) -> int:
        Path(command[-1][1:]).write_text("[]", encoding="utf-8")
        return 0

    outcome = learning.cycle(
        config, run=run, screening=lambda _s, items: ([a for a, _ in items], [])
    )
    assert outcome["skipped"] == "no new moment has a trusted face box"


def test_each_dataset_attempt_gets_a_fresh_directory_and_a_complete_one_is_reused(
    tmp_path: Path,
) -> None:
    assert learning.dataset_attempt(tmp_path) == tmp_path / "dataset-1"
    (tmp_path / "dataset-1").mkdir()  # an attempt that stopped midway
    assert learning.dataset_attempt(tmp_path) == tmp_path / "dataset-2"
    (tmp_path / "dataset-2").mkdir()
    (tmp_path / "dataset-2" / "data.yaml").write_text("x", encoding="utf-8")
    assert learning.dataset_attempt(tmp_path) == tmp_path / "dataset-2"


def test_a_decided_cycle_keeps_only_its_decision_metrics_and_manifest(tmp_path: Path) -> None:
    cycle = tmp_path / "cycle-0001"
    (cycle / "dataset-1" / "images").mkdir(parents=True)
    (cycle / "dataset-1" / "images" / "a.jpg").write_bytes(b"12345")
    (cycle / "runs").mkdir()
    for name in ("decision.json", "manifest.json", "eval-frozen.json", "val.txt"):
        (cycle / name).write_text("x", encoding="utf-8")
    log = tmp_path / "deletions.md"
    assert learning.prune_cycle(cycle, log) == 6
    assert sorted(path.name for path in cycle.iterdir()) == [
        "decision.json",
        "eval-frozen.json",
        "manifest.json",
    ]
    assert "dataset-1 (5 bytes)" in log.read_text(encoding="utf-8")


def test_stopped_attempts_of_an_undecided_cycle_are_removed(tmp_path: Path) -> None:
    cycle = tmp_path / "cycle-0002"
    (cycle / "dataset-1").mkdir(parents=True)
    (cycle / "dataset-2").mkdir()
    (cycle / "dataset-2" / "data.yaml").write_text("x", encoding="utf-8")
    log = tmp_path / "deletions.md"
    learning.prune_attempts(cycle, log)
    assert sorted(path.name for path in cycle.iterdir()) == ["dataset-2"]
    assert learning.prune_attempts(tmp_path / "missing", log) == 0


def test_only_the_promoted_and_one_earlier_learned_model_are_kept(tmp_path: Path) -> None:
    import os

    models = tmp_path / "models"
    for index, digest in enumerate(["aaa", "bbb", "ccc"]):
        folder = models / f"learned-{digest}"
        folder.mkdir(parents=True)
        (folder / "weights.pt").write_bytes(b"w")
        os.utime(folder, (index, index))
    (models / "promoted.json").write_text(
        json.dumps({"weights": str(models / "learned-aaa" / "weights.pt")}), encoding="utf-8"
    )
    learning.prune_weights(models, tmp_path / "deletions.md")
    assert sorted(path.name for path in models.glob("learned-*")) == ["learned-aaa", "learned-ccc"]


def test_the_face_cache_keeps_the_newest_windows_and_every_regression_one(tmp_path: Path) -> None:
    import os

    for name, count in (("r1", 3), ("reg", 2)):
        for index in range(count):
            window = tmp_path / name / f"window-{index:012d}"
            window.mkdir(parents=True)
            (window / "response.json").write_text("[]", encoding="utf-8")
            os.utime(window, (index, index))
    learning.bound_faces(tmp_path, 1, tmp_path / "deletions.md", ["reg"])
    assert sorted(str(p.relative_to(tmp_path)) for p in tmp_path.glob("*/window-*")) == [
        "r1/window-000000000002",
        "reg/window-000000000000",
        "reg/window-000000000001",
    ]
    assert learning.tree_bytes(tmp_path / "missing") == 0
    assert learning.tree_bytes(tmp_path / "deletions.md") > 0


def test_a_cycle_over_its_disk_budget_after_eviction_stops_with_an_error(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(gpu_lease, "root", lambda: tmp_path / "gpu-leases")
    config = {**_config(tmp_path, _store(tmp_path)), "diskCapBytes": 0}
    (Path(str(config["work"])) / "big").mkdir(parents=True)
    (Path(str(config["work"])) / "big" / "f").write_bytes(b"x" * 10)
    with pytest.raises(ReplayError, match="disk budget exceeded"):
        learning.cycle(config, run=lambda _c: 0)
