import json
import os
import time
from pathlib import Path

import pytest

from replay import pipeline_events as events
from replay import workflow_parallel

CALM = events.Health(memory_free_percent=80, disk_free_bytes=100 * 1024**3, now=0.0)


def _sources(tmp_path: Path) -> events.Sources:
    (tmp_path / "recordings").mkdir()
    return events.Sources(
        failures=tmp_path / "failures.jsonl",
        runner_log=tmp_path / "runner.log",
        recordings=tmp_path / "recordings",
        tails=(tmp_path / "finish-new.txt",),
    )


def _snapshot(sources: events.Sources, states: dict[str, str]) -> None:
    rows = [{"recording": key, "state": value} for key, value in states.items()]
    with sources.runner_log.open("a", encoding="utf-8") as stream:
        stream.write("dispatched pipeline-v1\n" + json.dumps({"recordings": rows}) + "\n")
    os.utime(sources.runner_log, (0, 0))


def _receipt(sources: events.Sources, directory: str, step: str) -> None:
    steps = sources.recordings / directory / "steps"
    steps.mkdir(parents=True, exist_ok=True)
    (steps / f"{step}.json").write_text("{}", encoding="utf-8")


def test_first_pass_sets_baselines_then_reports_only_changes(tmp_path: Path) -> None:
    sources = _sources(tmp_path)
    sources.failures.write_text('{"t": 0, "recording": "1", "step": "old"}\n', encoding="utf-8")
    _snapshot(sources, {"1": "running", "youtube-x": "ready"})
    _receipt(sources, "replay-1", "clocks")
    watch = events.Watch()
    assert events.scan(watch, sources, CALM) == []
    with sources.failures.open("a", encoding="utf-8") as stream:
        stream.write(
            '{"t": 0, "recording": "1", "step": "cleanup", "class": "chunk", "cause": "boom"}\n'
            "not json\n"
            '{"t": 0, "recording": "partial'
        )
    sources.tails[0].write_text("10/23 gate complete\n", encoding="utf-8")
    _snapshot(sources, {"1": "drained", "youtube-x": "ready"})
    _receipt(sources, "replay-1", "cleanup")
    _receipt(sources, "replay-1", "drain-123")
    _receipt(sources, "youtube-x", "asr")
    found = events.scan(watch, sources, CALM)
    assert found == [
        f"FAIL {time.strftime('%H:%M', time.localtime(0))} 1 cleanup chunk boom",
        "FAIL not json",
        "FINISH-NEW 10/23 gate complete",
        "STATE 1 running -> drained",
        "STEP 1 cleanup",
        "STEP youtube-x asr",
    ]
    assert events.scan(watch, sources, CALM) == []


def test_shrunk_file_is_read_from_the_start(tmp_path: Path) -> None:
    sources = _sources(tmp_path)
    sources.tails[0].write_text("one\ntwo\n", encoding="utf-8")
    _snapshot(sources, {})
    watch = events.Watch()
    assert events.scan(watch, sources, CALM) == []
    sources.tails[0].write_text("x\n", encoding="utf-8")
    assert events.scan(watch, sources, CALM) == ["FINISH-NEW x"]


def test_alerts_are_edge_triggered(tmp_path: Path) -> None:
    sources = _sources(tmp_path)
    low = events.Health(memory_free_percent=10, disk_free_bytes=1, now=1000.0)
    watch = events.Watch()
    assert events.scan(watch, sources, low) == [
        "ALERT boot volume free space below 30 GiB",
        "ALERT free memory 10% below 20%",
        "ALERT queue runner log quiet for over 5 min",
    ]
    assert events.scan(watch, sources, low) == []
    _snapshot(sources, {})
    assert events.scan(watch, sources, CALM) == [
        "CLEAR boot volume free space below 30 GiB",
        "CLEAR free memory 10% below 20%",
        "CLEAR queue runner log quiet for over 5 min",
    ]


def test_runner_log_without_a_valid_snapshot_has_no_states(tmp_path: Path) -> None:
    log = tmp_path / "runner.log"
    log.write_text("{broken\nplain line\n", encoding="utf-8")
    assert events._runner_states(log) == {}
    assert events._runner_states(tmp_path / "missing.log") == {}


def test_run_emits_each_pass_and_sleeps(tmp_path: Path) -> None:
    sources = _sources(tmp_path)
    _snapshot(sources, {})
    printed: list[str] = []
    slept: list[float] = []
    events.run(
        sources,
        interval=2.5,
        passes=2,
        health=lambda _disk: events.Health(memory_free_percent=5, disk_free_bytes=1, now=0.0),
        sleep=slept.append,
        emit=printed.append,
    )
    assert printed == [
        "ALERT boot volume free space below 30 GiB",
        "ALERT free memory 5% below 20%",
    ]
    assert slept == [2.5, 2.5]


def test_measure_reads_memory_and_disk(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    load = workflow_parallel.Load(cpus=1, load=0.0, memory_free_percent=42, gpu_percent=0)
    monkeypatch.setattr(workflow_parallel, "read_load", lambda: load)
    health = events.measure(tmp_path)
    assert health.memory_free_percent == 42
    assert health.disk_free_bytes > 0


def test_main_parses_arguments(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    seen: list[tuple[events.Sources, float]] = []
    monkeypatch.setattr(
        events, "run", lambda sources, *, interval: seen.append((sources, interval))
    )
    argv = ["--failures", "f", "--runner-log", "r", "--recordings", "d", "--tail", "t"]
    assert events.main([*argv, "--interval", "3"]) == 0
    assert seen == [(events.Sources(Path("f"), Path("r"), Path("d"), (Path("t"),)), 3.0)]
