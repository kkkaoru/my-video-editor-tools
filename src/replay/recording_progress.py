"""How far a running recording's steps are, measured on demand (``recording_watch --status``).

Each running step reports the best measure it has, always named so a guess never reads as a fact:

- ``chunks``: a cleanup's accepted chunks of all its chunk configurations;
- ``read``: how far its processes read their largest media input, from the kernel's file offset
  (``proc_pidfdinfo``; ``lsof`` on macOS prints the size there). Backward seeks make it dip;
- ``estimate``: time since the step started against the median of the same step's finished runs,
  unscaled by the recording's length and capped at 99%: only a rough guide.

A process belongs to the step whose ``*.run.log`` it or an ancestor holds open; one without any is
the pipeline's own preparation (``prepare``, such as the decodable copy of a VP9 source).
"""

import ctypes
import json
import os
import re
import statistics
import struct
import subprocess
import time
from collections.abc import Iterable, Iterator
from dataclasses import dataclass
from pathlib import Path

from replay.recording_steps import recording_name

LISTFDS = 1
VNODE_PATH_INFO = 2
VNODE_FD = 1
FD_ENTRY = struct.Struct("iI")
FILE_HEAD = struct.Struct("I4xq")  # proc_fileinfo: open flags, status (unread), offset
VNODE_INFO_BYTES = 152
FILE_HEAD_BYTES = 24
PATH_INFO_BYTES = 1200
WRITABLE = 0x2  # FWRITE
MEDIA = frozenset({".mp4", ".mov", ".m4a", ".m4s", ".mkv", ".webm", ".wav", ".caf", ".aac"})
PREPARE = "prepare"
RUN_LOG = re.compile(r"^(?P<step>.+)-(?P<started>\d+)\.run\.log$")
CAP = 99
_LIBPROC = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)


@dataclass(frozen=True, slots=True)
class OpenFile:
    path: Path
    offset: int
    writable: bool


@dataclass(frozen=True, slots=True)
class Progress:
    step: str
    percent: int
    basis: str


def open_files(pid: int) -> list[OpenFile]:
    """The process's open regular files; empty once it has gone."""
    size = _LIBPROC.proc_pidinfo(pid, LISTFDS, 0, None, 0)
    if size <= 0:
        return []
    listing = ctypes.create_string_buffer(size)
    size = _LIBPROC.proc_pidinfo(pid, LISTFDS, 0, listing, size)
    found: list[OpenFile] = []
    for index in range(max(size, 0) // FD_ENTRY.size):
        descriptor, kind = FD_ENTRY.unpack_from(listing, index * FD_ENTRY.size)
        if kind != VNODE_FD:
            continue
        info = ctypes.create_string_buffer(PATH_INFO_BYTES)
        if (
            _LIBPROC.proc_pidfdinfo(pid, descriptor, VNODE_PATH_INFO, info, PATH_INFO_BYTES)
            < PATH_INFO_BYTES
        ):
            continue
        flags, offset = FILE_HEAD.unpack_from(info)
        path = info.raw[FILE_HEAD_BYTES + VNODE_INFO_BYTES :].split(b"\0", 1)[0]
        found.append(OpenFile(Path(os.fsdecode(path)), offset, bool(flags & WRITABLE)))
    return found


def process_table() -> dict[int, tuple[int, str]]:
    """Every process: its parent and command."""
    listed = subprocess.run(
        ["ps", "-Ao", "pid=,ppid=,command="], capture_output=True, text=True, check=True
    )
    table: dict[int, tuple[int, str]] = {}
    for line in listed.stdout.splitlines():
        pid, parent, command = line.split(None, 2)
        table[int(pid)] = (int(parent), command)
    return table


def pipeline_processes(replay_id: str, table: dict[int, tuple[int, str]]) -> Iterator[int]:
    """The recording's pipeline driver's descendants, parents first."""
    needle = f"recording_pipeline run {replay_id} "
    frontier = [pid for pid, row in table.items() if needle in f"{row[1]} "]
    while frontier:
        pid = frontier.pop(0)
        children = [child for child, row in table.items() if row[0] == pid]
        yield from children
        frontier += children


def step_files(
    pids: Iterable[int], table: dict[int, tuple[int, str]]
) -> dict[str, tuple[int, list[OpenFile]]]:
    """Each running step's start (ns, 0 for ``prepare``) and its processes' open files."""
    owners: dict[int, tuple[str, int]] = {}
    steps: dict[str, tuple[int, list[OpenFile]]] = {}
    for pid in pids:
        files = open_files(pid)
        logs = (RUN_LOG.match(item.path.name) for item in files)
        match = next((found for found in logs if found is not None), None)
        owner = (
            (match["step"], int(match["started"]))
            if match is not None
            else owners.get(table[pid][0], (PREPARE, 0))
        )
        owners[pid] = owner
        steps.setdefault(owner[0], (owner[1], []))[1].extend(files)
    return steps


def read_position(files: Iterable[OpenFile]) -> Progress | None:
    """The furthest read of the largest media input, or None without a readable one."""
    furthest: dict[Path, int] = {}
    for item in files:
        if not item.writable and item.path.suffix.lower() in MEDIA:
            furthest[item.path] = max(item.offset, furthest.get(item.path, 0))
    sized = [(path.stat().st_size, path) for path in furthest if path.is_file()]
    sized = [entry for entry in sized if entry[0] > 0]
    if not sized:
        return None
    size, path = max(sized)
    return Progress("", min(100, furthest[path] * 100 // size), f"read of {path.name}")


def chunks(work: Path, recording: str) -> Progress | None:
    """This recording's accepted chunks of its chunk configurations (None before it has any)."""
    total = len(list(work.glob(f"clean-{recording}-*/configs/chunk-*")))
    if total == 0:
        return None
    done = len(list(work.glob(f"clean-{recording}-*/state/chunk-*/chunk-receipt.json")))
    return Progress("", min(100, done * 100 // total), f"chunks {done}/{total}")


def typical_seconds(recordings: Path, step: str) -> float | None:
    """The median wall time of the step's finished runs in every recording."""
    walls: list[float] = []
    for receipt in recordings.glob(f"*/steps/{step}.json"):
        try:
            wall = json.loads(receipt.read_text(encoding="utf-8")).get("wallSeconds")
        except (OSError, ValueError):
            continue
        if isinstance(wall, (int, float)) and wall > 0:
            walls.append(float(wall))
    return statistics.median(walls) if walls else None


def estimate(step: str, *, started_ns: int, recordings: Path, now: float) -> Progress | None:
    typical = typical_seconds(recordings, step)
    if typical is None or started_ns == 0:
        return None
    elapsed = max(0.0, now - started_ns / 1e9)
    return Progress(
        step,
        min(CAP, int(elapsed * 100 / typical)),
        f"estimate: {elapsed / 60:.0f} min of a typical {typical / 60:.0f} min",
    )


def running_steps(
    replay_id: str, *, edit_work: Path, recordings: Path, now: float | None = None
) -> list[Progress]:
    """The recording's running steps and how far each is; a step without any measure says so."""
    table = process_table()
    steps = step_files(pipeline_processes(replay_id, table), table)
    found: list[Progress] = []
    for step, (started, files) in sorted(steps.items()):
        measured = None
        if step == "cleanup":
            measured = chunks(edit_work, recording_name(replay_id))
        measured = measured or read_position(files)
        if measured is None:
            measured = estimate(
                step,
                started_ns=started,
                recordings=recordings,
                now=time.time() if now is None else now,
            )
        found.append(
            Progress(step, measured.percent, measured.basis)
            if measured is not None
            else Progress(step, -1, "no measure yet")
        )
    return found
