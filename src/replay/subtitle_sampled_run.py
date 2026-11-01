"""Sampled-OCR repair of one chunk with an audit gate and escalation to full OCR.

Attempt 1 uses the configured stride (the YOLO-admitted plan when configured). The repaired video
is audited by OCR of every Nth frame and by the caption-colour gate. Order of attempts:

1. ``attempt-sampled``: the sampled plan.
2. ``attempt-sampled-residual`` (only when 1 failed): what 1 left is repaired with 1's plan,
   without planning again; only frames around the findings gain repair boxes.
3. ``attempt-full`` (only when 2 failed or could not run): every frame recognised by OCR.
4. ``attempt-residual`` (only when 3 failed): what 3 left, repaired with 3's plan.

"What an attempt left" is its OCR residual lines or, when the OCR audit found none, the
caption-colour components of its colour-flagged frames (``Collaborators.colour_regions``,
which decodes only those frames). Every attempt is audited at the same gates; the first passing
one is accepted, and nothing is accepted when all fail. A healing attempt (2, 4) is the hook to
extend (10-25): add finding kinds to ``_left`` rather than new attempts beside it.
Each attempt keeps its own directory and nothing is overwritten.
The receipt states that regions came from sampling and that quality is not approved.
"""

import copy
import json
import os
import time
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass
from itertools import pairwise
from pathlib import Path

from replay.caption_io import FrameObservation
from replay.subtitle_colour_residual import GATE, flagged
from replay.subtitle_region_fill import Region
from replay.subtitle_repair_audit import (
    MIN_CONFIDENCE,
    Residual,
    audit_indices,
    residual_lines,
    unplanned_lines,
)

FULL_STRIDE = 1
AUDIT_STRIDE = 5


@dataclass(frozen=True, slots=True)
class Collaborators:
    """Injectable work: planning, rendering and auditing of one attempt."""

    plan: Callable[[int, Path], dict[str, object]]
    render: Callable[[dict[str, object], Path], Path]
    audit: Callable[[Path, Sequence[int]], Mapping[int, FrameObservation]]
    colour: Callable[[dict[str, object], Path, Sequence[int]], Sequence[int]] | None = None
    caption_coloured: Callable[[Sequence[Residual]], Sequence[bool]] | None = None
    # Registers the residual lines a failed full attempt left, as repair boxes for one more try.
    residual: Callable[[Sequence[Residual], int], None] | None = None
    # The residual caption-colour components of the given (colour-flagged) frames, as lines.
    colour_regions: (
        Callable[[dict[str, object], Path, Sequence[int]], Sequence[Residual]] | None
    ) = None
    # Whether an audited line (frame, line index) is interface text, never a caption residual:
    # moving (``subtitle_profile.audited_interface``) or a still label misread below the floor
    # (``subtitle_profile.still_label_reads``), with the profile's vocabulary.
    moving_interface: Callable[[Mapping[int, FrameObservation], int, int], bool] | None = None
    # Which below-floor OCR lines are caption-styled in the source (an unread caption).
    styled_caption: Callable[[Sequence[Residual]], Sequence[bool]] | None = None
    # Decisions the collaborators measured (the caption style), recorded in the receipt.
    notes: Callable[[], Mapping[str, object]] | None = None


@dataclass(frozen=True, slots=True)
class Attempt:
    """One attempt's receipt row, with the plan it rendered (as planned) and its video."""

    row: dict[str, object]
    plan: dict[str, object]
    video: Path


def _regions(plan: Mapping[str, object]) -> list[Region | None]:
    frames = plan["frames"]
    if not isinstance(frames, list):
        raise ValueError("Admission lacks per-frame entries")
    regions: list[Region | None] = []
    for entry in frames:
        box = entry.get("region") if isinstance(entry, dict) else None
        if isinstance(box, dict):
            regions.append((box["x"], box["y"], box["x"] + box["width"], box["y"] + box["height"]))
        else:
            regions.append(None)
    return regions


def _stacked(region: Region) -> Region:
    """The region with its own height added above and below it.

    A caption block taller than the profiled band stacks its extra lines there; only
    caption-coloured lines among them fail the audit, so scene text beside it stays interface.
    """
    left, top, right, bottom = region
    return left, top - (bottom - top), right, bottom + (bottom - top)


def _attempt(
    stride: int,
    directory: Path,
    tools: Collaborators,
    audit_stride: int,
    planned: dict[str, object] | None = None,
) -> Attempt:
    """Plan (or reuse ``planned``), render and audit one attempt."""
    directory.mkdir(mode=0o700)
    marks = [time.monotonic()]
    plan = tools.plan(stride, directory) if planned is None else copy.deepcopy(planned)
    kept = copy.deepcopy(plan)  # rendering may widen the plan's regions in place
    marks.append(time.monotonic())
    video = tools.render(plan, directory)
    marks.append(time.monotonic())
    regions = _regions(plan)
    indices = audit_indices(len(regions), audit_stride)
    # Wall time of each audit check (the OCR, then the rules over its reads): what the audit costs.
    checks: dict[str, float] = {}

    def timed[T](name: str, work: Callable[[], T]) -> T:
        started = time.monotonic()
        try:
            return work()
        finally:
            checks[name] = round(checks.get(name, 0.0) + time.monotonic() - started, 3)

    observed = timed("ocr", lambda: tools.audit(video, indices))
    stacked = [None if box is None else _stacked(box) for box in regions]
    residuals = residual_lines(observed, stacked)
    if tools.styled_caption is not None:
        # OCR reads a caption left on a busy background with low confidence, and a caption the plan
        # never saw (above the learned band, or on a frame the plan left without a region) lies
        # outside every search region: such a line counts too when its source is caption-styled
        # (an unread or unplanned caption must not pass the audit).
        low = [
            item
            for item in residual_lines(observed, stacked, minimum_confidence=0.0)
            if item.confidence < MIN_CONFIDENCE
        ]
        low += unplanned_lines(observed, stacked)
        if low:
            rule = tools.styled_caption
            styled = timed("styledCaption", lambda: rule(low))
            residuals = (
                *residuals,
                *(item for item, keep in zip(low, styled, strict=True) if keep),
            )
    interface: list[Residual] = []
    if tools.moving_interface is not None and residuals:
        rule_moving = tools.moving_interface
        moving = timed(
            "interface", lambda: [_moving(item, observed, rule_moving) for item in residuals]
        )
        interface = [item for item, flag in zip(residuals, moving, strict=True) if flag]
        residuals = tuple(item for item, flag in zip(residuals, moving, strict=True) if not flag)
    if tools.caption_coloured is not None and residuals:
        rule_coloured = tools.caption_coloured
        kept_lines = residuals
        flags = timed("captionColoured", lambda: rule_coloured(kept_lines))
        interface += [item for item, coloured in zip(residuals, flags, strict=True) if not coloured]
        residuals = tuple(item for item, coloured in zip(residuals, flags, strict=True) if coloured)
    gate = tools.colour
    counts: Sequence[int] = (
        () if gate is None else timed("colourGate", lambda: gate(plan, video, indices))
    )
    marks.append(time.monotonic())
    over = flagged(counts, gate=GATE)
    row: dict[str, object] = {
        "stride": stride,
        "directory": directory.name,
        "video": video.name,
        "sampling": plan.get("sampling"),
        "stageSeconds": plan.get("stageSeconds"),
        # Wall time of plan, render and audit (with the colour gate): where a chunk's time goes.
        "phaseSeconds": dict(
            zip(
                ("plan", "render", "audit"),
                (round(b - a, 3) for a, b in pairwise(marks)),
                strict=True,
            )
        ),
        "auditSeconds": checks,
        "auditStride": audit_stride,
        "auditedFrames": len(observed),
        "residualLines": [_residual(item) for item in residuals],
        "interfaceText": [_residual(item) for item in interface],
        "colourResidual": None
        if tools.colour is None
        else {
            "gate": GATE,
            "flaggedFrames": [indices[i] for i in over],
            "maximum": max(counts, default=0),
        },
        "passed": not residuals and not over,
        "replanned": planned is None,
    }
    return Attempt(row, kept, video)


def _moving(
    item: Residual,
    observed: Mapping[int, FrameObservation],
    rule: Callable[[Mapping[int, FrameObservation], int, int], bool],
) -> bool:
    """The residual is the line of its frame that the rule calls moving interface text."""
    frame = observed.get(item.frame)
    if frame is None:
        return False
    return any(
        line.text == item.text
        and item.box == (line.rectangle.x, line.rectangle.y, line.rectangle.width,
                         line.rectangle.height)
        and rule(observed, item.frame, index)
        for index, line in enumerate(frame.detections)
    )  # fmt: skip


def _residual(item: Residual) -> dict[str, object]:
    return {
        "frame": item.frame,
        "text": item.text,
        "confidence": item.confidence,
        "box": None if item.box is None else list(item.box),
    }


def _left(last: Attempt, tools: Collaborators) -> list[Residual]:
    """What a failed attempt left: its OCR residual lines, else its colour-flagged components."""
    left = _residuals(last.row)
    over = _flagged_frames(last.row)
    if not left and over and tools.colour_regions is not None:
        started = time.monotonic()
        left = list(tools.colour_regions(last.plan, last.video, over))
        seconds = last.row["auditSeconds"]
        if isinstance(seconds, dict):
            seconds["colourRegions"] = round(time.monotonic() - started, 3)
        # Kept in the receipt: the learning store holds them as candidates (cleanup_learning).
        last.row["colourRegions"] = [_residual(item) for item in left]
    return left


def _heal(tried: list[Attempt], directory: Path, tools: Collaborators, audit_stride: int) -> None:
    """When the last attempt failed, repair what it left with its own plan, once, and audit it."""
    last = tried[-1]
    if last.row["passed"] or tools.residual is None:
        return
    left = _left(last, tools)
    if not left:
        return
    # The audit found what the plan missed (a glyph neither detector nor colour key covered).
    tools.residual(left, audit_stride)
    stride = int(str(last.row["stride"]))
    tried.append(_attempt(stride, directory, tools, audit_stride, last.plan))


def _flagged_frames(attempt: Mapping[str, object]) -> list[int]:
    colour = attempt["colourResidual"]
    frames = colour.get("flaggedFrames") if isinstance(colour, dict) else None
    return [int(frame) for frame in frames] if isinstance(frames, list) else []


def _residuals(attempt: Mapping[str, object]) -> list[Residual]:
    lines = attempt["residualLines"]
    if not isinstance(lines, list):
        raise ValueError("Attempt lacks residual lines")
    return [
        Residual(int(line["frame"]), str(line["text"]), float(line["confidence"]),
                 (float(box[0]), float(box[1]), float(box[2]), float(box[3])))
        for line in lines
        if isinstance(line, dict) and isinstance(box := line.get("box"), list)
    ]  # fmt: skip


def run(
    output: Path,
    tools: Collaborators,
    *,
    stride: int = 6,
    audit_stride: int = AUDIT_STRIDE,
    pins: Mapping[str, str] | None = None,
) -> dict[str, object]:
    """Run the attempts in the module's order until one passes its audit; write the receipt."""
    if stride <= FULL_STRIDE or audit_stride < 1:
        raise ValueError("Sampled stride must exceed 1 and the audit stride must be positive")
    output.mkdir(mode=0o700, parents=True, exist_ok=True)
    tried = [_attempt(stride, output / "attempt-sampled", tools, audit_stride)]
    _heal(tried, output / "attempt-sampled-residual", tools, audit_stride)
    if not tried[-1].row["passed"]:
        tried.append(_attempt(FULL_STRIDE, output / "attempt-full", tools, audit_stride))
        _heal(tried, output / "attempt-residual", tools, audit_stride)
    attempts = [item.row for item in tried]
    accepted = next((item for item in reversed(attempts) if item["passed"]), None)
    receipt: dict[str, object] = {
        "version": 1,
        "kind": "sampled-ocr-repair-chunk",
        "attempts": attempts,
        "accepted": None if accepted is None else accepted["directory"],
        "escalated": len(attempts) > 1,
        "regionsFromSampling": True,
        "pins": dict(pins or {}),
        "qualityApproved": False,
        "deliveryComplete": False,
        **({} if tools.notes is None else dict(tools.notes())),
    }
    destination = output / "chunk-receipt.json"
    descriptor = os.open(destination, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
        json.dump(receipt, stream, indent=2, sort_keys=True)
    if accepted is None:
        raise ValueError("Audit failed after escalation; receipt retained, nothing accepted")
    return receipt
