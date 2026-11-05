"""Running steps' progress: real kernel offsets, named measures, estimates that say so."""

import os
import subprocess
import sys
from pathlib import Path

import pytest

from replay import recording_progress as progress


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def test_open_files_reports_the_real_offset_and_mode(tmp_path: Path) -> None:
    movie = tmp_path / "source.mp4"
    movie.write_bytes(b"x" * 1000)
    with movie.open("rb") as reading:
        reading.seek(250)
        found = {item.path.name: item for item in progress.open_files(os.getpid())}
        assert (found["source.mp4"].offset, found["source.mp4"].writable) == (250, False)
    with movie.open("ab") as writing:
        writing.write(b"y")
        writing.flush()
        found = {item.path.name: item for item in progress.open_files(os.getpid())}
        assert found["source.mp4"].writable is True
    assert progress.open_files(2**30) == []


def test_read_position_uses_the_largest_input_and_ignores_outputs(tmp_path: Path) -> None:
    large, small, output = tmp_path / "a.mp4", tmp_path / "b.wav", tmp_path / "out.mov"
    large.write_bytes(b"x" * 1000)
    small.write_bytes(b"x" * 10)
    output.write_bytes(b"x" * 5000)
    measured = progress.read_position(
        [
            progress.OpenFile(large, 100, False),
            progress.OpenFile(large, 420, False),
            progress.OpenFile(small, 10, False),
            progress.OpenFile(output, 5000, True),
            progress.OpenFile(tmp_path / "model.bin", 5, False),
        ]
    )
    assert measured == progress.Progress("", 42, "read of a.mp4")
    assert progress.read_position([progress.OpenFile(output, 1, True)]) is None


def test_chunks_count_accepted_ones_of_all_configurations(tmp_path: Path) -> None:
    assert progress.chunks(tmp_path, "youtube-x") is None
    write(tmp_path / "clean-youtube-x-p1/configs/chunk-0.json", "")
    write(tmp_path / "clean-youtube-x-p1/configs/chunk-1.json", "")
    write(tmp_path / "clean-youtube-x-p1/configs/chunk-2.json", "")
    write(tmp_path / "clean-youtube-x-p1/configs/chunk-3.json", "")
    write(tmp_path / "clean-youtube-other-p1/configs/chunk-0.json", "")
    write(tmp_path / "clean-youtube-x-p1/state/chunk-0/chunk-receipt.json", "")
    assert progress.chunks(tmp_path, "youtube-x") == progress.Progress("", 25, "chunks 1/4")


def test_an_estimate_is_the_median_capped_and_needs_a_history(tmp_path: Path) -> None:
    write(tmp_path / "a/steps/asr.json", '{"wallSeconds": 100.0}')
    write(tmp_path / "b/steps/asr.json", '{"wallSeconds": 300.0}')
    write(tmp_path / "c/steps/asr.json", '{"wallSeconds": 200.0}')
    write(tmp_path / "d/steps/asr.json", "{")
    assert progress.typical_seconds(tmp_path, "asr") == 200.0
    assert progress.estimate("asr", started_ns=0, recordings=tmp_path, now=1000.0) is None
    assert progress.estimate("none", started_ns=1, recordings=tmp_path, now=1000.0) is None
    half = progress.estimate("asr", started_ns=int(900e9), recordings=tmp_path, now=1000.0)
    assert half == progress.Progress("asr", 50, "estimate: 2 min of a typical 3 min")
    late = progress.estimate("asr", started_ns=int(1e9), recordings=tmp_path, now=10000.0)
    assert late == progress.Progress("asr", 99, "estimate: 167 min of a typical 3 min")


def test_processes_belong_to_the_step_whose_run_log_an_ancestor_holds(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    table = {
        1: (0, "python recording_pipeline run abc --defaults d"),
        2: (1, "ffmpeg prepare"),
        3: (1, "runner"),
        4: (3, "ffmpeg child"),
        9: (0, "unrelated recording_pipeline run abcd --defaults d"),
    }
    files = {
        2: [progress.OpenFile(tmp_path / "in.mp4", 1, False)],
        3: [progress.OpenFile(tmp_path / "web-hls-123.run.log", 0, True)],
        4: [progress.OpenFile(tmp_path / "in.mp4", 2, False)],
    }
    pids = list(progress.pipeline_processes("abc", table))
    assert pids == [2, 3, 4]
    monkeypatch.setattr(progress, "open_files", lambda pid: files[pid])
    assert progress.step_files(pids, table) == {
        "prepare": (0, [progress.OpenFile(tmp_path / "in.mp4", 1, False)]),
        "web-hls": (
            123,
            [
                progress.OpenFile(tmp_path / "web-hls-123.run.log", 0, True),
                progress.OpenFile(tmp_path / "in.mp4", 2, False),
            ],
        ),
    }


def test_running_steps_choose_the_best_measure_and_admit_having_none(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    movie = tmp_path / "in.mp4"
    movie.write_bytes(b"x" * 100)
    write(tmp_path / "clean-replay-1-p1/configs/chunk-0.json", "")
    write(tmp_path / "r/a/steps/asr.json", '{"wallSeconds": 100.0}')
    reader = [progress.OpenFile(movie, 30, False)]
    steps = {
        "cleanup": (5, reader),
        "compose": (5, reader),
        "asr": (int(50e9), []),
        "summary": (5, []),
    }
    monkeypatch.setattr(progress, "process_table", dict)
    monkeypatch.setattr(progress, "step_files", lambda _pids, _table: steps)
    assert progress.running_steps(
        "1", edit_work=tmp_path, recordings=tmp_path / "r", now=100.0
    ) == [
        progress.Progress("asr", 50, "estimate: 1 min of a typical 2 min"),
        progress.Progress("cleanup", 0, "chunks 0/1"),
        progress.Progress("compose", 30, "read of in.mp4"),
        progress.Progress("summary", -1, "no measure yet"),
    ]


def test_the_process_table_lists_this_process() -> None:
    table = progress.process_table()
    assert table[os.getpid()][0] == os.getppid()
    child = subprocess.Popen([sys.executable, "-c", "input()"], stdin=subprocess.PIPE)
    try:
        assert progress.process_table()[child.pid][0] == os.getpid()
    finally:
        child.communicate(b"\n")
