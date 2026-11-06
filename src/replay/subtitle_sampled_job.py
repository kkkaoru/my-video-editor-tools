"""One-chunk job: sampled-OCR repair with real collaborators, audit gate and receipt.

``python -m replay.subtitle_sampled_job CONFIG.json`` reads a bounded JSON description of
one source chunk (clock, canvas, source, profile observations, native tool, pinned detector
artifacts), runs ``subtitle_sampled_run`` and prints the accepted attempt. Existing outputs
are never replaced; a failed audit after escalation leaves a receipt and a nonzero exit.
"""

import argparse
import glob
import json
import os
import sys
import threading
import time
from collections.abc import Callable, Iterator, Mapping, Sequence
from concurrent.futures import Future, ThreadPoolExecutor
from fractions import Fraction
from functools import partial
from math import ceil, floor
from pathlib import Path
from typing import Protocol

import numpy as np

from replay import cleanup_learning, subtitle_sttn, yolo_boxes, yolo_learning
from replay import subtitle_band_audit as band_audit
from replay import subtitle_colour_audit as colour_audit
from replay import subtitle_colour_scene as scene
from replay import subtitle_colour_support as colour
from replay import subtitle_detector_mps as mps
from replay import subtitle_sampled_chunk as chunk
from replay import subtitle_sampled_run as runner
from replay import subtitle_yolo_admission as yolo_plan
from replay import subtitle_yolo_regions as yolo
from replay.caption_io import FrameObservation, native_ocr_frames
from replay.caption_masks import TextRectangle
from replay.local_asr import fingerprint
from replay.recordings import record
from replay.subtitle_cleanup_plan import CaptionSearchProfile, search_profile
from replay.subtitle_clock import VideoFrameClock
from replay.subtitle_colour_residual import GATE
from replay.subtitle_inpaint import Image
from replay.subtitle_plan import repair_spans
from replay.subtitle_profile import (
    audited_interface,
    japanese_letters,
    profiled_detections,
    still_label_reads,
)
from replay.subtitle_region_fill import Region
from replay.subtitle_repair_audit import MIN_CONFIDENCE, Residual
from replay.subtitle_stream import SubtitleSpan
from replay.subtitle_sttn import refill_stream
from replay.subtitle_video import VideoRepairJob
from replay.subtitle_video_fast import (
    Schedule,
    SupportStore,
    render_refilled_video,
    render_repaired_video_fast,
)
from replay.yolo_track import prefetched

MAX_CONFIG_BYTES = 8 * 1024 * 1024
RENDER_BATCH = 16
COLOUR_KEY_PIN = "pink-key-v1"
LINE_HEIGHTS = 2  # a planned box taller than this many residual heights is a region, not its line


class Detects(Protocol):
    def detect(self, images: Sequence[Image]) -> tuple[Image, ...]: ...


def _span_origin(spans: Sequence[SubtitleSpan], index: int) -> tuple[int, int]:
    for span in spans:
        if span.start_frame <= index < span.end_frame:
            return span.x, span.y
    raise ValueError(f"Active frame {index} lies in no repair span")


def _rectangle_box(box: TextRectangle) -> tuple[float, float, float, float]:
    return box.x, box.y, box.width, box.height


def _corner_box(corners: yolo.Corners) -> colour.Box:
    """A YOLO box's float corners as the whole pixels it touches."""
    return (floor(corners[0]), floor(corners[1]), ceil(corners[2]), ceil(corners[3]))


def _rectangle_corners(box: TextRectangle) -> colour.Box:
    """An OCR line's rectangle as the whole pixels it touches (left, top, right, bottom)."""
    return (floor(box.x), floor(box.y), ceil(box.x + box.width), ceil(box.y + box.height))


def _entry_boxes(entry: object) -> list[colour.Box]:
    raw = record(entry).get("lineBoxes", [])
    if not isinstance(raw, list):
        raise ValueError("Admission line boxes must be a list")
    return [(int(b[0]), int(b[1]), int(b[2]), int(b[3])) for b in raw]


def key_frames(
    admitted: Mapping[str, object], spans: Sequence[SubtitleSpan]
) -> list[colour.FrameBoxes]:
    """Per active frame in render order: ROI origin and the recognised line boxes."""
    entries = admitted["frames"]
    if not isinstance(entries, list):
        raise ValueError("Admission lacks per-frame entries")
    frames: list[colour.FrameBoxes] = []
    for index, raw in enumerate(entries):
        entry = record(raw)
        if entry.get("classification") != "overlay-subtitle":
            continue
        if "lineBoxes" not in entry:
            raise ValueError("Admission lacks recognised line boxes for colour keying")
        boxes = _entry_boxes(entry)
        region = entry.get("region")
        if not boxes and isinstance(region, dict):
            # The detector found a caption that OCR could not read (a lone digit): key its region.
            x, y = int(region["x"]), int(region["y"])
            boxes = [(x, y, x + int(region["width"]), y + int(region["height"]))]
        frames.append(colour.FrameBoxes(_span_origin(spans, index), tuple(boxes)))
    return frames


CAPTION_OPEN = "<＜《"  # how OCR reads the caption frame's << (also as く right before text)
CAPTION_CLOSE = ">＞》"


def _caption_styled(text: str, share: float) -> bool:
    """A below-floor line is a caption: clearly outlined, or outlined and framed or long."""
    letters = "".join(text.split())
    framed = (
        letters[:1] in CAPTION_OPEN
        or letters[:2] in ("くく", "く3")
        or letters[-1:] in CAPTION_CLOSE
    )
    return share >= colour_audit.LOW_CONFIDENCE_OUTLINED_SHARE or (
        share >= colour_audit.FRAMED_OUTLINED_SHARE
        and (framed or len(letters) >= colour_audit.FRAMED_MIN_LETTERS)
    )


def _interface_boxes(
    observed: Mapping[int, FrameObservation],
    frame: int,
    excluded: list[list[colour.Box]],
    interface: Callable[[Mapping[int, FrameObservation], int, int], bool],
) -> bool:
    """Fill ``excluded[frame]`` with the frame's interface boxes grown by their height, except
    any that another line of the frame overlaps; True when any remains."""
    lines = observed[frame].detections
    flags = [interface(observed, frame, index) for index in range(len(lines))]
    others = [line.rectangle for line, flag in zip(lines, flags, strict=True) if not flag]
    for line, flag in zip(lines, flags, strict=True):
        if not flag:
            continue
        box = line.rectangle
        grown = (
            int(box.x - box.height),
            int(box.y - box.height),
            int(box.x + box.width + box.height + 1),
            int(box.y + 2 * box.height + 1),
        )
        if not any(
            other.x < grown[2] and grown[0] < other.x + other.width
            and other.y < grown[3] and grown[1] < other.y + other.height
            for other in others
        ):  # fmt: skip
            excluded[frame].append(grown)
    return bool(excluded[frame])


def _same_row(first: colour.Box, second: colour.Box) -> bool:
    """The boxes overlap horizontally and by half the shorter height vertically, and ``second``
    is no taller than ``LINE_HEIGHTS`` of ``first``: one text line, not a whole region keyed as one
    box (a frame whose detector region had no recognised line)."""
    rows = min(first[3], second[3]) - max(first[1], second[1])
    shorter = min(first[3] - first[1], second[3] - second[1])
    return (
        first[0] < second[2]
        and second[0] < first[2]
        and rows * 2 >= shorter > 0
        and second[3] - second[1] <= LINE_HEIGHTS * (first[3] - first[1])
    )


def persistence(
    frame: int,
    count: int,
    stride: int,
    present: Sequence[bool],
) -> tuple[int, int]:
    """The frames a residual caption line persists over in the source: from its frame, every
    ``stride``-th frame forward and backward while ``present`` (caption colour in its box, one
    flag per frame of ``range(0, count, stride)``) holds,
    plus one stride beyond each end, within the chunk. Healing only the audited neighbours left
    the rest of a long caption unmasked, and the refill copied it back from there."""
    grid = list(range(0, count, stride))
    flags = dict(zip(grid, present, strict=True))
    anchor = min(grid, key=lambda value: abs(value - frame)) if grid else frame
    low = high = anchor
    while low - stride >= 0 and flags.get(low - stride, False):
        low -= stride
    while high + stride < count and flags.get(high + stride, False):
        high += stride
    first = max(0, min(low, frame) - stride)
    last = min(count - 1, max(high, frame) + stride)
    return first, last


def line_extent(box: colour.Box, entries: Sequence[object], frame: int, reach: int) -> colour.Box:
    """The residual ``box`` grown to the planned caption line it lies on.

    OCR of a repaired frame reads only what survived (a fragment such as "Thar" of a translation
    line); healing that fragment alone leaves the rest of the line. The planned line boxes of the
    frames within ``reach`` on the same row as the fragment give the whole line's extent.
    """
    lines = [
        other
        for index in range(max(0, frame - reach), min(len(entries), frame + reach + 1))
        for other in _entry_boxes(entries[index])
        if _same_row(box, other)
    ]
    return (
        min(b[0] for b in (box, *lines)),
        min(b[1] for b in (box, *lines)),
        max(b[2] for b in (box, *lines)),
        max(b[3] for b in (box, *lines)),
    )


def with_residual(
    admitted: dict[str, object],
    boxes: Sequence[Sequence[yolo.Corners]],
    extra: Mapping[int, Sequence[colour.Box]],
    clock: VideoFrameClock,
) -> list[list[yolo.Corners]]:
    """Add residual boxes to the planned YOLO boxes and line boxes of the chunk's frames.

    A residual line beyond a frame's caption region (a tall block whose upper lines lie above the
    profiled band) widens that region and its mask too, or the repair would stay clipped to it.
    A frame the plan left unknown (no caption found there) becomes a caption frame with the
    residual boxes as its lines and its own one-frame mask, so the audit's findings are repaired
    where the plan missed them. Indices outside the chunk are ignored.
    """
    entries, masks = admitted["frames"], admitted.get("masks")
    if not isinstance(entries, list) or not isinstance(masks, list):
        raise ValueError("Admission lacks per-frame entries")
    # A promoted frame keeps the plan's own record of whether OCR read it (eligibility checks it).
    sampling = admitted.get("sampling")
    frames_read = sampling.get("sampledFrames") if isinstance(sampling, dict) else None
    read = set(frames_read) if isinstance(frames_read, list) else set()
    merged = [list(frame) for frame in boxes]
    for index, added in sorted(extra.items()):
        if not 0 <= index < len(entries):
            continue
        overlays = [
            position
            for position, entry in enumerate(entries)
            if record(entry).get("classification") == "overlay-subtitle"
        ]
        if index not in overlays:
            _promote(entries, masks, index, added, clock, sampled=index in read)
            overlays = sorted([*overlays, index])
        entry = record(entries[index])
        entry["lineBoxes"] = [*_entry_boxes(entry), *(list(box) for box in added)]
        regions = [record(entry["region"]), record(record(masks[overlays.index(index)])["region"])]
        for region in regions:
            _widen(region, added)
        if index < len(merged):
            merged[index] += [(float(a), float(b), float(c), float(d)) for a, b, c, d in added]
    # Promoting an unknown frame and widening regions changes which frames the plan leaves
    # unmodified; the stored list must describe the plan the audit and eligibility will read.
    admitted["unknownSourcePTS"] = [
        _frames(record(entry)["sourcePTS"])
        for entry in entries
        if record(entry).get("classification") == "unknown"
    ]
    return merged


def _frames(value: object) -> int:
    """A whole non-negative frame index from a stored plan row."""
    if type(value) is not int or value < 0:
        raise ValueError("Unknown source PTS rows carry whole frame indices")
    return value


def _promote(
    entries: list[object],
    masks: list[object],
    index: int,
    boxes: Sequence[colour.Box],
    clock: VideoFrameClock,
    *,
    sampled: bool,
) -> None:
    """Make an unknown frame a caption frame over ``boxes`` with its own mask (in frame order);
    ``sampled`` is whether the plan's OCR read the frame."""
    first = boxes[0]
    region: dict[str, object] = {
        "x": first[0], "y": first[1], "width": first[2] - first[0], "height": first[3] - first[1],
    }  # fmt: skip
    entry = record(entries[index])
    entry.update(classification="overlay-subtitle", region=region, lineBoxes=[], sampled=sampled)
    terminal = clock.pts[-1] + clock.last_duration
    following = clock.pts[index + 1] if index + 1 < len(clock.pts) else terminal
    before = sum(
        1
        for position in range(index)
        if record(entries[position]).get("classification") == "overlay-subtitle"
    )
    masks.insert(
        before,
        {
            "startSeconds": float((clock.pts[index] - clock.pts[0]) * clock.time_base),
            "endSeconds": float((following - clock.pts[0]) * clock.time_base),
            "region": dict(region),
        },
    )


def _widen(region: dict[str, object], boxes: Sequence[colour.Box]) -> None:
    """Grow an {x, y, width, height} region in place to cover every box and one line beyond.

    The extra line height above and below lets the colour key's stacked-line rule reach a
    further caption line of the same block that OCR did not report.
    """
    x, y = int(str(region["x"])), int(str(region["y"]))
    right, bottom = x + int(str(region["width"])), y + int(str(region["height"]))
    left = min([x, *(box[0] for box in boxes)])
    top = min([y, *(box[1] - (box[3] - box[1]) for box in boxes)])
    right = max([right, *(box[2] for box in boxes)])
    bottom = max([bottom, *(box[3] + (box[3] - box[1]) for box in boxes)])
    region.update(x=left, y=top, width=right - left, height=bottom - top)


def box_frames(
    admitted: Mapping[str, object],
    spans: Sequence[SubtitleSpan],
    boxes: Sequence[Sequence[yolo.Corners]],
) -> list[yolo.PlannedBoxes]:
    """Per active frame in render order: ROI origin and that frame's YOLO caption boxes."""
    entries = admitted["frames"]
    if not isinstance(entries, list) or len(entries) != len(boxes):
        raise ValueError("YOLO boxes do not match the admitted frames")
    return [
        yolo.PlannedBoxes(_span_origin(spans, index), tuple(boxes[index]))
        for index, raw in enumerate(entries)
        if record(raw).get("classification") == "overlay-subtitle"
    ]


def _clock(value: object) -> VideoFrameClock:
    data = record(value)
    pts = data.get("pts")
    if not isinstance(pts, list) or not all(type(item) is int for item in pts):
        raise ValueError("Invalid source clock")
    base, last = data.get("timeBase"), data.get("lastDuration")
    if not isinstance(base, str) or type(last) is not int:
        raise ValueError("Invalid source clock")
    return VideoFrameClock(tuple(int(item) for item in pts), Fraction(base), last)


def _profile_frames(directory: Path) -> tuple[FrameObservation, ...]:
    files = sorted(glob.glob(str(directory / "*.result.json")))
    if not files:
        raise ValueError("Missing subtitle profile observations")
    return tuple(
        frame
        for name in files
        for frame in native_ocr_frames(json.loads(Path(name).read_text(encoding="utf-8")))
    )


def _profile(directory: Path) -> CaptionSearchProfile:
    return search_profile(_profile_frames(directory))


def caption_style(
    frames: tuple[FrameObservation, ...],
    profile: CaptionSearchProfile,
    *,
    source: Path,
    ffmpeg: str,
    cache: Path,
) -> dict[str, object]:
    """The recording's caption style from its profile's Japanese caption lines in the source,
    measured once and kept in ``cache`` (written atomically; a concurrent writer is harmless)."""
    if cache.exists():
        return record(json.loads(cache.read_text(encoding="utf-8")))
    lines = [
        (frame.actual_time, (line.rectangle.x, line.rectangle.y, line.rectangle.width,
                             line.rectangle.height))
        for frame in frames
        for line in profiled_detections(frame, profile.bands)
        if japanese_letters(line.text)
    ]  # fmt: skip
    canvas = (profile.bands.width, profile.bands.height)
    shares = (
        colour_audit.line_shares(ffmpeg=ffmpeg, source=source, lines=lines, size=canvas)
        if lines
        else []
    )
    ordered = sorted(shares)
    style: dict[str, object] = {
        "outlined": colour_audit.outlined_style(shares),
        "lines": len(shares),
        "medianOutlinedShare": round(ordered[len(ordered) // 2], 4) if ordered else 0.0,
    }
    staged = cache.with_name(f".{cache.name}.{os.getpid()}")
    staged.write_text(json.dumps(style) + "\n", encoding="utf-8")
    os.replace(staged, cache)
    return style


def real_collaborators(
    config: Mapping[str, object],
    *,
    network: mps.Network,
    render: Callable[[VideoRepairJob, Detects], object] | None = None,
    caption_detector: yolo.Detector | None = None,
    caption_masks: bool = False,
    yolo_admission: bool = False,
    sttn: subtitle_sttn.Inpainter | None = None,
    stored_boxes: Sequence[Sequence[yolo.Corners]] | None = None,
) -> tuple[runner.Collaborators, dict[str, str]]:
    """Build the planning, rendering and auditing collaborators from one chunk description."""
    source, native = Path(str(config["source"])), Path(str(config["native"]))
    clock = _clock(config["clock"])
    canvas = (int(str(config["width"])), int(str(config["height"])))
    root = Path(str(config["checkoutRoot"]))
    state_root = Path(str(config["output"]))
    media_root = Path(str(config.get("mediaOutput", config["output"])))
    profile = _profile(Path(str(config["profileDirectory"])))
    envelope = profile.measured_envelope
    band = chunk.ocr_band(envelope, canvas)
    radius = int(str(config.get("radius", 6)))
    workers = int(str(config.get("workers", 2)))
    # Source-frame masks the line checks share: each audited source frame is decoded once.
    source_masks = colour_audit.SourceMasks(
        str(config.get("ffmpeg", "ffmpeg")), source, float(clock.pts[0] * clock.time_base), canvas
    )

    if (caption_masks or yolo_admission) and caption_detector is None and stored_boxes is None:
        raise ValueError("YOLO repair masks and admission need the YOLO caption detector")
    planned_boxes: list[list[yolo.Corners]] = []
    extra: dict[int, list[colour.Box]] = {}
    # Learning is read at this chunk's start (the chunk boundary): a store, model or code
    # replaced since applies from here, and the receipt records which versions this chunk used.
    learning_work = state_root.parent.parent
    braked = cleanup_learning.braked(learning_work)
    learned = [] if braked else cleanup_learning.bands(learning_work, exclude=state_root.name)
    seeded: list[int] = []
    learning = {
        "store": cleanup_learning.store_version(learning_work),
        "code": cleanup_learning.code_version(Path(cleanup_learning.__file__).parent),
        "model": str(config.get("regionDetector", "")),
        "braked": braked,
    }
    learning["version"] = "/".join(str(learning[key]) for key in ("store", "code", "model"))

    def residual(lines: Sequence[Residual], audit_stride: int) -> None:
        # A residual caption persists beyond the audited frames: cover every frame its source
        # still shows caption colour in its box (one decode of the probe grid for all lines).
        boxes: list[tuple[int, colour.Box]] = []
        for line in lines:
            if line.box is None:
                continue
            x, y, w, h = line.box
            box = (int(x), int(y), int(np.ceil(x + w)), int(np.ceil(y + h)))
            planned = next(iter(rendered.values()), {}).get("frames")
            if isinstance(planned, list):
                box = line_extent(box, planned, line.frame, audit_stride)
            boxes.append((line.frame, box))
        grid = list(range(0, len(clock.pts), audit_stride))
        flags = caption_present(sorted({box for _, box in boxes}), grid) if boxes else {}
        for frame, box in boxes:
            first, last = persistence(frame, len(clock.pts), audit_stride, flags[box])
            for index in range(first, last + 1):
                extra.setdefault(index, []).append(box)

    def caption_present(
        boxes: Sequence[colour.Box], frames: Sequence[int]
    ) -> dict[colour.Box, list[bool]]:
        """Per box: whether each source frame holds caption colour inside it."""
        counts = colour_audit.source_pink_counts(
            ffmpeg=str(config.get("ffmpeg", "ffmpeg")),
            source=source,
            source_start=float(clock.pts[0] * clock.time_base),
            lines=[
                (frame, (box[0], box[1], box[2] - box[0], box[3] - box[1]))
                for box in boxes
                for frame in frames
            ],
            size=canvas,
            masks=source_masks,
        )
        found = [count >= colour_audit.MINIMUM_CAPTION_PINK for count in counts]
        return {box: found[n * len(frames) : (n + 1) * len(frames)] for n, box in enumerate(boxes)}

    def regions() -> list[Region | None]:
        if stored_boxes is not None:
            # yolo-track's pass already detected these frames (the same boxes as below).
            planned_boxes[:] = [list(frame) for frame in stored_boxes]
            return yolo.yolo_regions(planned_boxes, canvas=canvas, envelope=envelope)
        if caption_detector is None:
            return chunk.detector_regions(
                source, clock, canvas=canvas, envelope=envelope, network=network
            )
        # Decoding runs one batch ahead in a thread so it overlaps detection (900 frames: 2.7 s
        # -> 2.0 s); a deeper buffer only holds more 2 MiB frames since decoding is the faster.
        frames = prefetched(
            yolo.chunk_frames(
                source, clock, canvas=canvas, ffmpeg=str(config.get("ffmpeg", "ffmpeg"))
            ),
            depth=yolo.BATCH,
        )
        planned_boxes[:] = yolo.chunk_boxes(
            frames, caption_detector, count=len(clock.pts), scale=yolo.canvas_scale(canvas)
        )
        return yolo.yolo_regions(planned_boxes, canvas=canvas, envelope=envelope)

    def plan(stride: int, directory: Path) -> dict[str, object]:
        if yolo_admission and stride != runner.FULL_STRIDE:
            # The escalation attempt (stride 1) keeps the full-OCR plan as the fallback.
            started = time.monotonic()
            regions()
            admitted = yolo_plan.admission(
                boxes=planned_boxes, clock=clock, profile=profile, radius=radius
            )
            admitted["stageSeconds"] = {"yoloBoxes": round(time.monotonic() - started, 3)}
            return admitted
        sampled_at = directory / "ocr"
        return chunk.plan_chunk(
            clock=clock,
            profile=profile,
            stride=stride,
            radius=radius,
            triggers=lambda: chunk.band_triggers(
                source, clock, canvas_width=canvas[0], envelope=envelope
            ),
            ocr=lambda indices: chunk.session_ocr(
                native,
                source,
                clock,
                indices,
                canvas=canvas,
                work=sampled_at,
                root=root,
                workers=workers,
                band=band,
            ),  # fmt: skip
            regions=regions,
        )

    def render_attempt(admitted: dict[str, object], directory: Path) -> Path:
        # Residual bands earlier chunks of this recording confirmed widen this plan's repair.
        frames = admitted.get("frames")
        added = cleanup_learning.seeds(frames if isinstance(frames, list) else [], learned)
        seeded.append(len(added))  # per render: how many frames the learned bands widened
        boxes_by_frame = {index: [*extra.get(index, []), *added.get(index, [])]
                          for index in {*extra, *added}}  # fmt: skip
        boxes = (
            with_residual(admitted, planned_boxes, boxes_by_frame, clock)
            if boxes_by_frame
            else planned_boxes
        )
        (directory / "admission.json").write_text(json.dumps(admitted), encoding="utf-8")
        masks = admitted["masks"]
        spans = repair_spans(
            masks, frames=len(clock.pts), width=canvas[0], height=canvas[1], source_clock=clock
        )
        media = media_root / directory.name
        media.mkdir(parents=True, exist_ok=True, mode=0o700)
        video = media / "candidate.mp4"
        job = VideoRepairJob(
            source, video, canvas[0], canvas[1], len(clock.pts), spans, source_clock=clock,
            hardware_encode=sttn is not None, hardware_hevc=False, context_frames=0,
            context_donors_verified=False, decode_log=directory / "decode.log",
            ramp_start_frame=0, ramp_total_frames=None,
        )  # fmt: skip
        boxed: yolo.BoxMaskDetector | None = None
        detector: Detects
        if caption_masks:
            boxed = yolo.BoxMaskDetector(box_frames(admitted, spans, boxes))
            detector = boxed
        else:
            detector = mps.MpsTextDetector(network, max_batch=RENDER_BATCH)
        keyed: colour.ColourKeyDetector | None = None
        if config.get("colourKey") is True:
            planned = key_frames(admitted, spans)
            keyed = colour.ColourKeyDetector(detector.detect, planned)
            detector = keyed
        if render is not None:
            render(job, detector)
        elif sttn is not None:
            # Find the exact repair pixels with the usual renderer, then refill them with STTN.
            kept = (
                max(0, min((s.y for s in spans), default=0) - subtitle_sttn.MAX_SURROUNDING),
                min(
                    canvas[1],
                    max((s.y + s.height for s in spans), default=0) + subtitle_sttn.MAX_SURROUNDING,
                ),
            )
            store = SupportStore(canvas[0], kept)
            started = time.monotonic()
            render_repaired_video_fast(
                job,
                detect_batch=detector.detect,
                schedule=Schedule(workers=int(str(config.get("repairWorkers", 4))), window=32,
                                  detect_chunk=RENDER_BATCH),
                support_sink=store,
            )  # fmt: skip
            painter = sttn

            def refill(bands: Iterator[Image], rows: tuple[int, int]) -> Iterator[Image]:
                return refill_stream(
                    bands, store.bands(*rows), painter, heights=subtitle_sttn.REFILL_HEIGHTS
                )

            repaired = time.monotonic()
            render_refilled_video(job, store, refill, surroundings=subtitle_sttn.MAX_SURROUNDING)
            # Split of the render between the mask repair pass and the STTN refill pass.
            (directory / "render-seconds.json").write_text(
                json.dumps({"repair": round(repaired - started, 3),
                            "sttnRefill": round(time.monotonic() - repaired, 3)}),
                encoding="utf-8",
            )  # fmt: skip
        else:
            render_repaired_video_fast(
                job,
                detect_batch=detector.detect,
                schedule=Schedule(workers=int(str(config.get("repairWorkers", 4))), window=32,
                                  detect_chunk=RENDER_BATCH),
            )  # fmt: skip
        if keyed is not None and keyed.consumed != len(planned):
            raise ValueError("Colour-key frames were not all consumed by the renderer")
        if boxed is not None and boxed.consumed != len(box_frames(admitted, spans, boxes)):
            raise ValueError("YOLO mask frames were not all consumed by the renderer")
        rendered.clear()
        rendered[video] = admitted  # the plan the colour gate counts this video's boxes from
        return video

    relative = VideoFrameClock(
        tuple(pts - clock.pts[0] for pts in clock.pts), clock.time_base, clock.last_duration
    )

    audit_tile = config.get("auditTile")
    if audit_tile is not None and (type(audit_tile) is not int or not 1 <= audit_tile <= 4):
        raise ValueError("auditTile must be an integer from 1 to 4")

    audited: dict[tuple[Path, tuple[int, ...]], list[Image]] = {}
    # The audited candidate's bands and the source's (decoded ahead), for the later recounts.
    band_store: dict[Path, tuple[tuple[int, ...], list[Image], Future[list[Image]] | None]] = {}

    def stored_bands(
        video: Path, frames: Sequence[int]
    ) -> tuple[dict[int, Image], dict[int, Image]] | None:
        """The candidate's and the source's audit-band rows of ``frames``, when all are held."""
        stored = band_store.get(video)
        if stored is None:
            return None
        indices, cleaned, pending = stored
        if pending is None or not cleaned or not set(frames) <= set(indices):
            return None
        return dict(zip(indices, cleaned, strict=True)), dict(
            zip(indices, pending.result(), strict=True)
        )

    def band_counts(
        video: Path, frames: Sequence[int]
    ) -> tuple[tuple[int, int] | None, list[Image] | None, list[Image] | None]:
        """``colour_counts``'s band, candidate and source rows from the held bands (it checks
        that the boxes fit); none when they are not held."""
        held = stored_bands(video, frames)
        if held is None:
            return None, None, None
        return (
            band_audit.held_band(envelope, canvas),
            [held[0][frame] for frame in frames],
            [held[1][frame] for frame in frames],
        )

    def scene_frames(
        video: Path, boxes: Sequence[Sequence[colour.Box]], frames: Sequence[int]
    ) -> scene.Frames | None:
        """Full-size frames holding only the held band rows (zero elsewhere), for the scene
        recount, when every audited frame's boxes lie ``scene.BAND_REACH`` rows inside the band:
        all its pixels then lie in the band, so its counts equal the full-frame ones."""
        held = stored_bands(video, frames)
        band = band_audit.held_band(envelope, canvas)
        if held is None or not scene.inside_band(band, [boxes[frame] for frame in frames]):
            return None
        origin = colour_audit.crop_origin(band[0])

        def frames_of(kind: str, wanted: Sequence[int]) -> Iterator[Image]:
            rows = held[0] if kind == "cleaned" else held[1]
            for frame in wanted:
                full = np.zeros((canvas[1], canvas[0], 3), dtype=np.uint8)
                full[origin : origin + rows[frame].shape[0]] = rows[frame]
                yield full

        return frames_of

    recognised: dict[tuple[Path, tuple[int, ...]], Mapping[int, FrameObservation]] = {}
    rendered: dict[Path, dict[str, object]] = {}
    # The recording's caption style, measured once from its profile (caption_style) and recorded
    # in the receipt: the outline rules apply only to pink-outlined captions.
    styles: list[dict[str, object]] = []
    styling = threading.Lock()

    def outlined_style(_admitted: dict[str, object] | None = None) -> bool:
        with styling:
            if not styles:
                styles.append(
                    caption_style(
                        _profile_frames(Path(str(config["profileDirectory"]))), profile,
                        source=source, ffmpeg=str(config.get("ffmpeg", "ffmpeg")),
                        cache=state_root.parent / "caption-style.json",
                    )
                )  # fmt: skip
            return styles[0].get("outlined") is True

    def styled_caption(lines: Sequence[Residual]) -> list[bool]:
        """Which below-floor (or unplanned) lines are an unread caption: Japanese ones styled
        like the captions (``japanese_styled``) or English ones left in caption pink
        (``english_left``)."""
        return [
            kana or english
            for kana, english in zip(japanese_styled(lines), english_left(lines), strict=True)
        ]

    def japanese_styled(lines: Sequence[Residual]) -> list[bool]:
        """Below-floor Japanese lines of ``LOW_CONFIDENCE_MIN_LETTERS`` or more whose source is
        clearly caption-styled (``LOW_CONFIDENCE_OUTLINED_SHARE``); none unless the style is
        outlined."""
        if not outlined_style():
            return [False] * len(lines)
        japanese = [
            item.box is not None
            and japanese_letters(item.text)
            and len("".join(item.text.split())) >= colour_audit.LOW_CONFIDENCE_MIN_LETTERS
            for item in lines
        ]
        chosen = [item for item, kana in zip(lines, japanese, strict=True) if kana]
        if not chosen:
            return japanese
        shares = iter(
            colour_audit.source_outlined_shares(
                ffmpeg=str(config.get("ffmpeg", "ffmpeg")),
                source=source,
                source_start=float(clock.pts[0] * clock.time_base),
                lines=[(item.frame, item.box) for item in chosen if item.box is not None],
                size=canvas,
                masks=source_masks,
            )
        )
        return [
            kana and _caption_styled(item.text, next(shares))
            for item, kana in zip(lines, japanese, strict=True)
        ]

    def english_left(lines: Sequence[Residual]) -> list[bool]:
        """Horizontal Latin lines (``colour_audit.ENGLISH_MIN_LETTERS`` letters or more) whose
        source box is caption-pink filled (``ENGLISH_FILL_SHARE``) and whose box in the audited
        candidate (before any render: in the source) still holds ``GATE`` unchanged caption-colour
        pixels: a translation line the plan left (its fill is pink, not outlined, so the Japanese
        style rule cannot see it)."""
        latin = [
            item.box is not None
            and not japanese_letters(item.text)
            and sum(c.isascii() and c.isalpha() for c in item.text)
            >= colour_audit.ENGLISH_MIN_LETTERS
            and item.box[2] >= 2 * item.box[3]
            for item in lines
        ]
        if not any(latin):
            return [False] * len(lines)
        video = next(iter(rendered), None)
        start = float(clock.pts[0] * clock.time_base)
        ffmpeg = str(config.get("ffmpeg", "ffmpeg"))
        chosen = [item for item, wide in zip(lines, latin, strict=True) if wide and item.box]
        # One decode for every chosen line's source box, one for the filled lines' frames.
        counts = colour_audit.source_pink_counts(
            ffmpeg=ffmpeg, source=source, source_start=start,
            lines=[(item.frame, item.box) for item in chosen if item.box is not None], size=canvas,
            masks=source_masks,
        )  # fmt: skip
        filled = [
            item
            for item, count in zip(chosen, counts, strict=True)
            if item.box is not None
            and count >= colour_audit.ENGLISH_FILL_SHARE * item.box[2] * item.box[3]
        ]
        if not filled:
            return [False] * len(lines)
        if video is None:
            pink = dict(zip(chosen, counts, strict=True))
            return [item in filled and pink[item] >= GATE for item in lines]
        boxes: list[list[colour.Box]] = [[] for _ in clock.pts]
        for item in filled:
            if item.box is not None:
                x, y, width, height = item.box
                boxes[item.frame].append(
                    (int(x), int(y), int(np.ceil(x + width)), int(np.ceil(y + height)))
                )
        frames = sorted({item.frame for item in filled})
        band, cleaned_bands, source_rows = band_counts(video, frames)
        # A frame's filled lines share its count: any of them left in caption colour fails it.
        left = dict(
            zip(
                frames,
                colour_audit.colour_counts(
                    ffmpeg=ffmpeg, source=source, source_start=start, cleaned=video,
                    indices=frames, boxes=boxes, size=canvas, band=band,
                    cleaned_bands=cleaned_bands, source_bands=source_rows,
                ),
                strict=True,
            )
        )  # fmt: skip
        return [item in filled and left[item.frame] >= GATE for item in lines]

    def notes() -> dict[str, object]:
        style = {"captionStyle": dict(styles[0])} if styles else {}
        reason = (
            "braked" if braked
            else "no band confirmed yet" if not learned
            else "no frame's caption region reaches a band" if not any(seeded)
            else None
        )  # fmt: skip
        return {
            **style,
            "learnedBands": len(learned),
            "learning": {**learning, "seededFrames": list(seeded), "noSeedReason": reason},
        }

    # The colour gate needs only the decoded bands: it counts them while the OCR recognises the
    # tiles, and the gate collects the count afterwards (same inputs, same counts).
    early: dict[tuple[Path, tuple[int, ...]], Future[list[int]]] = {}
    counter = ThreadPoolExecutor(max_workers=1)
    gated = config.get("colourGate") is True
    # The colour gate's source rows depend only on the source and the frames: decode them
    # while the band audit recognises the candidate.
    ahead: dict[tuple[Path, tuple[int, ...]], Future[list[Image]]] = {}
    decoder = ThreadPoolExecutor(max_workers=1)

    source_cache: dict[tuple[int, ...], list[Image]] = {}  # the same frames in every attempt

    def source_bands(indices: tuple[int, ...]) -> list[Image]:
        if indices in source_cache:
            return source_cache[indices]
        band = band_audit.held_band(envelope, canvas)
        rows = list(
            colour_audit._frames(
                str(config.get("ffmpeg", "ffmpeg")), source,
                float(clock.pts[0] * clock.time_base), indices, canvas, band,
            )
        )  # fmt: skip
        # The line checks' source masks from these rows, while the OCR runs (audit waits).
        source_masks.add_bands(indices, rows, colour_audit.crop_origin(band[0]))
        source_cache.clear()
        source_cache[indices] = rows
        return rows

    def read_tiles(movie: Path, tiles: VideoFrameClock, height: int) -> dict[int, FrameObservation]:
        return chunk.session_ocr(
            native, movie, tiles, range(len(tiles.pts)), canvas=(canvas[0], height),
            work=movie.parent / "ocr", root=root, workers=workers,
        )  # fmt: skip

    def source_read(
        tile: int, directory: Path, indices: Sequence[int]
    ) -> Mapping[int, FrameObservation]:
        """The audit's read of these source frames; their rows stay for the attempt's own audit."""
        frames = tuple(indices)
        source_masks.expect(frames)
        return band_audit.band_audit(
            source, frames, clock=relative, canvas=canvas, envelope=envelope, tile=tile,
            work=directory / "source-read", recognise=read_tiles,
            ffmpeg=str(config.get("ffmpeg", "ffmpeg")), held=source_bands(frames),
        )  # fmt: skip

    def audit(video: Path, indices: Sequence[int]) -> Mapping[int, FrameObservation]:
        # A repaired candidate's timeline starts at zero, not at the chunk's source time.
        work = state_root / video.parent.name / "audit"
        source_masks.expect(indices)  # the line checks read these frames' source
        if audit_tile is None:
            return chunk.session_ocr(
                native, video, relative, indices, canvas=canvas,
                work=work, root=root, workers=workers, band=band,
            )  # fmt: skip

        def recognise(
            movie: Path, tiles: VideoFrameClock, height: int
        ) -> dict[int, FrameObservation]:
            key = (video, tuple(indices))
            if gated and decoded and video in rendered and key not in early:
                early[key] = counter.submit(
                    count, rendered[video], video, indices, list(decoded), ahead.pop(key, None)
                )
            return read_tiles(movie, tiles, height)

        decoded: list[Image] = []
        ahead.clear()
        if gated:
            ahead[(video, tuple(indices))] = decoder.submit(source_bands, tuple(indices))
        band_store.clear()
        band_store[video] = (tuple(indices), decoded, ahead.get((video, tuple(indices))))
        observed = band_audit.band_audit(
            video, indices, clock=relative, canvas=canvas, envelope=envelope, tile=audit_tile,
            work=work, recognise=recognise, ffmpeg=str(config.get("ffmpeg", "ffmpeg")),
            decoded=decoded,
        )  # fmt: skip
        pending = band_store[video][2]
        if pending is not None:
            pending.result()  # the source masks hold the band rows before the line checks run
        audited.clear()
        audited[(video, tuple(indices))] = decoded  # the colour gate reads these bands again
        recognised.clear()
        recognised[(video, tuple(indices))] = observed  # its interface lines, for the colour gate
        return observed

    def colour_residual(
        admitted: dict[str, object], video: Path, indices: Sequence[int]
    ) -> list[int]:
        key = (video, tuple(indices))
        bands = audited.pop(key, None)
        counted = early.pop(key, None)
        counts = (
            counted.result()
            if counted is not None
            else count(admitted, video, indices, bands, ahead.pop(key, None))
        )
        observed = recognised.pop(key, None)
        if observed is None:
            return counts
        counts, excluded = beside_interface(admitted, video, indices, counts, observed)
        return beside_scene(admitted, video, indices, counts, observed, excluded)

    def beside_interface(
        admitted: dict[str, object],
        video: Path,
        indices: Sequence[int],
        counts: list[int],
        observed: Mapping[int, FrameObservation],
    ) -> tuple[list[int], list[list[colour.Box]]]:
        """Counts again, without the boxes of interface text, the frames the gate would flag; also
        the left-out boxes per frame.

        Scene pixels of caption colour sit beside interface labels (gift stickers); an interface
        box grown by its height is left out unless another (caption) line overlaps it.
        """
        entries = admitted["frames"]
        if not isinstance(entries, list):
            raise ValueError("Admission lacks per-frame entries")
        excluded: list[list[colour.Box]] = [[] for _ in entries]
        again = [
            frame
            for frame, value in zip(indices, counts, strict=True)
            if value >= GATE and frame in observed
            and _interface_boxes(observed, frame, excluded, moving_interface)
        ]  # fmt: skip
        if not again:
            return counts, excluded
        band, cleaned_bands, source_rows = band_counts(video, again)
        recounted = colour_audit.colour_counts(
            ffmpeg=str(config.get("ffmpeg", "ffmpeg")),
            source=source,
            source_start=float(clock.pts[0] * clock.time_base),
            cleaned=video,
            indices=again,
            boxes=[_entry_boxes(entry) for entry in entries],
            size=canvas,
            excluded=excluded,
            outlined=outlined_style(admitted),
            band=band,
            cleaned_bands=cleaned_bands,
            source_bands=source_rows,
        )
        fresh = dict(zip(again, recounted, strict=True))
        return [
            fresh.get(frame, value) for frame, value in zip(indices, counts, strict=True)
        ], excluded

    def beside_scene(
        admitted: dict[str, object],
        video: Path,
        indices: Sequence[int],
        counts: list[int],
        observed: Mapping[int, FrameObservation],
        excluded: list[list[colour.Box]],
    ) -> list[int]:
        """Counts again the frames still at or over the gate without the residual components the
        source shows as scene (``subtitle_colour_scene``): caption boxes are the plan's line boxes
        and the YOLO caption boxes, caption lines the audit's reads that are not interface text."""
        if all(value < GATE for value in counts):
            return counts
        entries = admitted["frames"]
        if not isinstance(entries, list):
            raise ValueError("Admission lacks per-frame entries")
        boxes = [_entry_boxes(entry) for entry in entries]
        presence = [
            [*row, *(_corner_box(corners) for corners in planned_boxes[index])]
            if index < len(planned_boxes) else row
            for index, row in enumerate(boxes)
        ]  # fmt: skip
        lines = {
            frame: [_rectangle_corners(line.rectangle) for line in item.detections]
            for frame, item in observed.items()
        }
        judged: dict[tuple[int, int], bool] = {}

        def interface(frame: int, index: int) -> bool:
            """Not a caption read: interface text, or a below-floor read the audit would not
            count as an unread caption (a name label over a pink sticker, 10/27 chunk 102)."""
            if (frame, index) not in judged:
                line = observed[frame].detections[index]
                judged[(frame, index)] = residual_interface(observed, frame, index) or (
                    line.confidence < MIN_CONFIDENCE
                    and not styled_caption(
                        [
                            Residual(
                                frame, line.text, line.confidence, _rectangle_box(line.rectangle)
                            )
                        ]
                    )[0]
                )
            return judged[(frame, index)]

        return scene.scene_counts(
            ffmpeg=str(config.get("ffmpeg", "ffmpeg")),
            source=source,
            source_start=float(clock.pts[0] * clock.time_base),
            cleaned=video,
            indices=indices,
            counts=counts,
            boxes=boxes,
            evidence=scene.Evidence(presence, lines, interface),
            size=canvas,
            excluded=excluded,
            outlined=outlined_style(admitted),
            frames=scene_frames(video, boxes, indices),
        )

    def count(
        admitted: dict[str, object],
        video: Path,
        indices: Sequence[int],
        bands: list[Image] | None,
        pending: Future[list[Image]] | None,
    ) -> list[int]:
        entries = admitted["frames"]
        if not isinstance(entries, list):
            raise ValueError("Admission lacks per-frame entries")
        boxes = [_entry_boxes(entry) for entry in entries]
        return colour_audit.colour_counts(
            ffmpeg=str(config.get("ffmpeg", "ffmpeg")),
            source=source,
            source_start=float(clock.pts[0] * clock.time_base),
            cleaned=video,
            indices=indices,
            boxes=boxes,
            size=canvas,
            band=None if bands is None else band_audit.held_band(envelope, canvas),
            cleaned_bands=bands,
            source_bands=None if pending is None else pending.result(),
            outlined=outlined_style(admitted),
        )

    def colour_regions(
        admitted: dict[str, object], video: Path, frames: Sequence[int]
    ) -> list[Residual]:
        # Decodes only the flagged frames: what a corrective repair of them should cover.
        entries = admitted["frames"]
        if not isinstance(entries, list):
            raise ValueError("Admission lacks per-frame entries")
        found = colour_audit.residual_boxes(
            ffmpeg=str(config.get("ffmpeg", "ffmpeg")),
            source=source,
            source_start=float(clock.pts[0] * clock.time_base),
            cleaned=video,
            indices=sorted(set(frames)),
            boxes=[_entry_boxes(entry) for entry in entries],
            size=canvas,
        )
        return [
            Residual(index, "caption colour", 0.0, (left, top, right - left, bottom - top))
            for index, boxes in found.items()
            for left, top, right, bottom in boxes
        ]

    def caption_coloured(residuals: Sequence[Residual]) -> list[bool]:
        lines = [(item.frame, item.box) for item in residuals if item.box is not None]
        if len(lines) != len(residuals):
            raise ValueError("Residual lines lack their boxes")
        start = float(clock.pts[0] * clock.time_base)
        ffmpeg = str(config.get("ffmpeg", "ffmpeg"))
        counts = colour_audit.source_pink_counts(
            ffmpeg=ffmpeg,
            source=source,
            source_start=start,
            lines=lines,
            size=canvas,
            masks=source_masks,
        )
        # A Japanese line must also be caption-styled (pink outline around white glyphs); an
        # English translation line is pink fill and keeps the plain pink test.
        styled = outlined_style()
        japanese = [styled and japanese_letters(item.text) for item in residuals]
        shares = (
            colour_audit.source_outlined_shares(
                ffmpeg=ffmpeg,
                source=source,
                source_start=start,
                lines=lines,
                size=canvas,
                masks=source_masks,
            )
            if any(japanese)
            else [1.0] * len(residuals)
        )
        return [
            count >= colour_audit.MINIMUM_CAPTION_PINK
            and (not kana or share >= colour_audit.MINIMUM_OUTLINED_SHARE)
            for count, kana, share in zip(counts, japanese, shares, strict=True)
        ]

    pins = {"native": fingerprint(native), "source": str(config.get("sourceSHA256", ""))}
    if audit_tile is not None:
        pins["auditBand"] = f"tile{audit_tile}"
    if config.get("colourKey") is True:
        pins["colourKey"] = COLOUR_KEY_PIN
    gate = colour_residual if gated else None
    coloured = caption_coloured if gated else None
    flagged_regions = colour_regions if gated else None
    vocabulary = profile.bands.vocabulary
    reach = int(str(config.get("auditStride", runner.AUDIT_STRIDE)))

    def moving_interface(observed: Mapping[int, FrameObservation], frame: int, index: int) -> bool:
        return audited_interface(observed, frame, index, vocabulary, reach)

    def residual_interface(
        observed: Mapping[int, FrameObservation], frame: int, index: int
    ) -> bool:
        """Moving interface text, or (gated) a still label misread below the floor: one of its
        nearby vocabulary reads holds no caption colour in the source. Residual lines only; the
        colour recount keeps such a box."""
        if moving_interface(observed, frame, index):
            return True
        if not gated:
            return False
        anchors = [
            Residual(at, line.text, line.confidence, _rectangle_box(line.rectangle))
            for at, line in still_label_reads(observed, frame, index, vocabulary, reach)
        ]
        return bool(anchors) and not all(caption_coloured(anchors))

    return runner.Collaborators(
        plan,
        render_attempt,
        audit,
        gate,
        coloured,
        residual,
        flagged_regions,
        residual_interface,
        styled_caption=styled_caption if gated else None,
        notes=notes,
        source_read=(
            partial(source_read, audit_tile)
            if yolo_admission and gated and audit_tile is not None
            else None
        ),
    ), pins


def stored_boxes(
    config: Mapping[str, object], settings: yolo.YoloSettings
) -> yolo_boxes.Boxes | None:
    """yolo-track's boxes of this chunk when the configuration names them and all are present."""
    if "yoloBoxes" not in config:
        return None
    clock = _clock(config["clock"])
    return yolo_boxes.load(
        Path(str(config["yoloBoxes"])), clock.pts, time_base=clock.time_base,
        source_sha256=str(config["sourceSHA256"]), weights_sha256=settings.sha256,
        imgsz=settings.imgsz,
    )  # fmt: skip


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("config", type=Path)
    arguments = parser.parse_args(argv)
    if arguments.config.stat().st_size > MAX_CONFIG_BYTES:
        raise ValueError("Chunk configuration exceeds its byte budget")
    config = record(json.loads(arguments.config.read_text(encoding="utf-8")))
    network = mps.lazy_network(
        Path(str(config["detectorProgram"])), sha256=str(config["detectorProgramSHA256"])
    )
    region_pins: dict[str, str] = {}
    caption_detector: yolo.Detector | None = None
    caption_masks = yolo_admission = False
    stored: yolo_boxes.Boxes | None = None
    if "regionDetector" in config:
        settings = yolo.settings_from(config["regionDetector"])
        # A model promoted since the recording started applies from this chunk on: its stored
        # yolo-track boxes then name other weights, so this chunk detects with the promoted ones.
        settings = yolo_learning.promoted(
            settings,
            settings.weights.parent,
            Path(str(config["output"])).parent.parent.parent,
        )
        stored = stored_boxes(config, settings)
        # Stored boxes need no detector: its model load is skipped too.
        caption_detector = None if stored is not None else yolo.load_detector(settings)
        caption_masks, yolo_admission = settings.masks, settings.admission
        print(f"YOLO_BOXES {'stored' if stored is not None else 'detected'}", flush=True)
        masks = (":masks" if settings.masks else "") + (":admission" if settings.admission else "")
        region_pins = {"regionDetector": f"yolo:{settings.sha256}:{settings.imgsz}{masks}"}
    sttn_pins: dict[str, str] = {}
    painter: subtitle_sttn.Inpainter | None = None
    if "sttn" in config:
        settings_sttn = record(config["sttn"])
        painter = subtitle_sttn.Inpainter(
            Path(str(settings_sttn["directory"])),
            checkpoint_sha=str(settings_sttn["checkpointSHA256"]),
            architecture_sha=str(settings_sttn["architectureSHA256"]),
        )
        sttn_pins = {
            "sttn": str(settings_sttn["checkpointSHA256"]),
            "sttnAttention": subtitle_sttn.ATTENTION,
            "sttnTiles": subtitle_sttn.TILES,
            "sttnCompute": subtitle_sttn.compute(),
        }
    tools, pins = real_collaborators(
        config,
        network=network,
        caption_detector=caption_detector,
        caption_masks=caption_masks,
        yolo_admission=yolo_admission,
        sttn=painter,
        stored_boxes=stored,
    )
    output = Path(str(config["output"]))
    source_clock = _clock(config["clock"])
    times = [float(pts * source_clock.time_base) for pts in source_clock.pts]
    try:
        receipt = runner.run(
            output,
            tools,
            stride=int(str(config.get("stride", 6))),
            audit_stride=int(str(config.get("auditStride", runner.AUDIT_STRIDE))),
            pins=pins
            | {"detectorProgram": str(config["detectorProgramSHA256"])}
            | region_pins
            | sttn_pins,
        )
    except ValueError:
        # A chunk that failed every attempt still teaches: its retained receipt's candidates.
        kept = output / "chunk-receipt.json"
        if kept.is_file():
            cleanup_learning.record(
                output.parent.parent,
                output.name,
                record(json.loads(kept.read_text("utf-8"))),
                times,
            )
        raise
    cleanup_learning.record(output.parent.parent, output.name, receipt, times)
    print(json.dumps({"accepted": receipt["accepted"], "escalated": receipt["escalated"]}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
