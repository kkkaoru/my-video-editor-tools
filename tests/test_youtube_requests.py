"""The one entry point restarts a stopped runner and starts only what is not running."""

import shutil
import subprocess
from pathlib import Path

import pytest

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"


def tool(path: Path, body: str) -> None:
    path.write_text(f"#!/bin/bash\n{body}\n", encoding="utf-8")
    path.chmod(0o700)


@pytest.fixture
def project(tmp_path: Path) -> Path:
    root = tmp_path / "repository"
    (root / "scripts").mkdir(parents=True)
    shutil.copyfile(SCRIPTS / "youtube-requests.sh", root / "scripts/youtube-requests.sh")
    shutil.copyfile(SCRIPTS / "tmux-job.sh", root / "scripts/tmux-job.sh")
    (root / "edits/work/recording-pipeline").mkdir(parents=True)
    tools = root / ".venv/bin"
    tools.mkdir(parents=True)
    # Preflights pass; every job's command is recorded instead of run.
    tool(tools / "python", '[[ "$2" == replay.disk_budget ]] || printf "%s\\n" "$*" >> calls')
    tool(tools / "pgrep", "exit 1")
    tool(tools / "brew", "echo /ffmpeg-full")
    # Runs the job in place, like the fake in test_tmux_job.py.
    tool(
        tools / "tmux",
        'if [[ "$5" == has-session ]]; then exit 1; fi\n'
        'while [[ "$1" != /bin/bash ]]; do shift; done\n'
        'exec "$@"',
    )
    return root


def request(project: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["/bin/bash", "scripts/youtube-requests.sh", "https://www.youtube.com/watch?v=abcdefghijk"],
        cwd=project,
        capture_output=True,
        text=True,
        timeout=10,
        check=False,
    )


def test_a_stopped_runner_restarts_over_its_previous_job_files(project: Path) -> None:
    jobs = project / "edits/work/youtube-intake"
    jobs.mkdir()
    (jobs / "queue-runner.launch").mkdir()
    (jobs / "queue-runner.log").write_text("previous run\n", encoding="utf-8")
    (jobs / "queue-runner.status").write_text("0\n", encoding="utf-8")
    result = request(project)
    assert (result.returncode, result.stderr) == (0, "")
    rotated = [path.read_text(encoding="utf-8") for path in jobs.glob("queue-runner-*.old")]
    assert rotated == ["previous run\n"]
    assert (jobs / "queue-runner.log").read_text(encoding="utf-8") == ""
    assert (project / "calls").read_text(encoding="utf-8").splitlines() == [
        "-m replay.recording_queue run"
        " --queue edits/work/recording-pipeline/requested-youtube-queue.json"
        " --defaults edits/work/recording-pipeline/DEFAULTS-v1.json",
        "-m replay.disk_relief --apply --watch",
        "-m replay.volume_resume --volume /Volumes/m.2-ssd -- .venv/bin/python"
        " -m replay.youtube_intake"
        " --queue edits/work/recording-pipeline/requested-youtube-queue.json"
        " --root /Volumes/m.2-ssd/movie/youtube --ffmpeg-directory /ffmpeg-full/bin"
        " https://www.youtube.com/watch?v=abcdefghijk",
    ]


def test_a_ticking_runner_and_relief_are_left_alone(project: Path) -> None:
    tool(project / ".venv/bin/pgrep", "exit 0")
    result = request(project)
    assert (result.returncode, result.stderr) == (0, "")
    assert result.stdout.splitlines()[0] == "runner already ticking"
    assert (project / "calls").read_text(encoding="utf-8").splitlines() == [
        "-m replay.volume_resume --volume /Volumes/m.2-ssd -- .venv/bin/python"
        " -m replay.youtube_intake"
        " --queue edits/work/recording-pipeline/requested-youtube-queue.json"
        " --root /Volumes/m.2-ssd/movie/youtube --ffmpeg-directory /ffmpeg-full/bin"
        " https://www.youtube.com/watch?v=abcdefghijk",
    ]
