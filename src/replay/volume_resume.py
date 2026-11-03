"""Rerun a command after a watched volume detached under it.

``python -m replay.volume_resume --volume /Volumes/m.2-ssd -- COMMAND ARGS...`` runs the command
in its own process group and relays its output. When a watched volume disappears or is mounted
again (its ``st_dev`` changed) while the command runs, the group is interrupted (SIGINT, then
SIGTERM and SIGKILL after a grace period, so yt-dlp keeps its ``.part`` files). After a failure
caused by such a detach the supervisor waits, bounded, until the same volumes (same UUIDs,
``volume_events.await_return``) are back, lets them settle and reruns the command, at most
``--max-restarts`` times. The command must resume from its own evidence. ``--no-interrupt``
never signals the running command (one that recovers short detaches itself, like
``recording_pipeline run``) and only acts once it exits.

A failure while every watched volume stayed mounted and unchanged is returned as is: a blind
retry adds no evidence. Known states a rerun cannot resume are named with their recovery step;
the supervisor never removes evidence itself.
"""

from __future__ import annotations

import argparse
import json
import os
import select
import signal
import subprocess
import sys
import time
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from types import FrameType
from typing import IO

from replay import volume_events
from replay.core import ReplayError

POLL_SECONDS = 10.0
SETTLE_SECONDS = 30.0
GRACE_SECONDS = 60.0
# A write fails with EIO before macOS drops the mount, so a failure is watched a little longer.
RECHECK_SECONDS = 15.0
MAX_WAIT_SECONDS = 24 * 3600.0
MAX_RESTARTS = 5
TAIL_BYTES = 64 * 1024
CHUNK_BYTES = 64 * 1024
STOP_SIGNALS = (signal.SIGINT, signal.SIGTERM, signal.SIGKILL)
FORWARDED = (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)
PREFIX = "VOLUME_RESUME"
GUIDE = "docs/operations.md#resuming-after-the-ssd-detached"
HINTS: tuple[tuple[str, str], ...] = (
    (
        "Uncertain worker attempt",
        "a worker's attempt.json has no result: inspect its worker.log and redo that step"
        " (recording_queue redo QUEUE ID --from STEP); never delete the attempt to force a run",
    ),
    (
        "interrupted attempt cannot resume",
        "the stage refuses its interrupted partial state: redo it with"
        " recording_queue redo QUEUE ID --from STEP",
    ),
    (
        "code closure",
        "a pinned code file changed: restore it, or redo the step with run --force STEP",
    ),
    (
        "Compressor submission input changed",
        "a Compressor submission pin no longer matches its file (device numbers are already"
        " ignored, so the file itself moved or changed): reconcile the job, then redo"
        " the caption render step",
    ),
)


class Stopped(Exception):
    """The supervisor itself was asked to stop; the command is stopped and not rerun."""


@dataclass(frozen=True, slots=True)
class Options:
    volumes: tuple[Path, ...]
    max_wait: float = MAX_WAIT_SECONDS
    max_restarts: int = MAX_RESTARTS
    poll: float = POLL_SECONDS
    settle: float = SETTLE_SECONDS
    grace: float = GRACE_SECONDS
    recheck: float = RECHECK_SECONDS
    restart_compressor: bool = False
    # False: never signal a running command (it recovers short detaches itself); act on exit.
    interrupt: bool = True


@dataclass(frozen=True, slots=True)
class Probe:
    """Volume and clock access, replaced in tests."""

    device: Callable[[Path], int | None]
    current: Callable[[], volume_events.Snapshot] = volume_events.snapshot
    compressor_running: Callable[[], bool] = volume_events.compressor_running
    restart_compressor: Callable[[], None] = volume_events.restart_compressor
    sleep: Callable[[float], None] = time.sleep
    clock: Callable[[], float] = time.monotonic


@dataclass(frozen=True, slots=True)
class Attempt:
    code: int
    tail: bytes
    detached: bool
    stopped: bool = False


def mounted_device(path: Path) -> int | None:
    """The device of the volume mounted at ``path``; None when nothing is mounted there."""
    try:
        return os.stat(path).st_dev if os.path.ismount(path) else None
    except OSError:
        return None


def require_mounted(path: Path) -> None:
    """Refuse a path under an unmounted ``/Volumes/<name>``: writes would land on the boot disk."""
    parts = path.absolute().parts
    if len(parts) > 2 and Path(*parts[:2]) == volume_events.VOLUMES:
        mount = Path(*parts[:3])
        if not os.path.ismount(mount):
            raise ReplayError(f"{mount} is not mounted; refusing to use {path}")


def log(event: str, **fields: object) -> None:
    stamp = datetime.now(UTC).isoformat(timespec="seconds")
    print(f"{PREFIX} {json.dumps({'event': event, **fields, 'at': stamp})}", file=sys.stderr)
    sys.stderr.flush()


def moved(baseline: volume_events.Snapshot, probe: Probe) -> bool:
    return any(probe.device(Path(m)) != volume.device for m, volume in baseline.items())


def await_mounted(options: Options, probe: Probe) -> volume_events.Snapshot:
    """The watched volumes (with UUIDs) once all are mounted; waits at most ``max_wait``."""
    deadline = probe.clock() + options.max_wait
    waited = False
    while True:
        now = probe.current()
        found = {str(v): now[str(v)] for v in options.volumes if str(v) in now}
        if len(found) == len(options.volumes):
            if waited:
                log("volumes-mounted", volumes=sorted(found))
            return found
        if probe.clock() >= deadline:
            raise ReplayError(f"Watched volumes did not mount within {options.max_wait:.0f} s")
        if not waited:
            log("waiting-for-volumes", missing=sorted(set(map(str, options.volumes)) - set(found)))
            waited = True
        probe.sleep(options.poll)


def relay(stream: IO[bytes], tail: bytearray, timeout: float) -> bytes | None:
    """Copy available output to stdout and the tail; None when none is ready, b"" at its end."""
    ready, _, _ = select.select([stream], [], [], timeout)
    if not ready:
        return None
    data = os.read(stream.fileno(), CHUNK_BYTES)
    sys.stdout.buffer.write(data)
    sys.stdout.flush()
    tail.extend(data)
    del tail[:-TAIL_BYTES]
    return data


def follow(
    child: subprocess.Popen[bytes],
    tail: bytearray,
    *,
    seconds: float,
    check: Callable[[], bool],
    probe: Probe,
) -> bool:
    """Relay output until the child exits (False) or, every ``seconds``, ``check`` holds (True)."""
    stream = child.stdout
    if stream is None:
        raise ReplayError("The command has no output pipe")
    open_output = True
    next_check = probe.clock() + seconds
    while child.poll() is None:
        if open_output:
            open_output = relay(stream, tail, min(seconds, 1.0)) != b""
        else:
            probe.sleep(min(seconds, 1.0))
        if probe.clock() >= next_check:
            if child.poll() is None and check():
                return True
            next_check = probe.clock() + seconds
    while open_output and relay(stream, tail, 0):
        continue
    return False


def elapsed(clock: Callable[[], float], deadline: float) -> Callable[[], bool]:
    return lambda: clock() >= deadline


def stop(child: subprocess.Popen[bytes], tail: bytearray, options: Options, probe: Probe) -> int:
    """Interrupt the command's process group, escalating after each grace period."""
    for number in STOP_SIGNALS:
        try:
            os.killpg(child.pid, number)
        except ProcessLookupError:
            break
        if not follow(
            child,
            tail,
            seconds=min(options.poll, options.grace),
            check=elapsed(probe.clock, probe.clock() + options.grace),
            probe=probe,
        ):
            break
    return exit_code(child.wait())


def exit_code(returncode: int) -> int:
    return 128 - returncode if returncode < 0 else returncode


def run_once(
    command: Sequence[str],
    baseline: volume_events.Snapshot,
    options: Options,
    probe: Probe,
) -> Attempt:
    tail = bytearray()
    with subprocess.Popen(
        list(command), stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True
    ) as child:
        try:
            if follow(
                child,
                tail,
                seconds=options.poll,
                check=lambda: options.interrupt and moved(baseline, probe),
                probe=probe,
            ):
                log("volume-detached-while-running", pid=child.pid)
                return Attempt(stop(child, tail, options, probe), bytes(tail), detached=True)
        except Stopped:
            return Attempt(stop(child, tail, options, probe), bytes(tail), False, stopped=True)
        return Attempt(exit_code(child.wait()), bytes(tail), detached=False)


def detached_soon(baseline: volume_events.Snapshot, options: Options, probe: Probe) -> bool:
    deadline = probe.clock() + options.recheck
    while not moved(baseline, probe):
        if probe.clock() >= deadline:
            return False
        probe.sleep(min(options.poll, 1.0))
    return True


def explain(tail: bytes) -> None:
    text = tail.decode("utf-8", errors="replace")
    hints = [hint for needle, hint in HINTS if needle in text]
    log("failed-with-volumes-present", hints=hints, guide=GUIDE)


def recover(baseline: volume_events.Snapshot, options: Options, probe: Probe) -> None:
    """Wait for the same volumes, restart Compressor if asked (it holds the old mount), settle."""
    log("waiting-for-return", volumes=sorted(baseline))
    reason = volume_events.await_return(
        baseline, current=probe.current, sleep=probe.sleep, clock=probe.clock, wait=options.max_wait
    )
    if reason is not None:
        raise ReplayError(f"Not rerunning: {reason}")
    if options.restart_compressor and probe.compressor_running():
        log("restarting-compressor")
        probe.restart_compressor()
    probe.sleep(options.settle)


def supervise(command: Sequence[str], options: Options, probe: Probe) -> int:
    restarts = 0
    while True:
        baseline = await_mounted(options, probe)
        attempt = run_once(command, baseline, options, probe)
        if attempt.code == 0 or attempt.stopped:
            return attempt.code
        if not attempt.detached and not detached_soon(baseline, options, probe):
            explain(attempt.tail)
            return attempt.code
        if restarts >= options.max_restarts:
            log("restart-limit", exit=attempt.code, restarts=restarts)
            return attempt.code
        recover(baseline, options, probe)
        restarts += 1
        log("restart", restart=restarts, exit=attempt.code, command=command[0])


def interrupt(_number: int, _frame: FrameType | None) -> None:
    raise Stopped


def parse(argv: Sequence[str]) -> tuple[Options, list[str]]:
    if "--" not in argv:
        raise SystemExit("usage: python -m replay.volume_resume --volume PATH [...] -- COMMAND")
    split = list(argv).index("--")
    parser = argparse.ArgumentParser(prog="python -m replay.volume_resume")
    parser.add_argument("--volume", action="append", required=True, type=Path)
    parser.add_argument("--max-wait", type=float, default=MAX_WAIT_SECONDS)
    parser.add_argument("--max-restarts", type=int, default=MAX_RESTARTS)
    parser.add_argument("--settle", type=float, default=SETTLE_SECONDS)
    parser.add_argument("--restart-compressor", action="store_true")
    parser.add_argument("--no-interrupt", dest="interrupt", action="store_false")
    args = parser.parse_args(argv[:split])
    command = list(argv[split + 1 :])
    if not command:
        parser.error("a command is required after --")
    for volume in args.volume:
        if volume.parent != volume_events.VOLUMES:
            parser.error(f"{volume} is not a volume under {volume_events.VOLUMES}")
    options = Options(
        volumes=tuple(args.volume),
        max_wait=args.max_wait,
        max_restarts=args.max_restarts,
        settle=args.settle,
        restart_compressor=args.restart_compressor,
        interrupt=args.interrupt,
    )
    return options, command


def main(argv: Sequence[str] | None = None) -> int:
    options, command = parse(sys.argv[1:] if argv is None else argv)
    for number in FORWARDED:
        signal.signal(number, interrupt)
    try:
        return supervise(command, options, Probe(device=mounted_device))
    except ReplayError as error:
        log("stopped", reason=str(error))
        return 1
    except Stopped:
        log("stopped", reason="signal")
        return 128 + signal.SIGTERM


if __name__ == "__main__":
    raise SystemExit(main())
