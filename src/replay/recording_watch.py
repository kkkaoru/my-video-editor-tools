"""Tell an agent about a recording queue only when it must act, each event once.

``python -m replay.recording_watch --queue Q --defaults D --jobs DIR`` polls what the runner and
the intake already write (``DIR/queue-runner.log``, ``DIR/intake*.status``) and prints one JSON
line per new event, which also goes to ``Q.events.jsonl`` so a later session sees it and a
restarted watcher stays silent about it. A run that works reports each finished step
(``step-ok``, with its place among the pipeline's steps); ``--status`` prints where every queued
recording is now, with each running step's percentage (``recording_progress``).

With ``--until-event`` it exits at the first new event, so an agent running it as a background
command is woken only then, and starts it again after acting.

A failed launch is not an event: the runner launches it again, and ``recording_pipeline run``
reuses every finished step's receipt, so it resumes at the step that failed. Only a recording that
failed ``MAX_FAILURES`` times (``held``) needs an agent, and fixing the shared code or running
``recording_queue unhold`` makes the runner resume it at that same step.
"""

import argparse
import json
import time
from collections.abc import Callable, Iterator, Sequence
from pathlib import Path

from replay import disk_budget, recording_progress
from replay import recording_pipeline as pipelines
from replay import recording_queue as queue_runner
from replay.core import ReplayError
from replay.recordings import record

STALE_SECONDS = 600.0  # ten missed ticks of the runner's 60 s poll
POLL_SECONDS = 60.0
LOG_LINES = 12
STEPS = (*pipelines.ORDER, "retire-intermediates")
PROBLEM_STATES = frozenset({"held", "error", "launch-refused"})
RESUME = (
    "Fix the shared cause, then the runner resumes at the failed step by itself "
    "(a code change clears the hold; otherwise recording_queue unhold ID)."
)


def last_tick(log: Path) -> list[dict[str, object]]:
    """The recordings of the runner's latest tick (none before its first)."""
    for line in reversed(log.read_text(encoding="utf-8", errors="replace").splitlines()):
        try:
            found = json.loads(line)
        except ValueError:
            continue
        if isinstance(found, dict) and isinstance(found.get("recordings"), list):
            return [record(row) for row in found["recordings"]]
    return []


def log_tail(directory: Path, launch: str) -> str:
    log = directory / f"{launch}.log"
    if not log.is_file():
        return ""
    lines = log.read_text(encoding="utf-8", errors="replace").splitlines()
    return "\n".join(line[:300] for line in lines[-LOG_LINES:])


def modified_time(path: Path) -> float:
    return path.stat().st_mtime


def finished_steps(steps: Path) -> list[tuple[str, dict[str, object]]]:
    """Every step's receipt, oldest first."""
    found = sorted(steps.glob("*.json"), key=modified_time)
    return [
        (path.stem, record(json.loads(path.read_text(encoding="utf-8"))))
        for path in found
        if path.stem in STEPS
    ]


def step_events(name: str, steps: Path) -> Iterator[dict[str, object]]:
    for number, (step, receipt) in enumerate(finished_steps(steps), start=1):
        yield {
            "key": f"step-ok:{name}:{step}:{receipt.get('endedAt')}",
            "kind": "step-ok",
            "recording": name,
            "step": step,
            "done": f"{number}/{len(STEPS)}",
            "wallSeconds": receipt.get("wallSeconds"),
        }


def running_lines(name: str, *, defaults: dict[str, object], checkout: Path) -> Iterator[str]:
    """One line per running step of the recording, with its percentage and basis."""
    for found in recording_progress.running_steps(
        name,
        edit_work=checkout / "edits/work",
        recordings=Path(str(defaults["root"])),
    ):
        percent = f"{found.percent}%" if found.percent >= 0 else "?"
        yield f"  {found.step}: {percent} ({found.basis})"


def status_lines(log: Path, *, defaults: dict[str, object], checkout: Path) -> Iterator[str]:
    """One line per queued recording: its state, finished steps and the steps left."""
    for row in last_tick(log):
        name = str(row["recording"])
        layout = pipelines.layout_of(name, defaults, checkout)
        done = [found[0] for found in finished_steps(layout.steps)]
        # A recording published before a step was added stays done without that step's receipt.
        finished = row["state"] == "done"
        left = [] if finished else [step for step in STEPS if step not in done]
        total = len(done) if finished else len(STEPS)
        yield (
            f"{name}: {row['state']}, {len(done)}/{total} steps, "
            f"last {done[-1] if done else '-'}, next {', '.join(left[:3]) or '-'}"
        )
        yield from running_lines(name, defaults=defaults, checkout=checkout)


def recording_events(
    row: dict[str, object], *, defaults: dict[str, object], checkout: Path
) -> Iterator[dict[str, object]]:
    name, state = str(row["recording"]), str(row["state"])
    yield from step_events(name, pipelines.layout_of(name, defaults, checkout).steps)
    if state == "done":
        yield {"key": f"finished:{name}", "kind": "finished", "recording": name}
    elif state in PROBLEM_STATES:
        layout = pipelines.layout_of(name, defaults, checkout)
        launches = queue_runner.launches(layout)
        launch = launches[-1] if launches else ""
        yield {
            "key": f"{state}:{name}:{launch}",
            "kind": state,
            "recording": name,
            "launch": launch,
            "error": row.get("error", ""),
            "log": log_tail(layout.directory, launch),
            "resume": RESUME,
        }


def events(
    defaults: dict[str, object], *, jobs: Path, checkout: Path, now: float
) -> Iterator[dict[str, object]]:
    runner = jobs / "queue-runner.log"
    status = jobs / "queue-runner.status"
    if status.is_file() or not runner.is_file() or now - runner.stat().st_mtime > STALE_SECONDS:
        yield {"key": f"runner-stopped:{int(now // STALE_SECONDS)}", "kind": "runner-stopped",
               "detail": f"No tick in {runner}: restart recording_queue run"}  # fmt: skip
    for found in sorted(jobs.glob("intake*.status")):
        if found.read_text(encoding="utf-8").strip() != "0":
            yield {"key": f"intake-failed:{found.name}", "kind": "intake-failed",
                   "log": log_tail(jobs, found.stem),
                   "resume": "Fix the cause and run the intake again."}  # fmt: skip
    if runner.is_file():
        for row in last_tick(runner):
            yield from recording_events(row, defaults=defaults, checkout=checkout)
    for volume in (checkout, Path(str(defaults["deliveryRoot"]))):
        room = disk_budget.measure(volume)
        if room.spare < 0:
            yield {"key": f"disk-low:{volume}:{int(now // 3600)}", "kind": "disk-low",
                   "detail": f"{volume} is {-room.spare} bytes below its floor"}  # fmt: skip


def report(
    queue: Path, defaults: dict[str, object], *, jobs: Path, checkout: Path, now: float
) -> list[dict[str, object]]:
    """The events not reported yet, printed and recorded."""
    store = queue.with_name(f"{queue.name}.events.jsonl")
    seen = (
        {record(json.loads(line))["key"] for line in store.read_text("utf-8").splitlines()}
        if store.is_file()
        else set()
    )
    fresh = []
    for event in events(defaults, jobs=jobs, checkout=checkout, now=now):
        if event["key"] in seen:
            continue
        seen.add(event["key"])
        fresh.append(event)
        with store.open("a", encoding="utf-8") as output:
            output.write(json.dumps(event, ensure_ascii=False) + "\n")
        print(json.dumps(event, ensure_ascii=False), flush=True)
    return fresh


def main(argv: Sequence[str] | None = None, *, sleep: Callable[[float], None] = time.sleep) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--queue", type=Path, required=True)
    parser.add_argument("--defaults", type=Path, required=True)
    parser.add_argument("--jobs", type=Path, required=True)
    parser.add_argument("--once", action="store_true")
    parser.add_argument("--status", action="store_true")
    # Exit at the first event: a background command wakes its agent only then.
    parser.add_argument("--until-event", action="store_true")
    args = parser.parse_args(argv)
    checkout = Path.cwd().resolve()
    defaults = pipelines.load_defaults(args.defaults)
    if args.status:
        for line in status_lines(
            args.jobs / "queue-runner.log", defaults=defaults, checkout=checkout
        ):
            print(line)
        return 0
    failed: set[str] = set()
    while True:
        try:
            fresh = report(args.queue, defaults, jobs=args.jobs, checkout=checkout, now=time.time())
            if args.until_event and fresh:
                return 0
        except (ReplayError, OSError, ValueError) as error:
            detail = f"{type(error).__name__}: {error}"
            if detail not in failed:
                failed.add(detail)
                print(json.dumps({"kind": "watch-failed", "detail": detail}), flush=True)
        if args.once:
            return 0
        sleep(POLL_SECONDS)


if __name__ == "__main__":
    raise SystemExit(main())
