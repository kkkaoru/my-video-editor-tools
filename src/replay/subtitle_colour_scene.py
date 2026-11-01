"""Source evidence that a colour-gate residual is scene, not a caption remnant.

The colour gate counts caption-pink pixels a repair left unchanged inside the line boxes. A pink
object of the scene that the repair restored (an avatar frame's rim, a sticker, a stick: thinner
than the gate's scene opening) is unchanged too. The source decides: a scene object is on screen
whether or not a caption is, so the same pixels appear at the same place (within ``SHIFT`` px of
motion) in a nearby audited frame where no caption covers that place: no recognised line box of
the plan, no YOLO caption box and no caption line the audit read in the repaired frame. The last
matters: a caption the plan missed in a frame has no plan box there but is still read by the
audit (10/22 chunk 19 frame 575: its << >> caption, still on screen five frames later, matched
there). A component that a caption line read in its own frame touches is never tested. A
residual component with no such frame nearby keeps counting, so the check can only remove what
the source shows as scene.

Measured 10-29 on the flagged frames of 10/27 (7701277517759449862) and 10/22
(7699422526320986886): restored rims, stickers and sticks are present in a caption-free audited
frame at 0.85-1.00 of their pixels within 4 px; caption remnants of ``MIN_AREA`` pixels or more
had none. The recount is
evidence for the gate, never proof of absence or approval.
"""

from collections.abc import Callable, Iterable, Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path

import cv2
import numpy as np
from numpy.typing import NDArray

from replay.subtitle_colour_audit import _frames, crop_origin
from replay.subtitle_colour_residual import GATE, in_parallel, residual_mask, unchanged_pink
from replay.subtitle_colour_support import Box, box_mask
from replay.subtitle_inpaint import Image

GROW = 6  # px around a component that a reference frame's caption boxes must leave free
SHIFT = 4  # px an object may move between the residual frame and its reference frame
SAME = 24  # per-channel difference that is still the same source pixel
MATCH = 0.8  # share of a component's pixels its reference frame must show for it to be scene
REFERENCES = 8  # nearest caption-free audited frames tried per component
REACH = 150  # reference frames lie at most this many frames from the residual frame
# A smaller component is never tested: a few pixels of smooth pink find their match by chance
# among the (2 * SHIFT + 1) ** 2 shifts (10/22 chunk 36 frame 95: a 7-px glyph fragment matched).
MIN_AREA = 16
# Rows around a box that a recount's pixels depend on: the box margin, the scene opening (erode
# and dilate) and its 3 px dilation, and the local-median window over the repaired frame's own
# opening, with room to spare. Boxes this far inside the audit band read only band pixels.
BAND_REACH = 72
# Frames of "source" or "cleaned" by audited index, full size, in the order asked.
Frames = Callable[[str, Sequence[int]], Iterable[Image]]


def inside_band(band: tuple[int, int], boxes: Sequence[Sequence[Box]]) -> bool:
    """Every box lies ``BAND_REACH`` rows inside the rows a crop of ``band`` really holds."""
    first = crop_origin(band[0])
    last = first + band[1]
    return all(
        first + BAND_REACH <= box[1] and box[3] + BAND_REACH <= last for row in boxes for box in row
    )


@dataclass(frozen=True, slots=True, eq=False)
class Component:
    """One residual component: its frame, bounding box (left, top, right, bottom), pixels in that
    box (0/1), source pixels there, and the caption-free frames to test it against."""

    frame: int
    box: Box
    part: Image
    source: Image
    references: tuple[int, ...]


def touches(box: Box, others: Sequence[Box]) -> bool:
    """Any of ``others`` overlaps ``box`` grown by ``GROW``."""
    left, top, right, bottom = box[0] - GROW, box[1] - GROW, box[2] + GROW, box[3] + GROW
    return any(
        other[0] < right and left < other[2] and other[1] < bottom and top < other[3]
        for other in others
    )


@dataclass(frozen=True, slots=True)
class Evidence:
    """Where the chunk shows captions: ``presence`` per chunk frame (the plan's recognised line
    boxes and the YOLO caption boxes) and ``lines`` per audited frame (the boxes of the lines the
    audit read in the repaired frame), of which ``interface(frame, index)`` are interface text."""

    presence: Sequence[Sequence[Box]]
    lines: Mapping[int, Sequence[Box]]
    interface: Callable[[int, int], bool]

    def read(self, frame: int, box: Box) -> bool:
        """A caption line (not interface text) read in audited ``frame`` touches ``box``."""
        return any(
            touches(box, (line,)) and not self.interface(frame, index)
            for index, line in enumerate(self.lines.get(frame, ()))
        )


def caption_free(box: Box, frame: int, evidence: Evidence) -> tuple[int, ...]:
    """The nearest ``REFERENCES`` audited frames within ``REACH`` that leave ``box`` free of
    caption boxes and of caption lines read there; none when a caption line read in ``frame``
    itself touches it."""
    if evidence.read(frame, box):
        return ()
    near = [index for index in evidence.lines if index != frame and abs(index - frame) <= REACH]
    near.sort(key=lambda index: (abs(index - frame), index))
    free: list[int] = []
    for index in near:
        if len(free) == REFERENCES:
            break
        if not touches(box, evidence.presence[index]) and not evidence.read(index, box):
            free.append(index)
    return tuple(free)


def present(component: Component, reference: Image) -> bool:
    """At least ``MATCH`` of the component's source pixels appear in ``reference`` at their place,
    for one shift of at most ``SHIFT`` px."""
    left, top, right, bottom = component.box
    height, width = reference.shape[:2]
    wanted = component.part > 0
    source = component.source.astype(np.int16)
    for dy in range(-SHIFT, SHIFT + 1):
        for dx in range(-SHIFT, SHIFT + 1):
            if top + dy < 0 or left + dx < 0 or bottom + dy > height or right + dx > width:
                continue
            window = reference[top + dy : bottom + dy, left + dx : right + dx].astype(np.int16)
            same = np.abs(window - source).max(axis=2) <= SAME
            if float(same[wanted].mean()) >= MATCH:
                return True
    return False


def components(
    frame: int,
    original: Image,
    cleaned: Image,
    allowed: Image,
    evidence: Evidence,
) -> list[Component]:
    """The residual components of one frame that have caption-free frames to test against."""
    return chosen(frame, original, labelled(original, cleaned, allowed), evidence)


Labelled = tuple[int, NDArray[np.int32], NDArray[np.int32]]


def labelled(original: Image, cleaned: Image, allowed: Image) -> Labelled:
    """The residual mask's connected components (count, labels, stats): pixels only."""
    mask = residual_mask(cleaned, original, allowed)
    count, labels, stats, _ = cv2.connectedComponentsWithStats(mask, connectivity=8)
    return count, np.asarray(labels, dtype=np.int32), np.asarray(stats, dtype=np.int32)


def chosen(
    frame: int, original: Image, found_labels: Labelled, evidence: Evidence
) -> list[Component]:
    """The labelled components of ``MIN_AREA`` or more with caption-free reference frames."""
    count, labels, stats = found_labels
    found: list[Component] = []
    for label in range(1, count):
        x, y, width, height, area = (int(value) for value in stats[label])
        if area < MIN_AREA:
            continue
        box = (x, y, x + width, y + height)
        references = caption_free(box, frame, evidence)
        if references:
            part = (labels[y : y + height, x : x + width] == label).astype(np.uint8)
            found.append(Component(frame, box, part, original[y : y + height, x : x + width].copy(),
                                   references))  # fmt: skip
    return found


def scene_counts(
    *,
    ffmpeg: str,
    source: Path,
    source_start: float,
    cleaned: Path,
    indices: Sequence[int],
    counts: Sequence[int],
    boxes: Sequence[Sequence[Box]],
    evidence: Evidence,
    size: tuple[int, int],
    excluded: Sequence[Sequence[Box]] | None = None,
    outlined: bool = False,
    frames: Frames | None = None,
) -> list[int]:
    """``counts`` again, for the frames at or over the gate, without the residual components the
    source shows as scene (``present`` in a caption-free reference frame).

    ``boxes`` and ``excluded`` are the gate's per-frame line boxes and left-out interface boxes.
    Only the flagged frames, their reference frames and, where scene was found, the flagged frames
    again are decoded.
    """
    again = [frame for frame, value in zip(indices, counts, strict=True) if value >= GATE]
    if not again:
        return list(counts)

    def decoded(kind: str, wanted: Sequence[int]) -> Iterable[Image]:
        if frames is not None:
            return frames(kind, wanted)
        if kind == "cleaned":
            return _frames(ffmpeg, cleaned, 0.0, wanted, size)
        return _frames(ffmpeg, source, source_start, wanted, size)

    shape = (size[1], size[0])

    def allowed(frame: int) -> Image:
        mask = box_mask(shape, (0, 0), boxes[frame])
        if excluded is not None and excluded[frame]:
            mask &= 1 - box_mask(shape, (0, 0), excluded[frame])
        return mask

    def masked(item: tuple[int, Image, Image]) -> tuple[int, Image, Labelled]:
        frame, original, clean = item
        return frame, original, labelled(original, clean, allowed(frame))

    # The masks in parallel; the evidence (interface rules of the caller) is asked on this thread.
    pending = [
        item
        for frame, original, found_labels in in_parallel(
            masked, zip(again, decoded("source", again), decoded("cleaned", again), strict=True)
        )
        for item in chosen(frame, original, found_labels, evidence)
    ]
    wanted = sorted({index for item in pending for index in item.references})
    if not wanted:
        return list(counts)

    def shown(item: tuple[int, Image]) -> set[int]:
        index, reference = item
        return {
            position
            for position, component in enumerate(pending)
            if index in component.references and present(component, reference)
        }

    # Positions in ``pending`` some reference frame shows as scene (``BATCH`` frames held at once).
    found: set[int] = set().union(
        *in_parallel(shown, zip(wanted, decoded("source", wanted), strict=True))
    )
    if not found:
        return list(counts)
    scene: dict[int, list[Component]] = {}
    for position in sorted(found):
        scene.setdefault(pending[position].frame, []).append(pending[position])
    flagged_frames = sorted(scene)

    def recounted(item: tuple[int, Image, Image]) -> int:
        frame, original, clean = item
        mask = allowed(frame)
        for component in scene[frame]:
            left, top, right, bottom = component.box
            mask[top:bottom, left:right] &= 1 - component.part
        return unchanged_pink(clean, original, mask, outlined=outlined)

    fresh = dict(
        zip(
            flagged_frames,
            in_parallel(
                recounted,
                zip(
                    flagged_frames,
                    decoded("source", flagged_frames),
                    decoded("cleaned", flagged_frames),
                    strict=True,
                ),
            ),
            strict=True,
        )
    )
    return [fresh.get(frame, value) for frame, value in zip(indices, counts, strict=True)]
