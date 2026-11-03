import os
import signal
import subprocess
import sys
import time
from pathlib import Path

import pytest

from replay import volume_resume
from replay.core import ReplayError
from replay.volume_events import Snapshot, Volume
from replay.volume_resume import (
    Options,
    Probe,
    Stopped,
    interrupt,
    main,
    mounted_device,
    parse,
    require_mounted,
    stop,
    supervise,
)

SSD = Path("/Volumes/ssd")
FAST = Options(volumes=(SSD,), poll=0.02, settle=0.0, grace=0.3, recheck=0.1, max_wait=5.0)


class Disk:
    """A fake volume driven by files, so the child process can unplug it."""

    def __init__(self, root: Path, *, uuid: str = "U1") -> None:
        self.unplugged = root / "unplugged"
        self.device_file = root / "device"
        self.device_file.write_text("7", encoding="utf-8")
        self.uuid = uuid
        self.sleeps: list[float] = []
        self.restarts = 0

    def device(self, path: Path) -> int | None:
        if self.unplugged.exists():
            return None
        return int(self.device_file.read_text(encoding="utf-8"))

    def current(self) -> Snapshot:
        number = self.device(SSD)
        return {} if number is None else {str(SSD): Volume(self.uuid, number)}

    def sleep(self, seconds: float) -> None:
        # Plugging back in renumbers the device, as macOS does.
        self.sleeps.append(seconds)
        if self.unplugged.exists():
            self.unplugged.unlink()
            self.device_file.write_text("8", encoding="utf-8")
        time.sleep(min(seconds, 0.01))

    def restart(self) -> None:
        self.restarts += 1

    def probe(self, *, running: bool = False) -> Probe:
        return Probe(
            device=self.device,
            current=self.current,
            compressor_running=lambda: running,
            restart_compressor=self.restart,
            sleep=self.sleep,
        )


def _python(code: str) -> list[str]:
    return [sys.executable, "-c", code]


def test_a_successful_command_runs_once_and_its_output_is_relayed(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    disk = Disk(tmp_path)
    assert supervise(_python("print('done')"), FAST, disk.probe()) == 0
    assert capsys.readouterr().out == "done\n"


def test_a_failure_with_the_volume_present_is_returned_with_its_recovery_hint(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    disk = Disk(tmp_path)
    command = _python("print('Uncertain worker attempt; reconcile'); raise SystemExit(3)")
    assert supervise(command, FAST, disk.probe()) == 3
    captured = capsys.readouterr()
    assert captured.out == "Uncertain worker attempt; reconcile\n"
    assert '"event": "failed-with-volumes-present"' in captured.err
    assert (
        '"hints": ["a worker\'s attempt.json has no result: inspect its worker.log and redo that'
        " step (recording_queue redo QUEUE ID --from STEP); never delete the attempt to force a"
        ' run"]'
    ) in captured.err
    assert '"event": "restart"' not in captured.err


def test_a_failure_from_an_unplug_waits_for_the_volume_and_reruns(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    disk = Disk(tmp_path)
    runs = tmp_path / "runs"
    command = _python(
        "import pathlib, sys\n"
        f"runs = pathlib.Path({str(runs)!r})\n"
        "runs.write_text(runs.read_text() + 'x' if runs.exists() else 'x')\n"
        "if runs.read_text() == 'x':\n"
        f"    pathlib.Path({str(disk.unplugged)!r}).touch()\n"
        "    sys.exit(1)\n"
    )
    assert supervise(command, FAST, disk.probe()) == 0
    assert runs.read_text(encoding="utf-8") == "xx"
    err = capsys.readouterr().err
    assert '"event": "waiting-for-return", "volumes": ["/Volumes/ssd"]' in err
    assert '"event": "restart"' in err
    assert '"restart": 1, "exit": 1' in err
    assert disk.device(SSD) == 8


def test_an_unplug_while_running_interrupts_the_group_and_reruns(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    disk = Disk(tmp_path)
    runs = tmp_path / "runs"
    command = _python(
        "import pathlib, signal, sys, time\n"
        f"runs = pathlib.Path({str(runs)!r})\n"
        "runs.write_text(runs.read_text() + 'x' if runs.exists() else 'x')\n"
        "if runs.read_text() == 'xx':\n"
        "    sys.exit(0)\n"
        "signal.signal(signal.SIGINT, lambda *_: (print('interrupted', flush=True), sys.exit(9)))\n"
        f"pathlib.Path({str(disk.unplugged)!r}).touch()\n"
        "time.sleep(30)\n"
    )
    assert supervise(command, FAST, disk.probe(running=True)) == 0
    captured = capsys.readouterr()
    assert captured.out == "interrupted\n"
    assert '"event": "volume-detached-while-running"' in captured.err
    assert '"restart": 1, "exit": 9' in captured.err
    assert disk.restarts == 0


def test_a_child_ignoring_interrupts_is_killed_and_the_restart_limit_holds(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    disk = Disk(tmp_path)
    command = _python(
        "import pathlib, signal, time\n"
        "signal.signal(signal.SIGINT, signal.SIG_IGN)\n"
        "signal.signal(signal.SIGTERM, signal.SIG_IGN)\n"
        f"pathlib.Path({str(disk.unplugged)!r}).touch()\n"
        "time.sleep(30)\n"
    )
    options = Options(volumes=(SSD,), poll=0.02, grace=0.2, recheck=0.1, max_restarts=0)
    assert supervise(command, options, disk.probe()) == 137
    assert '"event": "restart-limit", "exit": 137, "restarts": 0' in capsys.readouterr().err


def test_compressor_is_restarted_after_a_detach_when_asked(tmp_path: Path) -> None:
    disk = Disk(tmp_path)
    runs = tmp_path / "runs"
    command = _python(
        "import pathlib, sys\n"
        f"runs = pathlib.Path({str(runs)!r})\n"
        "runs.write_text(runs.read_text() + 'x' if runs.exists() else 'x')\n"
        "if runs.read_text() == 'x':\n"
        f"    pathlib.Path({str(disk.unplugged)!r}).touch()\n"
        "    sys.exit(1)\n"
    )
    options = Options(volumes=(SSD,), poll=0.02, settle=0.0, recheck=0.1, restart_compressor=True)
    assert supervise(command, options, disk.probe(running=True)) == 0
    assert disk.restarts == 1


def test_a_different_volume_at_the_mount_point_is_not_rerun(tmp_path: Path) -> None:
    disk = Disk(tmp_path)
    command = _python(
        "import pathlib, sys\n"
        f"pathlib.Path({str(tmp_path / 'device')!r}).write_text('9')\n"
        "sys.exit(1)\n"
    )
    probe = disk.probe()
    swapped = Probe(
        device=probe.device,
        current=lambda: {str(SSD): Volume("U2" if disk.device(SSD) == 9 else "U1", 7)},
        sleep=probe.sleep,
    )
    with pytest.raises(ReplayError, match="Not rerunning: /Volumes/ssd is a different volume"):
        supervise(command, FAST, swapped)


def test_a_missing_volume_is_awaited_before_the_first_run(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    disk = Disk(tmp_path)
    disk.unplugged.touch()
    assert supervise(_python("pass"), FAST, disk.probe()) == 0
    err = capsys.readouterr().err
    assert '"event": "waiting-for-volumes", "missing": ["/Volumes/ssd"]' in err
    assert '"event": "volumes-mounted", "volumes": ["/Volumes/ssd"]' in err


def test_a_volume_that_never_mounts_stops_the_supervisor(tmp_path: Path) -> None:
    disk = Disk(tmp_path)
    disk.unplugged.touch()
    probe = Probe(device=disk.device, current=dict, sleep=disk.sleep)
    options = Options(volumes=(SSD,), max_wait=0.0)
    with pytest.raises(ReplayError, match="Watched volumes did not mount within 0 s"):
        supervise(_python("pass"), options, probe)


def test_a_stop_request_stops_the_command_without_a_rerun(tmp_path: Path) -> None:
    disk = Disk(tmp_path)

    def device(path: Path) -> int | None:
        raise Stopped

    probe = Probe(device=device, current=disk.current, sleep=disk.sleep)
    assert supervise(_python("import time; time.sleep(30)"), FAST, probe) == 130


def test_a_command_closing_its_output_is_still_followed(tmp_path: Path) -> None:
    disk = Disk(tmp_path)
    command = _python("import os, time; os.close(1); os.close(2); time.sleep(0.2)")
    assert supervise(command, FAST, disk.probe()) == 0


def test_stopping_a_finished_group_returns_its_code(tmp_path: Path) -> None:
    disk = Disk(tmp_path)
    with subprocess.Popen(
        _python("raise SystemExit(4)"), stdout=subprocess.PIPE, start_new_session=True
    ) as child:
        child.wait()
        assert stop(child, bytearray(), FAST, disk.probe()) == 4


def test_a_command_without_an_output_pipe_is_refused(tmp_path: Path) -> None:
    disk = Disk(tmp_path)
    with subprocess.Popen(_python("pass")) as child, pytest.raises(ReplayError, match="pipe"):
        volume_resume.follow(
            child, bytearray(), seconds=0.01, check=lambda: False, probe=disk.probe()
        )


def test_interrupt_raises_stopped() -> None:
    with pytest.raises(Stopped):
        interrupt(signal.SIGTERM, None)


def test_mounted_device(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    assert mounted_device(tmp_path) is None
    assert isinstance(mounted_device(Path("/")), int)
    monkeypatch.setattr(os.path, "ismount", lambda path: True)
    assert mounted_device(tmp_path / "gone") is None


def test_require_mounted(tmp_path: Path) -> None:
    require_mounted(tmp_path)
    require_mounted(Path("/Volumes"))
    with pytest.raises(ReplayError, match="/Volumes/no-such-volume-x is not mounted"):
        require_mounted(Path("/Volumes/no-such-volume-x/movie/youtube"))


def test_parse() -> None:
    options, command = parse(
        [
            "--volume",
            "/Volumes/ssd",
            "--max-wait",
            "60",
            "--max-restarts",
            "2",
            "--settle",
            "5",
            "--restart-compressor",
            "--no-interrupt",
            "--",
            "echo",
            "--volume",
        ]
    )
    assert options == Options(
        volumes=(SSD,),
        max_wait=60.0,
        max_restarts=2,
        settle=5.0,
        restart_compressor=True,
        interrupt=False,
    )
    assert command == ["echo", "--volume"]


def test_parse_refusals(capsys: pytest.CaptureFixture[str]) -> None:
    with pytest.raises(SystemExit, match="usage"):
        parse(["--volume", "/Volumes/ssd"])
    with pytest.raises(SystemExit):
        parse(["--volume", "/Volumes/ssd", "--"])
    with pytest.raises(SystemExit):
        parse(["--volume", "/Users/x", "--", "true"])
    err = capsys.readouterr().err
    assert "a command is required after --" in err
    assert "/Users/x is not a volume under /Volumes" in err


def test_main(monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]) -> None:
    handled: list[int] = []
    monkeypatch.setattr(signal, "signal", lambda number, handler: handled.append(number))
    outcomes: list[BaseException | int] = [ReplayError("gone"), Stopped(), 0]

    def fake(command: list[str], options: Options, probe: Probe) -> int:
        outcome = outcomes.pop(0)
        if isinstance(outcome, BaseException):
            raise outcome
        return outcome

    monkeypatch.setattr(volume_resume, "supervise", fake)
    monkeypatch.setattr(sys, "argv", ["volume_resume", "--volume", "/Volumes/ssd", "--", "true"])
    assert main() == 1
    assert main(["--volume", "/Volumes/ssd", "--", "true"]) == 143
    assert main(["--volume", "/Volumes/ssd", "--", "true"]) == 0
    assert handled == [2, 15, 1, 2, 15, 1, 2, 15, 1]
    err = capsys.readouterr().err
    assert '"event": "stopped"' in err
    assert '"reason": "gone"' in err
    assert '"reason": "signal"' in err


def test_no_interrupt_lets_the_command_recover_a_detach_itself(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    disk = Disk(tmp_path)
    command = _python(
        "import pathlib, signal, sys, time\n"
        "signal.signal(signal.SIGINT, lambda *_: (print('interrupted', flush=True), sys.exit(9)))\n"
        f"pathlib.Path({str(disk.unplugged)!r}).touch()\n"
        "time.sleep(0.3)\n"
        f"pathlib.Path({str(disk.unplugged)!r}).unlink()\n"
        "print('recovered', flush=True)\n"
    )
    options = Options(volumes=(SSD,), poll=0.02, recheck=0.1, interrupt=False)
    assert supervise(command, options, disk.probe()) == 0
    captured = capsys.readouterr()
    assert captured.out == "recovered\n"
    assert captured.err == ""
