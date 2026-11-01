"""What a recording's cleanup chunks learned, carried to its later chunks; attempt anomalies.

Learned store (``<cleanup work>/learned/residuals.jsonl``, one JSON row per confirmed residual):
a residual line of a failed attempt is a confirmed caption residual only when its chunk was then
accepted by a later attempt (the repair removed it) and the audit had kept it as caption-coloured
(``residualLines`` hold only such lines when the colour gate runs).

Candidates (rows with a ``status``): what the confirmation above cannot see is stored too, so the
hardest chunks are not lost. A chunk that failed every attempt contributes its residual lines as
``verified``: the audit read them, left out interface text and labels, and found caption colour
in their source boxes (the source caption-style check). Colour-gate regions (``colourRegions``)
are not stored: the gate also flags restored scene pink (10/27's avatar rims), which must never
seed a band or reach the YOLO pool. Rows marked ``unconfirmed`` (written by an earlier version)
are read by neither.

Use: a band (one row class; columns united) that such residuals confirmed in at least
``MIN_SUPPORT`` chunks seeds the plans of later chunks as extra repair line boxes on frames
whose caption region reaches it. It only widens repair: the colour key still removes
caption-coloured pixels only, and the audit gate (OCR and colour) never reads the store.

Anomalies: ``threshold`` derives from the latest other recordings' chunk receipts the mean
attempts per chunk plus two standard errors over ``WINDOW`` chunks; ``anomalous`` compares a
recording's last ``WINDOW`` chunks with it.
"""

import fcntl
import hashlib
import json
import math
import os
from collections.abc import Iterable, Mapping, Sequence
from dataclasses import dataclass
from itertools import pairwise
from pathlib import Path

STORE = Path("learned") / "residuals.jsonl"
MIN_SUPPORT = 2  # chunks that must confirm a band before it seeds a plan
ROW_STEP = 10  # rows of one row class (a caption line's top and height, measured 10-28)
WINDOW = 20  # chunks the attempt-rate anomaly averages over
BASELINE_RECORDINGS = 4  # recent other recordings the attempt limit derives from
MAX_ROW_BYTES = 4096
MAX_STORE_BYTES = 16 * 1024 * 1024
FRAME_BYTES = 140_000  # a training JPEG with its label (dataset-v1: 1.3 GB over 9299 frames)

BRAKE = Path("learning-brake.json")  # beside the cleanup work roots; an agent removes it
DIVERGE_SEGMENTS = 3  # consecutive version segments whose attempts must not settle
# A version segment needs this many chunks: the store changes after nearly every healed chunk, and
# concurrent chunks finish interleaved, so shorter runs are noise and join the next run.
SEGMENT_CHUNKS = 5
# The code a chunk's plan and learning come from: its version is part of every receipt.
CODE = ("subtitle_sampled_job.py", "subtitle_sampled_run.py", "subtitle_profile.py",
        "cleanup_learning.py", "subtitle_sampled_admission.py")  # fmt: skip

UNCONFIRMED, VERIFIED = "unconfirmed", "verified"

Box = tuple[int, int, int, int]  # left, top, right, bottom


@dataclass(frozen=True, slots=True)
class Band:
    box: Box
    chunks: int


def _lines(attempt: Mapping[str, object], field: str) -> list[tuple[int, Box]]:
    """(chunk frame, box) of an attempt's ``field`` lines (frame -1 when undated)."""
    found: list[tuple[int, Box]] = []
    value = attempt.get(field)
    for line in value if isinstance(value, list) else []:
        box = line.get("box") if isinstance(line, dict) else None
        frame = line.get("frame") if isinstance(line, dict) else None
        if isinstance(box, list) and len(box) == 4:
            x, y, w, h = (float(value) for value in box)
            at = frame if type(frame) is int else -1
            found.append((at, (int(x), int(y), math.ceil(x + w), math.ceil(y + h))))
    return found


def _failed(receipt: Mapping[str, object]) -> list[Mapping[str, object]]:
    """The failed attempts: those before the accepted one, or all when none was accepted."""
    accepted = receipt.get("accepted")
    attempts = receipt.get("attempts")
    if not isinstance(attempts, list) or not (accepted is None or isinstance(accepted, str)):
        return []
    failed: list[Mapping[str, object]] = []
    for attempt in attempts:
        if not isinstance(attempt, dict) or attempt.get("directory") == accepted:
            break
        failed.append(attempt)
    return failed


def confirmed_frames(receipt: Mapping[str, object]) -> list[tuple[int, Box]]:
    """(chunk frame, residual box) of the failed attempts before the accepted one; none when
    nothing passed."""
    if not isinstance(receipt.get("accepted"), str):
        return []
    return [item for attempt in _failed(receipt) for item in _lines(attempt, "residualLines")]


def candidate_frames(receipt: Mapping[str, object]) -> list[tuple[int, Box, str]]:
    """(chunk frame, box, status) of what confirmation cannot see: the residual lines of a chunk
    that failed every attempt, verified."""
    if receipt.get("accepted") is not None:
        return []
    return [
        (frame, box, VERIFIED)
        for attempt in _failed(receipt)
        for frame, box in _lines(attempt, "residualLines")
    ]


def confirmed(receipt: Mapping[str, object]) -> list[Box]:
    """Residual boxes of the failed attempts before the accepted one (none when nothing passed)."""
    return [box for _, box in confirmed_frames(receipt)]


def record(
    work: Path,
    chunk: str,
    receipt: Mapping[str, object],
    times: Sequence[float] = (),
) -> int:
    """Append the chunk's confirmed residuals and candidates to the recording's store; return how
    many.
    ``times`` (source seconds per chunk frame) dates each row for the YOLO export."""
    seen: set[tuple[int, Box]] = set()
    rows: list[str] = []
    found = [(frame, box, "") for frame, box in confirmed_frames(receipt)]
    for frame, box, status in [*found, *candidate_frames(receipt)]:
        if (frame, box) in seen:
            continue
        seen.add((frame, box))
        row: dict[str, object] = {"chunk": chunk, "box": list(box)}
        if status:
            row["status"] = status
        if 0 <= frame < len(times):
            row["seconds"] = round(times[frame], 6)
        rows.append(json.dumps(row) + "\n")
    if not rows:
        return 0
    path = work / STORE
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
    with os.fdopen(descriptor, "a", encoding="utf-8") as stream:
        fcntl.flock(stream, fcntl.LOCK_EX)  # chunk jobs of one recording append concurrently
        stream.write("".join(rows))
    return len(rows)


def training_frames(work: Path) -> list[dict[str, object]]:
    """Confirmed residual moments for a YOLO dataset, one per row class and source second
    (deduplicated): source seconds and the residual box. Rows without a date are skipped."""
    path = work / STORE
    if not path.is_file() or path.stat().st_size > MAX_STORE_BYTES:
        return []
    chosen: dict[tuple[int, int, int], dict[str, object]] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        try:
            row = json.loads(line) if len(line) <= MAX_ROW_BYTES else None
        except ValueError:
            continue
        if not isinstance(row, dict) or row.get("status") == UNCONFIRMED:
            continue
        box, seconds = row.get("box"), row.get("seconds")
        if not (isinstance(box, list) and len(box) == 4 and isinstance(seconds, (int, float))):
            continue
        top, height = _row_class((box[0], box[1], box[2], box[3]))
        chosen.setdefault((top, height, int(seconds)), {"seconds": seconds, "box": box})
    return [chosen[key] for key in sorted(chosen)]


def _rows(work: Path) -> list[tuple[str, Box]]:
    path = work / STORE
    if not path.is_file() or path.stat().st_size > MAX_STORE_BYTES:
        return []
    found: list[tuple[str, Box]] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if len(line) > MAX_ROW_BYTES:
            continue
        try:
            row = json.loads(line)
        except ValueError:
            continue  # a partial last line of a killed writer
        box = row.get("box") if isinstance(row, dict) else None
        if isinstance(row, dict) and row.get("status") == UNCONFIRMED:
            continue  # the colour gate alone flagged it: it seeds nothing
        if isinstance(box, list) and len(box) == 4 and all(type(v) is int for v in box):
            found.append((str(row.get("chunk", "")), (box[0], box[1], box[2], box[3])))
    return found


def _row_class(box: Box) -> tuple[int, int]:
    """Its top and height in ``ROW_STEP`` rows: residuals of one caption placement share both."""
    return box[1] // ROW_STEP, (box[3] - box[1]) // ROW_STEP


def bands(work: Path, *, exclude: str = "") -> list[Band]:
    """Residual bands (one row class, columns united) that ``MIN_SUPPORT`` chunks or more
    (other than ``exclude``) confirmed; never one region-sized blob of drifting rows."""
    groups: dict[tuple[int, int], tuple[Box, set[str]]] = {}
    for chunk, box in _rows(work):
        if chunk == exclude:
            continue
        key = _row_class(box)
        united, chunks = groups.get(key, (box, set()))
        groups[key] = (
            (min(united[0], box[0]), min(united[1], box[1]),
             max(united[2], box[2]), max(united[3], box[3])),
            chunks | {chunk},
        )  # fmt: skip
    return [
        Band(box, len(chunks))
        for _, (box, chunks) in sorted(groups.items())
        if len(chunks) >= MIN_SUPPORT
    ]


def seeds(frames: Sequence[object], learned: Sequence[Band]) -> dict[int, list[Box]]:
    """Extra repair boxes per frame: each learned band on every caption frame whose region
    reaches it (overlaps it or lies within one band height of it)."""
    found: dict[int, list[Box]] = {}
    for index, entry in enumerate(frames):
        if not isinstance(entry, dict) or entry.get("classification") != "overlay-subtitle":
            continue
        region = entry.get("region")
        if not isinstance(region, dict):
            continue
        top, bottom = int(region["y"]), int(region["y"]) + int(region["height"])
        for band in learned:
            reach = band.box[3] - band.box[1]
            if band.box[1] < bottom + reach and band.box[3] > top - reach:
                found.setdefault(index, []).append(band.box)
    return found


def attempts(receipts: Iterable[Path]) -> list[int]:
    """Attempts used per chunk, from readable chunk receipts."""
    counts: list[int] = []
    for path in receipts:
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            continue
        tried = data.get("attempts") if isinstance(data, dict) else None
        if isinstance(tried, list) and tried:
            counts.append(len(tried))
    return counts


def recent_works(work: Path, count: int = BASELINE_RECORDINGS) -> list[Path]:
    """The ``count`` most recently changed other cleanup work roots of the same kind beside
    ``work`` (``clean-replay-*`` or ``clean-youtube-*``): the current planning regime of that
    kind (older recordings ran other plans; YouTube and TikTok replays differ in kind)."""
    kind = work.name.split("-")[1] if work.name.count("-") >= 2 else ""
    others = [
        path
        for path in work.parent.glob(f"clean-{kind}-*-p1")
        if path.is_dir() and path.resolve() != work.resolve()
    ]
    return sorted(others, key=lambda path: path.stat().st_mtime)[-count:]


def threshold(baseline: Sequence[int], window: int = WINDOW) -> float | None:
    """Mean attempts per chunk plus two standard errors of a ``window``-chunk mean."""
    if len(baseline) < 2 * window:
        return None
    mean = sum(baseline) / len(baseline)
    spread = math.sqrt(sum((value - mean) ** 2 for value in baseline) / len(baseline))
    return mean + 2 * spread / math.sqrt(window)


def anomalous(recent: Sequence[int], limit: float | None, window: int = WINDOW) -> bool:
    """The last ``window`` chunks used more attempts on average than ``limit`` allows."""
    if limit is None or len(recent) < window:
        return False
    tail = recent[-window:]
    return sum(tail) / window > limit


def braked(work: Path) -> bool:
    """Learning is stopped (no seeding, no promotion) until an agent removes the brake."""
    return (work.parent / BRAKE).is_file()


def store_version(work: Path) -> str:
    """Digest of the learned store as a chunk reads it (``empty`` without one)."""
    path = work / STORE
    if not path.is_file():
        return "empty"
    return hashlib.sha256(path.read_bytes()).hexdigest()[:16]


def code_version(package: Path) -> str:
    """Digest of the planning and learning modules a chunk job runs."""
    digest = hashlib.sha256()
    for name in CODE:
        path = package / name
        digest.update(name.encode("utf-8") + b"\0" + (path.read_bytes() if path.is_file() else b""))
    return digest.hexdigest()[:16]


def segments(history: Sequence[tuple[int, str]]) -> list[float]:
    """Mean attempts per run of consecutive chunks that used one version (a swap starts a new
    run once the current one holds ``SEGMENT_CHUNKS`` chunks), in finish order; a shorter
    trailing run is not a segment yet."""
    means: list[float] = []
    run: list[int] = []
    previous: str | None = None
    for count, version in history:
        if previous is not None and version != previous and len(run) >= SEGMENT_CHUNKS:
            means.append(sum(run) / len(run))
            run = []
        run.append(count)
        previous = version
    if len(run) >= SEGMENT_CHUNKS:
        means.append(sum(run) / len(run))
    return means


def diverged(history: Sequence[tuple[int, str]], limit: float | None) -> bool:
    """Replacements diverge: the last ``DIVERGE_SEGMENTS`` version segments' attempts never fell
    and all stayed above ``limit`` (the recording kind's measured settle level)."""
    means = segments(history)
    if limit is None or len(means) < DIVERGE_SEGMENTS:
        return False
    tail = means[-DIVERGE_SEGMENTS:]
    return all(b >= a for a, b in pairwise(tail)) and min(tail) > limit


def brake(work: Path, reason: str, evidence: Sequence[str]) -> Path:
    """Stop learning for every recording until an agent removes the returned file."""
    path = work.parent / BRAKE
    path.write_text(json.dumps({"reason": reason, "evidence": list(evidence)}, indent=1) + "\n",
                    encoding="utf-8")  # fmt: skip
    return path


def estimated_bytes(frames: Sequence[object]) -> int:
    """Disk a dataset of these training moments takes (``FRAME_BYTES`` each)."""
    return len(frames) * FRAME_BYTES
