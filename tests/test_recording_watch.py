"""The watcher stays silent while a queue works and reports each actionable event once."""

import json
from pathlib import Path
from typing import TypedDict

import pytest

from replay import disk_budget, recording_progress
from replay import recording_pipeline as pipelines
from replay import recording_watch as watch


class Values(TypedDict):
    defaults: dict[str, object]
    jobs: Path
    checkout: Path
    now: float


class Room:
    def __init__(self, spare: int) -> None:
        self.spare = spare


def setup(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, rows: list[dict[str, object]]) -> Values:
    jobs = tmp_path / "jobs"
    jobs.mkdir()
    (jobs / "queue-runner.log").write_text(
        "not json\n" + json.dumps({"at": 1, "recordings": rows}) + "\nQueue tick failed\n",
        encoding="utf-8",
    )
    monkeypatch.setattr(disk_budget, "measure", lambda _path: Room(1))
    defaults: dict[str, object] = {
        "root": str(tmp_path / "edits/work/recordings"),
        "python": "/py",
        "deliveryRoot": str(tmp_path),
    }
    return {
        "defaults": defaults,
        "jobs": jobs,
        "checkout": tmp_path,
        "now": (jobs / "queue-runner.log").stat().st_mtime,
    }


def kinds(queue: Path, values: Values) -> list[object]:
    return [event["kind"] for event in watch.report(queue, **values)]


def test_a_working_queue_is_silent(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    rows: list[dict[str, object]] = [
        {"recording": "1", "state": "running"},
        {"recording": "2", "state": "ready"},
        {"recording": "3", "state": "launched"},
    ]
    values = setup(tmp_path, monkeypatch, rows)
    assert kinds(tmp_path / "q.json", values) == []
    assert not (tmp_path / "q.json.events.jsonl").exists()


def test_a_held_recording_reports_its_log_once_and_a_later_failure_again(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    values = setup(tmp_path, monkeypatch, [{"recording": "7", "state": "held"}])
    layout = pipelines.layout_of("7", values["defaults"], tmp_path)
    layout.directory.mkdir(parents=True)
    (layout.directory / "pipeline-v1.launch").touch()
    (layout.directory / "pipeline-v1.log").write_text("boom\n", encoding="utf-8")
    queue = tmp_path / "q.json"
    first = watch.report(queue, **values)
    assert [(event["kind"], event["launch"], event["log"]) for event in first] == [
        ("held", "pipeline-v1", "boom")
    ]
    assert "resumes at the failed step" in str(first[0]["resume"])
    assert watch.report(queue, **values) == []
    (layout.directory / "pipeline-v2.launch").touch()
    assert kinds(queue, values) == ["held"]


def test_a_finished_recording_a_failed_intake_and_a_stopped_runner_are_reported(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    values = setup(tmp_path, monkeypatch, [{"recording": "7", "state": "done"}])
    jobs = values["jobs"]
    (jobs / "intake-a.status").write_text("1\n", encoding="utf-8")
    (jobs / "intake-b.status").write_text("0\n", encoding="utf-8")
    (jobs / "intake-a.log").write_text("no original\n", encoding="utf-8")
    queue = tmp_path / "q.json"
    assert kinds(queue, values) == ["intake-failed", "finished"]
    stale = values["now"] + watch.STALE_SECONDS + 1
    assert kinds(queue, {**values, "now": stale}) == ["runner-stopped"]
    assert kinds(queue, {**values, "now": stale}) == []
    (jobs / "queue-runner.log").unlink()
    assert kinds(queue, {**values, "now": stale + watch.STALE_SECONDS}) == ["runner-stopped"]


def test_a_volume_below_its_floor_and_an_unreadable_queue_state_are_reported(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    values = setup(tmp_path, monkeypatch, [])
    monkeypatch.setattr(disk_budget, "measure", lambda _path: Room(-5))
    assert kinds(tmp_path / "q.json", values) == ["disk-low"]
    capsys.readouterr()
    monkeypatch.setattr(pipelines, "load_defaults", lambda _path: values["defaults"])
    monkeypatch.chdir(tmp_path)

    def unreadable(*_arguments: object, **_options: object) -> list[dict[str, object]]:
        raise OSError("gone")

    monkeypatch.setattr(watch, "report", unreadable)
    arguments = ["--queue", "q.json", "--defaults", "d.json", "--jobs", "jobs"]
    assert watch.main([*arguments, "--once"]) == 0
    assert json.loads(capsys.readouterr().out) == {
        "kind": "watch-failed",
        "detail": "OSError: gone",
    }
    slept: list[float] = []

    def stop(seconds: float) -> None:
        slept.append(seconds)
        raise KeyboardInterrupt

    with pytest.raises(KeyboardInterrupt):
        watch.main(arguments, sleep=stop)
    assert slept == [60.0]


def test_a_tail_without_a_log_is_empty_and_a_log_without_a_tick_has_no_rows(
    tmp_path: Path,
) -> None:
    assert watch.log_tail(tmp_path, "pipeline-v1") == ""
    log = tmp_path / "runner.log"
    log.write_text('{"at": 1}\n', encoding="utf-8")
    assert watch.last_tick(log) == []


def test_until_event_keeps_polling_while_silent_and_exits_at_the_first_event(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(pipelines, "load_defaults", lambda _path: {})
    monkeypatch.chdir(tmp_path)
    answers = iter([[], [{"kind": "held"}]])
    monkeypatch.setattr(watch, "report", lambda *_a, **_k: next(answers))
    slept: list[float] = []
    arguments = ["--queue", "q.json", "--defaults", "d.json", "--jobs", "jobs", "--until-event"]
    assert watch.main(arguments, sleep=slept.append) == 0
    assert slept == [60.0]


def test_each_finished_step_is_reported_once_with_its_place_and_status_shows_the_next(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    values = setup(tmp_path, monkeypatch, [{"recording": "7", "state": "running"}])
    steps = pipelines.layout_of("7", values["defaults"], tmp_path).steps
    steps.mkdir(parents=True)
    (steps / "audio-clock.json").write_text(
        json.dumps({"endedAt": 1.0, "wallSeconds": 9.0}), encoding="utf-8"
    )
    (steps / "unrelated.json").write_text("{}", encoding="utf-8")
    queue = tmp_path / "q.json"
    assert watch.report(queue, **values) == [
        {
            "key": "step-ok:7:audio-clock:1.0",
            "kind": "step-ok",
            "recording": "7",
            "step": "audio-clock",
            "done": "1/31",
            "wallSeconds": 9.0,
        }
    ]
    assert watch.report(queue, **values) == []
    capsys.readouterr()
    monkeypatch.setattr(pipelines, "load_defaults", lambda _path: values["defaults"])
    monkeypatch.chdir(tmp_path)
    running = [
        recording_progress.Progress("web-hls", 42, "read of a.mp4"),
        recording_progress.Progress("asr", -1, "no measure yet"),
    ]
    monkeypatch.setattr(recording_progress, "running_steps", lambda *_a, **_k: running)
    arguments = ["--queue", "q.json", "--defaults", "d.json", "--jobs", "jobs", "--status"]
    assert watch.main(arguments) == 0
    assert capsys.readouterr().out == (
        "7: running, 1/31 steps, last audio-clock, next clocks, yolo-track, separation\n"
        "  web-hls: 42% (read of a.mp4)\n"
        "  asr: ? (no measure yet)\n"
    )
