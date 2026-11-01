"""One line per pipeline event, for an event-driven watcher instead of agents polling status.

Each pass compares the files the queue already writes with the previous pass and prints only
what changed: a new failure record, a recording's queue-state change, a newly written step
receipt, a new line in a tailed result file, and edge-triggered health alerts (queue runner log
gone quiet, low free memory, low boot-volume space). The first pass only sets the baselines and
reports alerts already active, so a restart does not replay history. Reading is local and
cheap; only the printed lines cost a reader anything.
"""

import argparse
import json
import shutil
import sys
import time
from collections.abc import Callable, Iterable
from dataclasses import dataclass, field
from pathlib import Path

from replay import recording_pipeline, workflow_parallel

RUNNER_STALE_SECONDS = 300.0
MEMORY_FREE_MIN = 20
DISK_FREE_MIN_BYTES = 30 * 1024**3
CAUSE_CHARACTERS = 160
STEPS = frozenset(recording_pipeline.ORDER)


@dataclass(frozen=True, slots=True)
class Sources:
    failures: Path
    runner_log: Path
    recordings: Path
    tails: tuple[Path, ...] = ()
    disk: Path = Path("/")


@dataclass(frozen=True, slots=True)
class Health:
    memory_free_percent: int
    disk_free_bytes: int
    now: float


@dataclass(slots=True)
class Watch:
    offsets: dict[Path, int] = field(default_factory=dict)
    states: dict[str, str] = field(default_factory=dict)
    receipts: dict[str, frozenset[str]] = field(default_factory=dict)
    alerts: frozenset[str] = frozenset()
    started: bool = False


def _new_lines(watch: Watch, path: Path) -> list[str]:
    """Complete lines appended since the last pass; a shrunk file starts over."""
    size = path.stat().st_size if path.exists() else 0
    offset = watch.offsets.get(path, size if not watch.started else 0)
    offset = 0 if size < offset else offset
    if size == offset:
        watch.offsets[path] = offset
        return []
    with path.open("rb") as stream:
        stream.seek(offset)
        chunk = stream.read(size - offset)
    complete = chunk[: chunk.rfind(b"\n") + 1]
    watch.offsets[path] = offset + len(complete)
    return complete.decode("utf-8", errors="replace").splitlines()


def _failure(line: str) -> str:
    try:
        record = json.loads(line)
    except json.JSONDecodeError:
        return f"FAIL {line[:CAUSE_CHARACTERS]}"
    stamp = time.strftime("%H:%M", time.localtime(float(record.get("t", 0))))
    cause = str(record.get("cause", ""))[:CAUSE_CHARACTERS]
    return (
        f"FAIL {stamp} {record.get('recording')} {record.get('step')} {record.get('class')} {cause}"
    )


def _runner_states(path: Path) -> dict[str, str]:
    """The recordings' states from the runner log's newest snapshot line."""
    if not path.exists():
        return {}
    with path.open("rb") as stream:
        stream.seek(max(0, path.stat().st_size - 65536))
        lines = stream.read().decode("utf-8", errors="replace").splitlines()
    for line in reversed(lines):
        if line.startswith("{"):
            try:
                rows = json.loads(line).get("recordings", [])
            except json.JSONDecodeError:
                continue
            return {str(row["recording"]): str(row["state"]) for row in rows}
    return {}


def _receipts(recordings: Path, recording: str) -> frozenset[str]:
    name = recording if recording.startswith("youtube-") else f"replay-{recording}"
    steps = recordings / name / "steps"
    if not steps.is_dir():
        return frozenset()
    return frozenset(path.stem for path in steps.glob("*.json") if path.stem in STEPS)


def _alerts(sources: Sources, health: Health) -> frozenset[str]:
    quiet = (
        not sources.runner_log.exists()
        or health.now - sources.runner_log.stat().st_mtime > RUNNER_STALE_SECONDS
    )
    checks = {
        "queue runner log quiet for over 5 min": quiet,
        f"free memory {health.memory_free_percent}% below {MEMORY_FREE_MIN}%": (
            health.memory_free_percent < MEMORY_FREE_MIN
        ),
        "boot volume free space below 30 GiB": health.disk_free_bytes < DISK_FREE_MIN_BYTES,
    }
    return frozenset(name for name, active in checks.items() if active)


def scan(watch: Watch, sources: Sources, health: Health) -> list[str]:
    """This pass's event lines; the first pass records the baselines and reports active alerts."""
    events = [_failure(line) for line in _new_lines(watch, sources.failures) if line.strip()]
    for tail in sources.tails:
        events += [f"{tail.stem.upper()} {line}" for line in _new_lines(watch, tail) if line]
    states = _runner_states(sources.runner_log)
    for recording, state in states.items():
        before = watch.states.get(recording)
        if watch.started and before is not None and before != state:
            events.append(f"STATE {recording} {before} -> {state}")
        receipts = _receipts(sources.recordings, recording)
        added = sorted(receipts - watch.receipts.get(recording, receipts))
        events += [f"STEP {recording} {name}" for name in added] if watch.started else []
        watch.receipts[recording] = receipts
    watch.states = states
    alerts = _alerts(sources, health)
    events += [f"ALERT {item}" for item in sorted(alerts - watch.alerts)]
    events += [f"CLEAR {item}" for item in sorted(watch.alerts - alerts)]
    watch.alerts = alerts
    watch.started = True
    return events


def measure(disk: Path) -> Health:
    return Health(
        memory_free_percent=workflow_parallel.read_load().memory_free_percent,
        disk_free_bytes=shutil.disk_usage(disk).free,
        now=time.time(),
    )


def run(
    sources: Sources,
    *,
    interval: float,
    passes: int | None = None,
    health: Callable[[Path], Health] = measure,
    sleep: Callable[[float], None] = time.sleep,
    emit: Callable[[str], None] = print,
) -> None:
    watch = Watch()
    done = 0
    while passes is None or done < passes:
        for event in scan(watch, sources, health(sources.disk)):
            emit(event)
        done += 1
        sys.stdout.flush()
        sleep(interval)


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--failures", type=Path, required=True)
    parser.add_argument("--runner-log", type=Path, required=True)
    parser.add_argument("--recordings", type=Path, required=True)
    parser.add_argument("--tail", type=Path, action="append", default=[])
    parser.add_argument("--interval", type=float, default=15.0)
    args = parser.parse_args(None if argv is None else list(argv))
    sources = Sources(args.failures, args.runner_log, args.recordings, tuple(args.tail))
    run(sources, interval=args.interval)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
