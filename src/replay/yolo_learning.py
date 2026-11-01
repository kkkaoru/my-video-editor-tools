"""Incrementally update the caption/face YOLO detector from confirmed cleanup findings; promote it
only past the pinned model's bar; hand it to the next chunk that starts.

One ``cycle`` (``python -m replay.yolo_learning cycle CONFIG.json``) trains only on what is new:

1. Moments: the dated confirmed residuals of every listed cleanup store
   (``cleanup_learning.training_frames``) that no earlier cycle trained on, spread evenly.
2. Face labels: a one-second Vision face window per moment (``measure-media video_face_track``;
   ``yolo_joint_dataset`` keeps a frame only with a trusted face box). Windows are cached per
   moment and never recomputed; new ones stop when ``labelSeconds`` is spent (the rest waits for
   the next cycle).
3. Dataset: ``yolo_joint_dataset`` builds the new frames and the regression recordings; training
   adds a replay buffer from the base dataset's training list (as many frames as are new, at most
   ``REPLAY_SHARE`` of the base) so the update does not forget. Validation is the regression
   frames plus a small sample of the frozen test frames (early stopping looks at them).
4. Training: ``yolo_train`` from the promoted (else pinned) weights with the backbone frozen
   (``freeze``), a few epochs, at nice 15, inside a memory reservation and a GPU lease.
5. Evaluation in two stages with ``yolo_joint_eval``: first the regression recordings and the
   frozen sample (a failure rejects at once); only then the full frozen recordings. Promotion
   needs every gate to pass and glyph coverage and face detection at least the pinned model's own
   numbers on each frozen recording (the bar is unchanged).
6. Promotion: ``<models>/learned-<sha12>/weights.pt`` with its sha256 and ``promoted.json``. A
   chunk job reads it when it starts (``promoted``): its stored yolo-track boxes then no longer
   match and it detects with the promoted weights itself, so no chunk mixes two models.

Each cycle writes ``decision.json`` with its per-phase wall seconds and prints a ``progress`` line
to standard error at every phase and every ``PROGRESS_EVERY`` new face windows, so its log shows
where a running cycle is. The cleanup learning brake
stops cycles and promotion use; one cycle runs at a time.
"""

import argparse
import dataclasses
import fcntl
import hashlib
import json
import math
import os
import shutil
import subprocess
import sys
import time
from collections.abc import Callable, Mapping, Sequence
from pathlib import Path

import cv2
import numpy as np

from replay import cleanup_learning, gpu_lease, memory_admission
from replay.core import ReplayError
from replay.recordings import record
from replay.subtitle_inpaint import Image
from replay.subtitle_yolo_regions import Corners, YoloSettings, load_detector
from replay.yolo_joint_dataset import caption_lines
from replay.yolo_labels import CAPTION, PixelBox, yolo_row
from replay.yolo_track import TRAIN_WIDTH

PROMOTED = "promoted.json"
WINDOW_SECONDS = 1.0
DATASET_STEP = 15  # frames between kept frames: each one-second window holds at least one
TRAIN_JOB = "yolo-train"
STATE = "cycle-state.json"
REPLAY_SHARE = 0.2  # replay buffer at most this share of the base training frames
SAMPLE_FRAMES = 40  # frozen frames per recording in the validation / first-stage sample
HIT_SHARE = 0.5  # a detection covering half the residual box means the model already finds it
HASH_DISTANCE = 4  # dhash bits within which two misses count as one moment
SCREEN_BATCH = 16
MEMORY_QUEUE = "memory-waiting"  # source_clean_run.MEMORY_QUEUE
POOL_MOMENTS = 2000  # face windows kept for reuse (newest); ~20 KB each
DISK_CAP_BYTES = 3 * 2**30  # learning work, cached labels and learned weights together
FREEZE_LAYERS = 10  # YOLO11 backbone layers kept frozen (the neck and head train)
LEASE_POLL_SECONDS = 30.0
PROGRESS_EVERY = 10  # new face windows between progress lines

Run = Callable[[Sequence[str]], int]


def progress(message: str) -> None:
    """One timestamped line on standard error (the loop log) for the running phase."""
    print(f"progress {time.strftime('%F %T')} {message}", file=sys.stderr, flush=True)


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def promoted(settings: YoloSettings, models: Path, brake_root: Path) -> YoloSettings:
    """The detector a starting chunk uses: the promoted weights when ``models/promoted.json``
    names intact ones and learning is not braked, else the configured (pinned) ones."""
    path = models / PROMOTED
    if not path.is_file() or (brake_root / cleanup_learning.BRAKE).is_file():
        return settings
    data = record(json.loads(path.read_text(encoding="utf-8")))
    weights, digest = Path(str(data.get("weights", ""))), str(data.get("sha256", ""))
    if not weights.is_file() or _sha256(weights) != digest:
        return settings
    return dataclasses.replace(settings, weights=weights, sha256=digest)


def moments(works: Sequence[Path]) -> list[tuple[Path, float]]:
    """(cleanup work root, source seconds) of every exported training moment."""
    return [
        (work, float(str(item["seconds"])))
        for work in works
        for item in cleanup_learning.training_frames(work)
    ]


def window_request(source: Path, seconds: float) -> dict[str, object]:
    start = max(0.0, seconds - WINDOW_SECONDS / 2)
    return {
        "path": str(source),
        "plan": {
            "durationSeconds": WINDOW_SECONDS,
            "maximumFrames": 30,
            "redetectIntervalSeconds": 0.5,
            "sourceTimescale": 90000,
            "startSeconds": round(start, 6),
        },
    }


def label(
    source: Path,
    seconds: Sequence[float],
    directory: Path,
    *,
    native: Path,
    run: Run,
    deadline: float = math.inf,
    clock: Callable[[], float] = time.monotonic,
) -> list[Path]:
    """Vision face windows of the moments (cached ones reused); new ones only until ``deadline``
    (``clock`` time). Returns the responses that exist, in moment order."""
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    responses: list[Path] = []
    moments = sorted(set(seconds))
    made = 0
    for index, at in enumerate(moments, 1):
        window = directory / window_name(at)
        response = window / "response.json"
        if not response.is_file():
            if clock() >= deadline:
                continue
            made += 1
            if made % PROGRESS_EVERY == 1:
                progress(f"faces {directory.name} moment {index}/{len(moments)} ({made} new)")
            window.mkdir(mode=0o700, exist_ok=True)
            request = window / "request.json"
            request.write_text(json.dumps(window_request(source, at)), encoding="utf-8")
            partial = window / "response.partial"
            command = [str(native), "measure-media", "video_face_track", str(request.resolve())]
            if run([*command, f">{partial}"]) != 0 or not partial.is_file():
                raise ReplayError(f"Vision face window failed for {source.name} at {at:.3f} s")
            partial.rename(response)
        responses.append(response)
    progress(f"faces {directory.name} done: {len(responses)}/{len(moments)} ({made} new)")
    return responses


def covered(detected: Sequence[Corners], box: Sequence[float]) -> bool:
    """A detected caption box covers at least ``HIT_SHARE`` of the residual box."""
    left, top, right, bottom = (float(value) for value in box)
    area = max(1.0, (right - left) * (bottom - top))
    return any(
        max(0.0, min(right, d[2]) - max(left, d[0])) * max(0.0, min(bottom, d[3]) - max(top, d[1]))
        >= HIT_SHARE * area
        for d in detected
    )


def dhash(image: Image, box: Sequence[float]) -> int:
    """64-bit difference hash of the residual's band (its rows grown by one height each way)."""
    left, top, right, bottom = (int(value) for value in box)
    height = max(1, bottom - top)
    crop = image[max(0, top - height) : bottom + height, max(0, left) : max(left + 1, right)]
    gray = cv2.cvtColor(cv2.resize(crop, (9, 8)), cv2.COLOR_BGR2GRAY)
    bits = (gray[:, 1:] > gray[:, :-1]).flatten()
    return int(sum(1 << index for index, bit in enumerate(bits) if bit))


def screen(
    items: Sequence[tuple[float, Sequence[float]]],
    *,
    decode: Callable[[float], Image],
    detect: Callable[[list[Image]], list[list[Corners]]],
) -> tuple[list[float], list[float]]:
    """(misses, already detected): the moments the current model misses, near-identical ones
    (``dhash`` within ``HASH_DISTANCE`` bits of a kept one) dropped; the rest it detects."""
    misses: list[float] = []
    hits: list[float] = []
    hashes: list[int] = []
    for start in range(0, len(items), SCREEN_BATCH):
        batch = items[start : start + SCREEN_BATCH]
        images = [decode(at) for at, _ in batch]
        for (at, box), image, found in zip(batch, images, detect(images), strict=True):
            if covered(found, box):
                hits.append(at)
                continue
            code = dhash(image, box)
            if any(bin(code ^ other).count("1") <= HASH_DISTANCE for other in hashes):
                hits.append(at)  # a near-identical miss is already kept
                continue
            hashes.append(code)
            misses.append(at)
    return misses, hits


def decode_frame(source: Path, seconds: float, *, ffmpeg: str = "ffmpeg") -> Image:
    """One full-resolution BGR frame at ``seconds`` (a keyframe-accurate seek)."""
    result = subprocess.run(
        [
            ffmpeg,
            "-v",
            "error",
            "-ss",
            f"{seconds:.6f}",
            "-i",
            str(source),
            "-frames:v",
            "1",
            "-f",
            "image2pipe",
            "-vcodec",
            "png",
            "-",
        ],
        stdin=subprocess.DEVNULL,
        capture_output=True,
        check=False,
    )
    image = (
        cv2.imdecode(np.frombuffer(result.stdout, np.uint8), cv2.IMREAD_COLOR)
        if result.returncode == 0 and result.stdout
        else None
    )
    if image is None:
        raise ReplayError(f"Frame at {seconds:.3f} s of {source.name} could not be decoded")
    return np.asarray(image, dtype=np.uint8)


def idle_cleanup(directory: Path, *, sleep: Callable[[float], None] = time.sleep) -> None:
    """Wait while a cleanup chunk of any recording waits for memory (learning never delays it)."""
    waiting = gpu_lease.waiting(directory, MEMORY_QUEUE)
    paused = waiting > 0
    if paused:
        progress(f"paused: {waiting} cleanup chunks wait for memory")
    while waiting:
        sleep(LEASE_POLL_SECONDS)
        waiting = gpu_lease.waiting(directory, MEMORY_QUEUE)
    if paused:
        progress("resumed: no cleanup chunk waits for memory")


def glyph_set(
    source: Path,
    seconds: Sequence[float],
    output: Path,
    name: str,
    *,
    decode: Callable[[Path, float], Image],
) -> list[Path]:
    """An evaluation recording without face labels: each moment's frame at the training width and
    its caption lines from the caption pixels (``yolo_joint_dataset.caption_lines``)."""
    images, labels = output / "images" / name, output / "labels" / name
    images.mkdir(mode=0o700, parents=True, exist_ok=True)
    labels.mkdir(mode=0o700, parents=True, exist_ok=True)
    written: list[Path] = []
    for index, at in enumerate(sorted(set(seconds))):
        frame = decode(source, at)
        height, width = frame.shape[:2]
        scaled = np.asarray(
            cv2.resize(frame, (TRAIN_WIDTH, round(height * TRAIN_WIDTH / width))), dtype=np.uint8
        )
        stem = f"{name}_{index:07d}"
        path = images / f"{stem}.jpg"
        if not cv2.imwrite(str(path), scaled, [cv2.IMWRITE_JPEG_QUALITY, 92]):
            raise ReplayError("A glyph-only frame could not be written")
        rows = [
            yolo_row(
                CAPTION,
                PixelBox(float(a), float(b), float(c - a), float(d - b)),
                canvas=(scaled.shape[1], scaled.shape[0]),
            )
            for a, b, c, d in caption_lines(scaled)
        ]
        (labels / f"{stem}.txt").write_text("".join(f"{row}\n" for row in rows), "utf-8")
        written.append(path)
    return written


def has_face(response: Path) -> bool:
    """The Vision window holds at least one trusted (detected or tracked) face box."""
    frames: object = json.loads(response.read_text(encoding="utf-8"))
    return isinstance(frames, list) and any(
        isinstance(frame, dict)
        and frame.get("method") in ("detection", "tracking")
        and bool(frame.get("faces"))
        for frame in frames
    )


def dataset_attempt(cycle_dir: Path) -> Path:
    """The cycle's completed dataset (one with ``data.yaml``) to reuse, else a fresh attempt
    directory: an earlier attempt that stopped midway is kept as it is, never overwritten."""
    attempt = 1
    while (cycle_dir / f"dataset-{attempt}").exists():
        if (cycle_dir / f"dataset-{attempt}" / "data.yaml").is_file():
            return cycle_dir / f"dataset-{attempt}"
        attempt += 1
    return cycle_dir / f"dataset-{attempt}"


def tree_bytes(path: Path) -> int:
    """Bytes of the regular files under ``path`` (links are not followed)."""
    if not path.exists():
        return 0
    if path.is_file():
        return path.stat().st_size
    return sum(
        item.lstat().st_size for item in path.rglob("*") if item.is_file() and not item.is_symlink()
    )


def log_deletion(log: Path, path: Path, size: int, why: str) -> None:
    with log.open("a", encoding="utf-8") as stream:
        stream.write(
            f"- {time.strftime('%Y-%m-%d %H:%M')} yolo-learning: removed {path} "
            f"({size} bytes). Why: {why}\n"
        )


def remove(path: Path, log: Path, why: str) -> int:
    """Remove a file or tree and log it; returns the bytes freed."""
    size = tree_bytes(path)
    if path.is_dir() and not path.is_symlink():
        shutil.rmtree(path)
    elif path.exists() or path.is_symlink():
        path.unlink()
    log_deletion(log, path, size, why)
    return size


def prune_cycle(cycle_dir: Path, log: Path) -> int:
    """After a decision: drop the cycle's build products (datasets, runs, samples, lists), keeping
    decision.json, the eval metrics and the manifest."""
    keep = {"decision.json", "manifest.json"}
    freed = 0
    for item in sorted(cycle_dir.iterdir()):
        if item.name in keep or (item.name.startswith("eval-") and item.suffix == ".json"):
            continue
        freed += remove(item, log, "the learning cycle has decided; its metrics are kept")
    return freed


def prune_attempts(cycle_dir: Path, log: Path) -> int:
    """Half-built dataset attempts of an undecided cycle (no data.yaml), removed before the next
    attempt (the cycle lock guarantees no other cycle uses them)."""
    if not cycle_dir.is_dir() or (cycle_dir / "decision.json").is_file():
        return 0
    return sum(
        remove(item, log, "a stopped dataset build; the next attempt rebuilds it")
        for item in sorted(cycle_dir.glob("dataset*"))
        if item.is_dir() and not (item / "data.yaml").is_file()
    )


def prune_weights(models: Path, log: Path, keep: int = 2) -> int:
    """Keep the promoted learned weights and ``keep - 1`` earlier ones (rollback); the pinned
    weights live outside ``learned-*`` and are never touched."""
    current = ""
    if (models / PROMOTED).is_file():
        current = str(record(json.loads((models / PROMOTED).read_text("utf-8"))).get("weights", ""))
    folders = sorted(models.glob("learned-*"), key=lambda path: path.stat().st_mtime, reverse=True)
    kept = [path for path in folders if str(path / "weights.pt") == current][:1]
    kept += [path for path in folders if path not in kept][: max(0, keep - len(kept))]
    return sum(
        remove(path, log, "only the promoted and one earlier learned model are kept")
        for path in folders
        if path not in kept
    )


def bound_faces(faces: Path, cap: int, log: Path, protected: Sequence[str] = ()) -> int:
    """Keep at most ``cap`` face windows (the newest; regression ones never count or go)."""
    windows = sorted(
        (path for path in faces.glob("*/window-*") if path.parent.name not in protected),
        key=lambda path: path.stat().st_mtime,
        reverse=True,
    )
    return sum(
        remove(path, log, "the face-label cache is capped (newest kept)") for path in windows[cap:]
    )


def window_name(seconds: float) -> str:
    return f"window-{round(seconds * 1000):012d}"


def sampled(lines: Sequence[str], count: int) -> list[str]:
    """``count`` lines spread evenly over ``lines`` (all when fewer)."""
    if count >= len(lines):
        return list(lines)
    if count <= 0:
        return []
    return [lines[index * len(lines) // count] for index in range(count)]


def data_yaml(train: Sequence[Path], val: Path, output: Path) -> Path:
    lines = [
        f"path: {output.parent}",
        f"train: [{', '.join(str(path) for path in train)}]",
        f"val: {val}",
        "names:",
        "  0: avatar_face",
        "  1: source_caption",
    ]
    output.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return output


def frozen_sample(base: Path, names: Sequence[str], output: Path, count: int) -> list[Path]:
    """A small dataset of ``count`` frozen frames per recording (links into ``base``)."""
    images: list[Path] = []
    for name in names:
        chosen = sampled(sorted(str(p) for p in (base / "images" / name).glob("*.jpg")), count)
        for kind in ("images", "labels"):
            (output / kind / name).mkdir(mode=0o700, parents=True, exist_ok=True)
        for item in (Path(path) for path in chosen):
            image = output / "images" / name / item.name
            label_file = output / "labels" / name / f"{item.stem}.txt"
            if not image.exists():
                image.symlink_to(item)
                label_file.symlink_to(base / "labels" / name / f"{item.stem}.txt")
            images.append(image)
    return images


def decide(
    candidate: Mapping[str, object],
    pinned: Mapping[str, object],
    frozen: Sequence[str],
    regression: Sequence[str],
    glyph_only: Sequence[str] = (),
) -> tuple[bool, str]:
    """Promote only when every gate passes on the frozen and regression recordings (only the
    caption gate on the glyph-only ones: their frames show no face) and glyph coverage and face
    detection are at least the pinned model's on each frozen recording."""
    new, old = record(candidate.get("recordings")), record(pinned.get("recordings"))
    for name in (*frozen, *regression, *glyph_only):
        gate = "caption" if name in glyph_only else "joint"
        if record(record(new.get(name)).get("pass")).get(gate) is not True:
            return False, f"{name} fails a gate"
    for name in frozen:
        mine, theirs = record(new[name]), record(old.get(name))
        for kind, key in (("caption", "glyphCoverage"), ("face", "detectionRate")):
            have = float(str(record(mine.get(kind)).get(key)))
            need = float(str(record(theirs.get(kind)).get(key)))
            if have < need:
                return False, f"{name} {kind} {key} {have:.4f} < pinned {need:.4f}"
    return True, "meets the pinned bar"


def promote(best: Path, models: Path) -> Path:
    """Version the weights by digest beside the pinned ones and point ``promoted.json`` at them."""
    digest = _sha256(best)
    folder = models / f"learned-{digest[:12]}"
    folder.mkdir(mode=0o700, parents=True, exist_ok=True)
    weights = folder / "weights.pt"
    if not weights.exists():
        shutil.copy2(best, weights)
    (folder / "weights.pt.sha256").write_text(f"{digest}  weights.pt\n", encoding="utf-8")
    temporary = models / f".{PROMOTED}.tmp"
    temporary.write_text(json.dumps({"weights": str(weights), "sha256": digest}) + "\n", "utf-8")
    temporary.replace(models / PROMOTED)
    return weights


def _keys(config: Mapping[str, object]) -> dict[str, list[tuple[str, float]]]:
    """Per recording name: (moment key, seconds) of every dated confirmed moment."""
    found: dict[str, list[tuple[str, float]]] = {}
    for item in (record(value) for value in _items(config["recordings"])):
        name = str(item["name"])
        found[name] = [
            (f"{name}/{window_name(at)}", at) for _, at in moments([Path(str(item["store"]))])
        ]
    return found


def _state(work: Path) -> dict[str, object]:
    path = work / STATE
    return record(json.loads(path.read_text(encoding="utf-8"))) if path.is_file() else {}


def _trained(work: Path) -> set[str]:
    return {str(key) for key in _items(_state(work).get("trained", []))}


def due(config: Mapping[str, object]) -> bool:
    """Enough confirmed moments no cycle trained on yet, and learning is not braked."""
    work = Path(str(config["work"]))
    stores = [Path(str(record(item)["store"])) for item in _items(config["recordings"])]
    if any(cleanup_learning.braked(path) for path in stores):
        return False
    trained = _trained(work)
    new = sum(key not in trained for keys in _keys(config).values() for key, _ in keys)
    return new >= int(str(config.get("minRows", 200)))


def shell(arguments: Sequence[str]) -> int:
    """Run a command at nice 15; a trailing ``>FILE`` sends its standard output there."""
    *command, last = arguments
    target = last[1:] if last.startswith(">") else None
    command = [*command] if target is not None else [*command, last]
    with Path(target).open("wb") if target else open(os.devnull, "wb") as stdout:
        return subprocess.run(
            ["/usr/bin/nice", "-n", "15", *command],
            stdin=subprocess.DEVNULL,
            stdout=stdout if target else None,
            check=False,
        ).returncode


Screen = Callable[[Path, list[tuple[float, list[float]]]], tuple[list[float], list[float]]]


def screen_with(settings: YoloSettings, *, ffmpeg: str = "ffmpeg") -> Screen:
    """``screen`` with the given detector (loaded on first use) and full-resolution frames."""
    loaded: list[Callable[[list[Image]], list[list[Corners]]]] = []

    def detect(images: list[Image]) -> list[list[Corners]]:
        if not loaded:
            loaded.append(load_detector(settings))
        return loaded[0](images)

    def screening(
        source: Path, items: list[tuple[float, list[float]]]
    ) -> tuple[list[float], list[float]]:
        return screen(
            items, decode=lambda at: decode_frame(source, at, ffmpeg=ffmpeg), detect=detect
        )

    return screening


def cycle(
    config: Mapping[str, object],
    *,
    run: Run = shell,
    clock: Callable[[], float] = time.monotonic,
    screening: Screen | None = None,
    decode: Callable[[Path, float], Image] | None = None,
) -> dict[str, object]:
    """One incremental cycle (see the module text); returns and records its decision."""
    work = Path(str(config["work"]))
    work.mkdir(mode=0o700, parents=True, exist_ok=True)
    with (work / "cycle.lock").open("w") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise ReplayError("Another learning cycle is running") from error
        return _cycle(config, work, run, clock, screening, decode or decode_frame)


def _cycle(
    config: Mapping[str, object],
    work: Path,
    run: Run,
    clock: Callable[[], float],
    screening: Screen | None,
    decode: Callable[[Path, float], Image],
) -> dict[str, object]:
    python = str(config.get("python", sys.executable))
    native = Path(str(config["native"]))
    base = Path(str(config["baseDataset"]))
    pinned = record(config["pinned"])
    models = Path(str(config["models"]))
    stores = [record(item) for item in _items(config["recordings"])]
    state = _state(work)
    number = int(str(state.get("cycles", 0))) + 1
    cycle_dir = work / f"cycle-{number:04d}"
    cycle_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
    # Disk stays bounded: stopped builds go, then the pool cap, then the budget check.
    log = Path(str(config.get("deletionsLog", work / "deletions.md")))
    protected = [str(record(item)["name"]) for item in _items(config["regression"])]
    pool = int(str(config.get("poolMoments", POOL_MOMENTS)))
    prune_attempts(cycle_dir, log)
    for decided in sorted(work.glob("cycle-*")):
        if (decided / "decision.json").is_file() and any(
            item.name not in ("decision.json", "manifest.json")
            and not item.name.startswith("eval-")
            for item in decided.iterdir()
        ):
            prune_cycle(decided, log)
    bound_faces(work / "faces", pool, log, protected)
    used_bytes = tree_bytes(work) + sum(tree_bytes(path) for path in models.glob("learned-*"))
    cap = int(str(config.get("diskCapBytes", DISK_CAP_BYTES)))
    if used_bytes > cap:
        raise ReplayError(
            f"Learning disk budget exceeded after eviction: {used_bytes} > {cap} bytes"
        )
    seconds: dict[str, float] = {}
    started = clock()
    # 1-2. New moments only, their cached or new face windows within the labelling budget.
    trained = _trained(work)
    deadline = started + float(str(config.get("labelSeconds", 1800)))
    keys = _keys(config)
    entries: list[dict[str, object]] = []
    used: list[str] = []
    start = promoted(
        YoloSettings(
            Path(str(pinned["weights"])),
            str(pinned["sha256"]),
            Path(str(config["configDirectory"])),
            Path(str(config["checkoutWork"])),
            imgsz=int(str(config.get("imgsz", 640))),
        ),
        models,
        Path(str(stores[0]["store"])).parent if stores else work,
    )
    progress(f"cycle {number} start from {start.weights}")
    screener = screen_with(start) if screening is None else screening
    detected: list[str] = []
    idle_cleanup(gpu_lease.root())
    for item in stores:
        name = str(item["name"])
        boxes = {
            float(str(row["seconds"])): [float(str(v)) for v in _items(row["box"])]
            for row in cleanup_learning.training_frames(Path(str(item["store"])))
        }
        fresh = [(key, at) for key, at in keys.get(name, []) if key not in trained]
        times = _spread([at for _, at in fresh], int(str(config.get("maxMoments", 400))))
        if not times:
            continue
        # Train only on what the current model misses; near-identical misses count once.
        progress(f"screen {name}: {len(times)} new moments")
        misses, hits = screener(Path(str(item["source"])), [(at, boxes[at]) for at in times])
        progress(f"screen {name} done: {len(misses)} missed, {len(hits)} detected")
        detected += [f"{name}/{window_name(at)}" for at in hits]
        times = misses
        if not times:
            continue
        responses = label(
            Path(str(item["source"])),
            times,
            work / "faces" / name,
            native=native,
            run=run,
            deadline=deadline,
            clock=clock,
        )
        # Every labelled moment counts as trained; only those with a trusted face can be frames.
        used += [f"{name}/{response.parent.name}" for response in responses]
        faced = [response for response in responses if has_face(response)]
        if faced:
            entries.append(_entry(work, name, Path(str(item["source"])), faced, "train"))
    seconds["labels"] = round(clock() - started, 1)
    if not used:
        (work / STATE).write_text(
            json.dumps({"cycles": number - 1, "trained": sorted({*trained, *detected})}), "utf-8"
        )
        reason = "the model detects every new moment" if detected else "no labelled new moment"
        return {"skipped": reason, "detected": len(detected), "seconds": seconds}
    regressions = [record(item) for item in _items(config["regression"])]
    checked: list[str] = []
    unusable: list[str] = []
    if not any(entry["role"] == "train" for entry in entries):
        reason = "no new moment has a trusted face box"
        (work / STATE).write_text(
            json.dumps({"cycles": number - 1, "trained": sorted({*trained, *used, *detected})}),
            "utf-8",
        )
        return {"skipped": reason, "labelled": len(used), "seconds": seconds}
    glyphs = [str(item["name"]) for item in regressions if item.get("glyphOnly") is True]
    glyph_dir = cycle_dir / "glyph-sets"
    progress(f"regression sets: {len(regressions)}")
    for item in (item for item in regressions if item.get("glyphOnly") is True):
        glyph_set(
            Path(str(item["source"])),
            [float(str(v)) for v in _items(item["seconds"])],
            glyph_dir,
            str(item["name"]),
            decode=decode,
        )
    for item in (item for item in regressions if item.get("glyphOnly") is not True):
        responses = label(
            Path(str(item["source"])),
            [float(str(v)) for v in _items(item["seconds"])],
            work / "faces" / str(item["name"]),
            native=native,
            run=run,
        )
        faced = [response for response in responses if has_face(response)]
        if faced:
            entries.append(
                _entry(work, str(item["name"]), Path(str(item["source"])), faced, "test")
            )
            checked.append(str(item["name"]))
        else:
            unusable.append(str(item["name"]))  # no trusted face: no dataset frame (see decide)
    if not checked:
        raise ReplayError("No regression recording has a trusted face box; nothing to gate on")
    seconds["labels"] = round(clock() - started, 1)
    # 3. The new frames, a replay buffer of the base training frames and a small validation set.
    mark = clock()
    dataset = dataset_attempt(cycle_dir)
    manifest = cycle_dir / "manifest.json"
    manifest.write_text(json.dumps({"output": str(dataset), "recordings": entries}), "utf-8")
    progress(f"dataset build {dataset.name}")
    if not (dataset / "data.yaml").is_file() and run(
        [python, "-B", "-m", "replay.yolo_joint_dataset", str(manifest)]
    ):
        raise ReplayError("The learning dataset could not be built")
    new_train = (
        (dataset / "train.txt").read_text("utf-8").split()
        if (dataset / "train.txt").is_file()
        else []
    )
    base_train = (base / "train.txt").read_text("utf-8").split()
    replay = cycle_dir / "replay.txt"
    replay.write_text(
        "".join(
            f"{line}\n"
            for line in sampled(
                base_train, min(len(new_train), int(REPLAY_SHARE * len(base_train)))
            )
        ),
        "utf-8",
    )
    frozen = [str(name) for name in _items(config["frozen"])]
    sample_dir = cycle_dir / "frozen-sample"
    sample = frozen_sample(base, frozen, sample_dir, SAMPLE_FRAMES)
    regression_images = sorted(
        str(p) for n in checked for p in (dataset / "images" / n).glob("*.jpg")
    )
    val = cycle_dir / "val.txt"
    val.write_text(
        "".join(f"{line}\n" for line in [*regression_images, *map(str, sample)]), "utf-8"
    )
    data = data_yaml([dataset / "train.txt", replay], val, cycle_dir / "data.yaml")
    seconds["build"] = round(clock() - mark, 1)
    progress(f"dataset done: {len(new_train)} new, {len(sample)} frozen-sample frames")
    # 4. A frozen-backbone update from the current weights.
    mark = clock()
    idle_cleanup(gpu_lease.root())
    best = cycle_dir / "runs" / "train" / "weights" / "best.pt"
    common = [
        "--config-directory",
        str(config["configDirectory"]),
        "--work",
        str(config["checkoutWork"]),
    ]
    if not best.is_file():
        progress("train waiting for memory and the GPU lease")
        with memory_admission.reserve(TRAIN_JOB, "yolo-learning"):
            waiter = gpu_lease.Waiter(gpu_lease.root(), "yolo-learning", step="yolo train")
            held = waiter.try_acquire()
            while held is None:
                time.sleep(LEASE_POLL_SECONDS)
                held = waiter.try_acquire()
            progress(f"train start ({config.get('epochs', 5)} epochs)")
            try:
                code = run(
                    [
                        python,
                        "-B",
                        "-m",
                        "replay.yolo_train",
                        str(data),
                        str(cycle_dir / "runs"),
                        "--weights",
                        str(start.weights),
                        "--weights-sha256",
                        start.sha256,
                        *common,
                        "--name",
                        "train",
                        "--epochs",
                        str(config.get("epochs", 5)),
                        "--imgsz",
                        str(config.get("imgsz", 640)),
                        "--batch",
                        str(config.get("batch", 8)),
                        "--patience",
                        "2",
                        "--cache",
                        "ram",
                        "--freeze",
                        str(FREEZE_LAYERS),
                    ]
                )
            finally:
                gpu_lease.release(held)
                waiter.close()
            if code != 0:
                raise ReplayError("yolo_train failed; its run directory is kept")
    seconds["train"] = round(clock() - mark, 1)
    progress(f"train done in {seconds['train']} s")
    # 5. Cheap stage first, the full frozen recordings only for a candidate that passed it.
    mark = clock()

    def evaluate(
        tag: str, weights: Path, directory: Path, names: Sequence[str]
    ) -> dict[str, object]:
        out = cycle_dir / f"eval-{tag}.json"
        progress(f"eval {tag}")
        if not out.is_file() and run(
            [
                python,
                "-B",
                "-m",
                "replay.yolo_joint_eval",
                str(weights),
                str(directory),
                str(out),
                *[a for n in names for a in ("--recording", n)],
                *common,
                "--imgsz",
                str(config.get("imgsz", 640)),
            ]
        ):
            raise ReplayError(f"yolo_joint_eval failed for {tag}")
        return record(record(json.loads(out.read_text(encoding="utf-8"))).get("recordings"))

    first = {
        **evaluate("regression", best, dataset, checked),
        **(evaluate("glyph-only", best, glyph_dir, glyphs) if glyphs else {}),
        **evaluate("frozen-sample", best, sample_dir, frozen),
    }
    failed = [
        name
        for name, result in first.items()
        if record(record(result).get("pass")).get("caption" if name in glyphs else "joint")
        is not True
    ]
    outcome: dict[str, object] = {
        "cycle": number,
        "moments": len(used),
        "candidate": str(best),
        "startWeights": str(start.weights),
        "regressionWithoutFaces": unusable,
    }
    if failed:
        accepted, reason = False, f"first stage fails on {', '.join(sorted(failed))}"
    else:
        full = {
            **evaluate("frozen", best, base, frozen),
            **evaluate("regression", best, dataset, checked),
            **(evaluate("glyph-only", best, glyph_dir, glyphs) if glyphs else {}),
        }
        pinned_eval = record(json.loads(Path(str(pinned["eval"])).read_text(encoding="utf-8")))
        accepted, reason = decide({"recordings": full}, pinned_eval, frozen, checked, glyphs)
    seconds["eval"] = round(clock() - mark, 1)
    seconds["total"] = round(clock() - started, 1)
    outcome |= {"accepted": accepted, "reason": reason, "seconds": seconds}
    if accepted and not any(cleanup_learning.braked(Path(str(i["store"]))) for i in stores):
        outcome["promoted"] = str(promote(best, models))
    (cycle_dir / "decision.json").write_text(json.dumps(outcome, indent=1), "utf-8")
    progress(f"decision: {'accepted' if accepted else 'rejected'} ({reason})")
    (work / STATE).write_text(
        json.dumps({"cycles": number, "trained": sorted({*trained, *used, *detected})}), "utf-8"
    )
    prune_cycle(cycle_dir, log)
    prune_weights(models, log)
    bound_faces(work / "faces", pool, log, protected)
    return outcome


def _spread(times: Sequence[float], limit: int) -> list[float]:
    """At most ``limit`` moments, evenly spaced over the sorted ones (labelling costs ~3 s each)."""
    ordered = sorted(times)
    if len(ordered) <= limit:
        return ordered
    return [ordered[round(index * (len(ordered) - 1) / (limit - 1))] for index in range(limit)]


def _items(value: object) -> list[object]:
    if not isinstance(value, list):
        raise ReplayError("The learning configuration needs a list here")
    return value


def _entry(
    work: Path, name: str, source: Path, responses: Sequence[Path], role: str
) -> dict[str, object]:
    listing = work / "faces" / f"{name}.txt"
    listing.write_text("".join(f"{path.resolve()}\n" for path in responses), encoding="utf-8")
    return {
        "name": name,
        "source": str(source),
        "faceResponses": str(listing),
        "step": DATASET_STEP,
        "role": role,
    }


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("cycle", "due"))
    parser.add_argument("config", type=Path)
    args = parser.parse_args(argv)
    config = record(json.loads(args.config.read_text(encoding="utf-8")))
    if args.action == "due":
        return 0 if due(config) else 1
    print(json.dumps(cycle(config)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
