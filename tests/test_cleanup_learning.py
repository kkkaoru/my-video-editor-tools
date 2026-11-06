import json
import os
from pathlib import Path

from replay import cleanup_learning as learning


def _receipt(accepted: str | None) -> dict[str, object]:
    return {
        "accepted": accepted,
        "attempts": [
            {
                "directory": "attempt-sampled",
                "residualLines": [{"box": [465.2, 1296.0, 48.0, 23.0]}],
            },
            {"directory": "attempt-full", "residualLines": []},
            {"directory": "attempt-residual", "residualLines": [{"box": [1, 2, 3, 4]}]},
        ],
    }


def test_only_residuals_before_the_accepted_attempt_are_confirmed() -> None:
    assert learning.confirmed(_receipt("attempt-full")) == [(465, 1296, 514, 1319)]
    assert learning.confirmed(_receipt(None)) == []
    assert learning.confirmed({"accepted": "a", "attempts": "x"}) == []


def test_lines_the_first_plan_read_ahead_are_confirmed_once_the_chunk_is_accepted() -> None:
    receipt: dict[str, object] = {
        "accepted": "attempt-sampled",
        "attempts": [
            {
                "directory": "attempt-sampled",
                "residualLines": [],
                "planRead": {"lines": [{"frame": 7, "box": [465.2, 1296.0, 48.0, 23.0]}]},
            }
        ],
    }
    assert learning.confirmed_frames(receipt) == [(7, (465, 1296, 514, 1319))]
    assert learning.confirmed_frames({**receipt, "accepted": None}) == []
    assert learning.confirmed_frames({"accepted": "a", "attempts": []}) == []
    assert learning.confirmed_frames({"accepted": "a", "attempts": ["x"]}) == []


def test_bands_need_two_chunks_and_exclude_the_running_one(tmp_path: Path) -> None:
    assert learning.record(tmp_path, "chunk-00001", _receipt("attempt-full")) == 1
    assert learning.record(tmp_path, "chunk-00002", {"accepted": None, "attempts": "x"}) == 0
    assert learning.bands(tmp_path) == []
    other = {
        "accepted": "attempt-full",
        "attempts": [
            {"directory": "attempt-sampled", "residualLines": [{"box": [470, 1293, 250, 26]}]},
            {"directory": "attempt-full"},
        ],
    }
    assert learning.record(tmp_path, "chunk-00003", other) == 1
    store = tmp_path / learning.STORE
    with store.open("a", encoding="utf-8") as stream:
        stream.write('{"chunk": "chunk-00009", "box": [1, 2]}\n{"partial\n' + "x" * 5000 + "\n")
    assert learning.bands(tmp_path) == [learning.Band((465, 1293, 720, 1319), 2)]
    assert learning.bands(tmp_path, exclude="chunk-00003") == []


def test_a_band_seeds_caption_frames_whose_region_reaches_it() -> None:
    band = learning.Band((465, 1292, 720, 1322), 2)
    frames: list[object] = [
        {"classification": "overlay-subtitle", "region": {"x": 0, "y": 1200, "height": 80}},
        {"classification": "overlay-subtitle", "region": {"x": 0, "y": 1000, "height": 80}},
        {"classification": "unknown"},
        {"classification": "overlay-subtitle"},
    ]
    assert learning.seeds(frames, [band]) == {0: [(465, 1292, 720, 1322)]}


def test_the_attempt_threshold_derives_from_earlier_receipts(tmp_path: Path) -> None:
    paths = []
    for index, count in enumerate([1, 2, 3, 2] * 10):
        path = tmp_path / f"r{index}.json"
        path.write_text(json.dumps({"attempts": [{}] * count}), encoding="utf-8")
        paths.append(path)
    (tmp_path / "bad.json").write_text("{", encoding="utf-8")
    counts = learning.attempts([*paths, tmp_path / "bad.json", tmp_path / "missing.json"])
    assert len(counts) == 40
    limit = learning.threshold(counts)
    assert limit is not None and round(limit, 3) == 2.316
    assert learning.threshold(counts[:39]) is None
    assert learning.anomalous([4] * 20, limit) is True
    assert learning.anomalous([2] * 20, limit) is False
    assert learning.anomalous([4] * 19, limit) is False
    assert learning.anomalous([4] * 20, None) is False


def test_the_baseline_takes_the_latest_other_cleanup_works(tmp_path: Path) -> None:
    names = ["clean-replay-1-p1", "clean-replay-2-p1", "clean-youtube-x-p1", "clean-replay-3-p1"]
    for index, name in enumerate([*names, "other"]):
        (tmp_path / name).mkdir()
        os.utime(tmp_path / name, (index, index))
    assert learning.recent_works(tmp_path / "clean-replay-3-p1", count=1) == [
        tmp_path / "clean-replay-2-p1"
    ]
    assert learning.recent_works(tmp_path / "clean-youtube-y-p1") == [
        tmp_path / "clean-youtube-x-p1"
    ]


def test_versions_change_with_the_store_and_the_code(tmp_path: Path) -> None:
    assert learning.store_version(tmp_path) == "empty"
    learning.record(tmp_path, "chunk-00001", _receipt("attempt-full"))
    first = learning.store_version(tmp_path)
    learning.record(tmp_path, "chunk-00002", _receipt("attempt-full"))
    assert learning.store_version(tmp_path) != first
    package = tmp_path / "package"
    package.mkdir()
    empty = learning.code_version(package)
    (package / "cleanup_learning.py").write_text("x = 1\n", encoding="utf-8")
    assert learning.code_version(package) != empty


def test_replacements_that_settle_pass_and_ones_that_do_not_diverge() -> None:
    n = learning.SEGMENT_CHUNKS
    settling = [(3, "a")] * n + [(2, "b")] * n + [(1, "c")] * n
    assert learning.segments(settling) == [3.0, 2.0, 1.0]
    assert learning.diverged(settling, 1.5) is False
    rising = [(2, "a")] * n + [(3, "b")] * n + [(4, "c")] * n
    assert learning.diverged(rising, 1.5) is True
    assert learning.diverged(rising, 3.5) is False  # it stays below the settle level
    assert learning.diverged(rising, None) is False
    assert learning.diverged(rising[: 2 * n], 1.5) is False
    assert learning.segments([]) == []
    # One-chunk versions (a store change per healed chunk) join runs; a short tail waits.
    flicker = [(2 + i % 2, f"v{i}") for i in range(3 * n)] + [(4, "z")]
    assert learning.segments(flicker) == [2.4, 2.6, 2.4]
    assert learning.diverged(flicker, 2.0) is False


def test_a_brake_stops_learning_until_it_is_removed(tmp_path: Path) -> None:
    work = tmp_path / "clean-replay-1-p1"
    work.mkdir()
    assert learning.braked(work) is False
    path = learning.brake(work, "did not settle", ["/r.json"])
    assert learning.braked(work) is True
    assert json.loads(path.read_text(encoding="utf-8")) == {
        "reason": "did not settle",
        "evidence": ["/r.json"],
    }
    path.unlink()
    assert learning.braked(work) is False


def test_dated_rows_export_one_training_moment_per_row_class_and_second(tmp_path: Path) -> None:
    receipt: dict[str, object] = {
        "accepted": "attempt-full",
        "attempts": [
            {
                "directory": "attempt-sampled",
                "residualLines": [
                    {"frame": 0, "box": [465.0, 1296.0, 48.0, 23.0]},
                    {"frame": 1, "box": [466.0, 1297.0, 48.0, 23.0]},
                    {"frame": 1, "box": [466.0, 1297.0, 48.0, 23.0]},
                    {"frame": 9, "box": [10.0, 1100.0, 300.0, 40.0]},
                ],
            },
            {"directory": "attempt-full"},
        ],
    }
    assert learning.record(tmp_path, "chunk-00001", receipt, [100.0, 100.5]) == 3
    store = tmp_path / learning.STORE
    with store.open("a", encoding="utf-8") as stream:
        stream.write('"text"\n{"box": [1, 2, 3, 4], "seconds": "x"}\n{bad\n')
    assert learning.training_frames(tmp_path) == [{"seconds": 100.0, "box": [465, 1296, 513, 1319]}]
    assert learning.training_frames(tmp_path / "missing") == []


def test_the_disk_estimate_counts_every_moment() -> None:
    assert learning.estimated_bytes([{}, {}]) == 280_000


def _colour_failed(accepted: str | None) -> dict[str, object]:
    line = {"frame": 10, "box": [465.0, 1296.0, 48.0, 23.0]}
    rim = {"frame": 40, "box": [900.0, 1220.0, 30.0, 8.0]}
    beside = {"frame": 12, "box": [470.0, 1300.0, 10.0, 10.0]}
    return {
        "accepted": accepted,
        "attempts": [
            {
                "directory": "attempt-sampled",
                "residualLines": [line],
                "colourRegions": [rim, beside],
            },
            {"directory": "attempt-full", "residualLines": [], "colourRegions": [rim]},
        ],
    }


def test_a_chunk_that_failed_every_attempt_gives_its_lines_and_no_colour_regions() -> None:
    assert learning.candidate_frames(_colour_failed(None)) == [
        (10, (465, 1296, 513, 1319), "verified"),
    ]
    assert learning.confirmed(_colour_failed(None)) == []


def test_a_healed_chunk_gives_no_candidates() -> None:
    assert learning.candidate_frames(_colour_failed("attempt-full")) == []
    assert learning.candidate_frames({"accepted": 3, "attempts": []}) == []


def test_unconfirmed_rows_seed_no_band_and_reach_no_training_pool(tmp_path: Path) -> None:
    times = [float(second) for second in range(60)]
    assert learning.record(tmp_path, "chunk-00001", _colour_failed(None), times) == 1
    assert learning.record(tmp_path, "chunk-00002", _colour_failed(None), times) == 1
    store = tmp_path / learning.STORE
    with store.open("a", encoding="utf-8") as stream:  # a row an earlier version wrote
        stream.write('{"chunk": "chunk-00003", "box": [900, 1220, 930, 1228], '
                     '"status": "unconfirmed", "seconds": 40.0}\n')  # fmt: skip
    assert json.loads(store.read_text(encoding="utf-8").splitlines()[0]) == {
        "chunk": "chunk-00001", "box": [465, 1296, 513, 1319], "status": "verified",
        "seconds": 10.0,
    }  # fmt: skip
    assert learning.bands(tmp_path) == [learning.Band((465, 1296, 513, 1319), 2)]
    assert learning.training_frames(tmp_path) == [{"seconds": 10.0, "box": [465, 1296, 513, 1319]}]
