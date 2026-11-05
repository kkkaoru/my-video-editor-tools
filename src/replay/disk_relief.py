"""Keep both volumes roomy without asking anyone, oldest and most regenerable first.

``python -m replay.disk_relief [--apply] [--watch]`` (user instruction, 2026-11-05: delete
autonomously; new files matter most, old ones are backed up in R2). While a volume is under its
target it works through two tiers, each oldest first, and stops as soon as the target is met:

1. ``regenerable``: media and ``.build`` trees older than ``KEEP_DAYS`` under the recordings' media
   root, the derived-source roots, and any ``edits/work`` directory neither code nor configuration
   names. Small files stay, so settings, receipts and evidence survive. Still short, the same
   goes for idle recordings' own work down to ``SHORT_KEEP_DAYS``.
2. ``offload``: delivered movies and originals, oldest broadcast first, keeping the newest
   ``KEEP_VIDEOS``: ``delivery_offload`` deletes one only after proving the R2 copy equal.

Never touched: files of a running recording, recordings on ``retirementHold``, tier 1's copy of any
catalogued movie or edit configuration's source, ``edits/models``, ``private`` and ``downloads``.
Every removal is appended to ``edits/work/disk-relief.jsonl``.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import time
from collections.abc import Callable, Iterable, Iterator, Sequence
from dataclasses import dataclass, replace
from itertools import chain
from pathlib import Path

from replay import delivery_offload, disk_budget, r2_worker
from replay.core import ReplayError
from replay.recording_steps import recording_name
from replay.recordings import record

GIB = disk_budget.GIB
KEEP_DAYS = 7.0
SHORT_KEEP_DAYS = 1.0
KEEP_VIDEOS = 14
MIN_BYTES = 1024 * 1024
BOOT_TARGET = 150 * GIB
SSD_TARGET = 400 * GIB
WATCH_SECONDS = 900.0
MEDIA = frozenset(
    {
        ".mp4",
        ".mov",
        ".m4a",
        ".m4s",
        ".mkv",
        ".webm",
        ".wav",
        ".caf",
        ".aac",
        ".pcm",
        ".moti",
        ".f32",
    }
)
BUILD_TREES = frozenset({".build"})
DERIVED_ROOTS = (
    "source-decodable", "source-caption-cleanup", "source-comment-clean", "source-vertical", "work",
)  # fmt: skip
SCRATCH_ROOTS = ("deliveries-v2", "verticals", "yolo-final-v1-media")
REFERENCE_ROOTS = (
    "src", "scripts", "web-viewer/src", "TranscriptViewer/Sources", "native/apple-pro-apps/Sources",
    "docs", ".agents",
)  # fmt: skip
WORK_NAME = re.compile(rb"edits/work/([A-Za-z0-9._-]+)")
# ``pytest_scratch`` sweeps its own runs, and a running test's old-dated fixtures are not ours.
ALWAYS_KEPT = frozenset({"recording-pipeline", "recordings", "pytest-default"})
RECORDING = re.compile(r"(?:replay-\d+|youtube-[A-Za-z0-9_-]{11})")
# A file gone mid-walk or R2 out of reach: the watch retries at its next pass.
FAILURES = (ReplayError, OSError, ValueError, subprocess.SubprocessError)


@dataclass(frozen=True, slots=True)
class Candidate:
    path: Path
    bytes: int
    modified: float


@dataclass(frozen=True, slots=True)
class Keep:
    """What tier 1 must leave: running or held recordings' paths and catalogued inodes."""

    recordings: frozenset[str]
    inodes: frozenset[tuple[int, int]]
    cutoff: float

    def holds(self, path: Path) -> bool:
        return any(name in part for part in path.parts for name in self.recordings)


def inode(path: Path) -> tuple[int, int] | None:
    try:
        found = path.stat()
    except OSError:
        return None
    return (found.st_dev, found.st_ino)


def old_build_trees(here: Path, directories: list[str], keep: Keep) -> Iterator[Candidate]:
    """Old build trees in ``here``, taken out of ``directories`` so the walk skips them."""
    for name in [*directories]:
        tree = here / name
        if name in BUILD_TREES and not tree.is_symlink():
            directories.remove(name)
            if tree.stat().st_mtime < keep.cutoff:
                size = sum(item.stat().st_size for item in tree.rglob("*")
                           if item.is_file() and not item.is_symlink())  # fmt: skip
                yield Candidate(tree, size, tree.stat().st_mtime)


def old_media(here: Path, names: list[str], keep: Keep) -> Iterator[Candidate]:
    for name in names:
        path = here / name
        if path.suffix.lower() not in MEDIA or path.is_symlink():
            continue
        found = path.stat()
        if (
            found.st_size >= MIN_BYTES
            and found.st_mtime < keep.cutoff
            and (found.st_dev, found.st_ino) not in keep.inodes
        ):
            yield Candidate(path, found.st_size, found.st_mtime)


def regenerable(root: Path, keep: Keep) -> Iterator[Candidate]:
    """Old media and build trees under ``root``, never following a symbolic link."""
    for folder, directories, names in os.walk(root):
        here = Path(folder)
        if keep.holds(here):
            directories.clear()
            continue
        yield from old_build_trees(here, directories, keep)
        yield from old_media(here, names, keep)


def candidates_under(roots: Iterable[Path], keep: Keep) -> list[Candidate]:
    return list(chain.from_iterable(regenerable(root, keep) for root in roots if root.is_dir()))


def referenced_work(checkout: Path, configurations: Iterable[Path]) -> set[str]:
    """``edits/work`` directories that code, documents or current configuration name."""
    named: set[str] = set(ALWAYS_KEPT)
    files = [*configurations]
    for root in REFERENCE_ROOTS:
        files += [path for path in (checkout / root).rglob("*") if path.is_file()]
    for path in files:
        try:
            named.update(item.decode() for item in WORK_NAME.findall(path.read_bytes()))
        except OSError:
            continue
    return named


def unreferenced_work(work: Path, named: set[str]) -> list[Path]:
    return sorted(path for path in work.iterdir() if path.is_dir() and path.name not in named)


def remove(candidate: Candidate, log: Path) -> None:
    if candidate.path.is_dir():
        shutil.rmtree(candidate.path)
    else:
        candidate.path.unlink()
    with log.open("a", encoding="utf-8") as output:
        row = {"at": round(time.time()), "tier": "regenerable", "path": str(candidate.path),
               "bytes": candidate.bytes}  # fmt: skip
        output.write(json.dumps(row, ensure_ascii=False) + "\n")


def modified_time(candidate: Candidate) -> float:
    return candidate.modified


def clear_oldest(
    candidates: Iterable[Candidate], *, volume: Path, target: int, log: Path, apply: bool
) -> int:
    """Remove the oldest candidates until ``volume`` has ``target`` free; the bytes they held."""
    freed = 0
    for candidate in sorted(candidates, key=modified_time):
        if apply and disk_budget.measure(volume).free >= target:
            break
        if not apply and disk_budget.measure(volume).free + freed >= target:
            break
        try:
            if apply:
                remove(candidate, log)
        except OSError as error:
            print(f"kept {candidate.path}: {error}", flush=True)
            continue
        freed += candidate.bytes
    return freed


def broadcast_start(row: dict[str, object]) -> str:
    return str(row.get("broadcastStartJST", ""))


def videos(catalog: Path) -> list[dict[str, object]]:
    """Catalogued videos, oldest broadcast first."""
    rows = record(json.loads(catalog.read_text(encoding="utf-8")))["videos"]
    found = [record(row) for row in rows] if isinstance(rows, list) else []
    return sorted(found, key=broadcast_start)


def row_paths(row: dict[str, object]) -> Iterator[Path]:
    original = row.get("original")
    values = row.get("localPath"), record(original).get("localPath") if original else None
    return (Path(value) for value in values if isinstance(value, str))


def catalogued_paths(rows: Iterable[dict[str, object]]) -> Iterator[Path]:
    return chain.from_iterable(map(row_paths, rows))


def source_paths(edits: Path) -> Iterator[Path]:
    for config in edits.glob("*/edit-config.json"):
        try:
            source = record(json.loads(config.read_text(encoding="utf-8"))).get("source")
        except (OSError, ValueError) as error:
            print(f"unreadable {config}: {error}", flush=True)
            continue
        if isinstance(source, str):
            yield Path(source)


def offload_rows(rows: list[dict[str, object]]) -> Iterator[tuple[str, str, Path, str]]:
    """(recording, role, local path, R2 key) of every old video's files on disk, oldest first."""
    for row in rows[: max(0, len(rows) - KEEP_VIDEOS)]:
        found = RECORDING.search(str(row.get("id", "")))
        if found is None:
            continue
        movie, original = row.get("localPath"), row.get("original")
        if isinstance(movie, str) and Path(movie).is_file():
            yield found[0], "delivery", Path(movie), f"media/{row.get('movie')}"
        data = record(original) if original else {}
        path = data.get("localPath")
        if isinstance(path, str) and Path(path).is_file():
            yield found[0], "original", Path(path), f"media-original/{data.get('movie')}"


def offload_oldest(
    rows: list[dict[str, object]], *, volume: Path, target: int, keep: Keep, log: Path
) -> int:
    """Delete old local copies proven equal to R2, until ``volume`` has ``target`` free."""
    freed = 0
    with r2_worker.session(delivery_offload.BUCKET) as store:
        for recording, role, path, key in offload_rows(rows):
            if disk_budget.measure(volume).free >= target:
                break
            if recording in keep.recordings:
                continue
            size = path.stat().st_size
            try:
                delivery_offload.offload(
                    delivery_offload.edit_root(recording), role=role, path=path, key=key,
                    store=store, sent=delivery_offload.uploaded(key),
                )  # fmt: skip
            except (ReplayError, OSError, ValueError) as error:
                print(f"kept {path}: {error}", flush=True)
                continue
            freed += size
            with log.open("a", encoding="utf-8") as output:
                row = {"at": round(time.time()), "tier": "offload", "path": str(path),
                       "key": key, "bytes": size}  # fmt: skip
                output.write(json.dumps(row, ensure_ascii=False) + "\n")
    return freed


def running_recordings() -> set[str]:
    listed = subprocess.run(["ps", "-Ao", "command="], capture_output=True, text=True, check=True)
    found = re.findall(r"recording_pipeline run (\d+|youtube-[\w-]{11})\s", listed.stdout)
    return {recording_name(item) for item in found}


def relieve(checkout: Path, *, apply: bool, now: float) -> dict[str, int]:
    """One pass over both volumes; the bytes each tier freed (or would, without ``apply``)."""
    pipeline_settings = checkout / "edits/work/recording-pipeline/DEFAULTS-v1.json"
    defaults = record(json.loads(pipeline_settings.read_text(encoding="utf-8")))
    settings = checkout / "edits/work/recording-pipeline/RETIREMENT-v1.json"
    held = record(json.loads(settings.read_text(encoding="utf-8"))).get("retirementHold")
    rows = videos(checkout / "web-viewer/catalog.published.json")
    protected = {*catalogued_paths(rows), *source_paths(checkout / "edits")}
    keep = Keep(
        frozenset({*running_recordings(), *(map(str, held) if isinstance(held, list) else ())}),
        frozenset(found for found in map(inode, protected) if found is not None),
        now - KEEP_DAYS * 86400,
    )
    log = checkout / "edits/work/disk-relief.jsonl"
    deliveries = Path(str(defaults["deliveryRoot"]))
    media = Path(str(record(defaults["captions"])["mediaRoot"]))
    ssd_roots = [media, *(deliveries / name for name in DERIVED_ROOTS),
                 *(media.parent / name for name in SCRATCH_ROOTS)]  # fmt: skip
    work = checkout / "edits/work"
    configurations = [
        *work.glob("recording-pipeline/references/*.json"),
        work / "recording-pipeline/DEFAULTS-v1.json",
        work / "clean-defaults-v1.json",
        *(checkout / "edits").glob("*/video-config.json"),
    ]
    named = referenced_work(checkout, configurations)
    boot_roots = [*unreferenced_work(work, named), work / "recordings"]
    outcome: dict[str, int] = {}
    short = replace(keep, cutoff=now - SHORT_KEEP_DAYS * 86400)
    for label, volume, target, roots, recordings in (
        ("boot", checkout, BOOT_TARGET, boot_roots, work / "recordings"),
        ("ssd", deliveries, SSD_TARGET, ssd_roots, media),
    ):
        passes = ((roots, keep), ([recordings], short))
        outcome[label] = sum(
            clear_oldest(
                candidates_under(folders, kept),
                volume=volume, target=target, log=log, apply=apply,
            )
            for folders, kept in passes
        )  # fmt: skip
    if apply and disk_budget.measure(deliveries).free < SSD_TARGET:
        outcome["offload"] = offload_oldest(
            rows, volume=deliveries, target=SSD_TARGET, keep=keep, log=log
        )
    return outcome


def main(argv: Sequence[str] | None = None, *, sleep: Callable[[float], None] = time.sleep) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--watch", action="store_true")
    args = parser.parse_args(argv)
    checkout = Path.cwd().resolve()
    while True:
        try:
            outcome = relieve(checkout, apply=args.apply, now=time.time())
            print(json.dumps({"apply": args.apply, **outcome}), flush=True)
        except FAILURES as error:
            if not args.watch:
                raise
            print(json.dumps({"failed": f"{type(error).__name__}: {error}"}), flush=True)
        if not args.watch:
            return 0
        sleep(WATCH_SECONDS)


if __name__ == "__main__":
    raise SystemExit(main())
