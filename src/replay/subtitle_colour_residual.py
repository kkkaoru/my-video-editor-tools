"""OCR-free residual check: unchanged caption-colour pixels inside detected line boxes.

A repaired frame still carries caption text where caption-coloured pixels sit inside the
original's recognised line boxes and are (nearly) identical to the original. Scene pixels of
the same hue are excluded because they lie outside the boxes, were changed by the repair, or
belong to a solid pink object of the original, or of the repaired frame where they also match
that object's colour around them. The repaired frame counts too because caption glyphs cut a pink
object under them into pieces narrower than the opening; once the repair has restored the
object, it is solid again (a pink heart under a caption line, 10-25). A leftover glyph stroke on
plain ground is still thinner than the opening, and one left on a pink object of another shade
differs from the object's local median colour, so both stay residual. The local check runs
only for pixels the original-frame rule left over, so frames without them cost nothing more.
The count is evidence for escalation and review, never proof of absence or approval.
"""

from collections.abc import Callable, Iterable, Iterator, Sequence
from concurrent.futures import ThreadPoolExecutor
from itertools import batched

import cv2
import numpy as np

from replay.subtitle_colour_support import pink
from replay.subtitle_inpaint import Image

GATE = 40  # unchanged caption-colour pixels per audited frame that trigger re-repair
SAME = 16  # maximum per-channel difference that still counts as unchanged
SHADE = 32  # per-channel distance from a solid object's median colour that is still its colour
SCENE_HUES = (140, 10)  # loose pink/salmon OpenCV hue range (wraps through red)
SCENE_SATURATION, SCENE_VALUE = 40, 120
OUTLINE_SHARE = 0.25
WHITE_REACH = 2
WHITE_SATURATION, WHITE_VALUE = 40, 200
SCENE_KERNEL = 21  # an opening this wide removes every glyph stroke but keeps solid objects
# Pixels around the allowed area a residual mask depends on: the opening (2 x 10), its 3 px
# dilation and restored_scene's 10 px median window over the repaired frame's own opening.
MASK_MARGIN = 2 * SCENE_KERNEL


# Frames are counted in parallel (OpenCV and NumPy release the GIL): 136 flagged frames took 9 s
# one by one (10/31 chunk 113). ``BATCH`` frames at most are held at once.
THREADS = 8
BATCH = 16
_POOL = ThreadPoolExecutor(max_workers=THREADS, thread_name_prefix="colour-count")


def in_parallel[T, R](work: Callable[[T], R], items: Iterable[T]) -> Iterator[R]:
    """``map(work, items)`` on the counting threads, in order, ``BATCH`` items at a time."""
    for group in batched(items, BATCH):
        yield from _POOL.map(work, group)


def scene_pink(original: Image) -> Image:
    """0/1 mask of solid pinkish scene objects: loose pink survives a glyph-wide opening."""
    hsv = cv2.cvtColor(original, cv2.COLOR_BGR2HSV)
    hue, saturation, value = hsv[..., 0], hsv[..., 1], hsv[..., 2]
    hues = (hue >= SCENE_HUES[0]) | (hue <= SCENE_HUES[1])
    loose = (hues & (saturation >= SCENE_SATURATION) & (value >= SCENE_VALUE)).astype(np.uint8)
    solid = cv2.morphologyEx(loose, cv2.MORPH_OPEN, np.ones((SCENE_KERNEL,) * 2, np.uint8))
    return np.asarray(cv2.dilate(solid, np.ones((3, 3), np.uint8)), dtype=np.uint8)


def unchanged_pink(
    cleaned: Image,
    original: Image,
    allowed: Image,
    *,
    outlined: bool = False,
    away: Image | None = None,
) -> int:
    """Count caption-colour pixels that are unchanged by the repair inside ``allowed`` (0/1).

    With ``outlined`` (the recording's captions are white glyphs outlined in caption pink, as
    measured by ``subtitle_colour_audit.caption_style``), a caption remnant is outline: in the
    original most of its pixels lie beside white glyph fill (measured 10-25: remnants 0.34-1.0 of
    their pixels, sticker and scene pink 0.00-0.11). When under ``OUTLINE_SHARE`` of the
    unchanged pixels do, only those beside white count. Without it every pixel counts.

    ``away`` (0/1) marks the surroundings of interface text: a connected component of the
    residual lying wholly inside it is scene beside the interface (a gift sticker), not counted;
    one reaching outside it (a caption beside the label) still counts whole.
    """
    mask = residual_mask(cleaned, original, allowed)
    if away is not None and mask.any() and away.any():
        count, labels = cv2.connectedComponents(mask, connectivity=8)
        outside = np.unique(labels[(mask > 0) & (away == 0)])
        inside = np.setdiff1d(np.arange(1, count), outside)
        mask = np.asarray(mask & ~np.isin(labels, inside), dtype=np.uint8)
    total = int(mask.sum())
    if not total or not outlined:
        return total
    window = region(mask, WHITE_REACH + 1)  # beside_white reaches WHITE_REACH px
    beside = 0 if window is None else int((mask[window] & beside_white(original[window])).sum())
    return total if beside >= OUTLINE_SHARE * total else beside


def beside_white(original: Image) -> Image:
    """0/1 mask of pixels within ``WHITE_REACH`` px of white (glyph fill) in the original."""
    hsv = cv2.cvtColor(original, cv2.COLOR_BGR2HSV)
    white = ((hsv[..., 1] <= WHITE_SATURATION) & (hsv[..., 2] >= WHITE_VALUE)).astype(np.uint8)
    kernel = np.ones((2 * WHITE_REACH + 1,) * 2, np.uint8)
    return np.asarray(cv2.dilate(white, kernel), dtype=np.uint8)


def region(mask: Image, margin: int) -> tuple[slice, slice] | None:
    """The bounding rows and columns of ``mask``'s set pixels grown by ``margin`` (inside the
    image); none when no pixel is set."""
    rows, columns = np.flatnonzero(mask.any(axis=1)), np.flatnonzero(mask.any(axis=0))
    if not rows.size:
        return None
    height, width = mask.shape[:2]
    return (
        slice(max(0, int(rows[0]) - margin), min(height, int(rows[-1]) + 1 + margin)),
        slice(max(0, int(columns[0]) - margin), min(width, int(columns[-1]) + 1 + margin)),
    )


def residual_mask(cleaned: Image, original: Image, allowed: Image) -> Image:
    """0/1 mask of the caption-colour pixels ``unchanged_pink`` counts.

    Computed only around ``allowed`` (``MASK_MARGIN``): a set pixel depends on pixels at most
    the scene opening and the local-median window away, so the mask equals the full-frame one
    (10/31 chunk 113: 136 flagged frames, 19 s of full-frame masks).
    """
    if cleaned.shape != original.shape or allowed.shape != cleaned.shape[:2]:
        raise ValueError("Cleaned frame, original frame and box mask must share one geometry")
    found = np.zeros(allowed.shape, dtype=np.uint8)
    window = region(allowed, MASK_MARGIN)
    if window is not None:
        found[window] = _residual_mask(cleaned[window], original[window], allowed[window])
    return found


def _residual_mask(cleaned: Image, original: Image, allowed: Image) -> Image:
    difference = np.abs(cleaned.astype(np.int16) - original.astype(np.int16)).max(axis=2)
    unchanged = np.minimum(np.minimum(pink(cleaned), pink(original)), (difference <= SAME))
    left = np.minimum(unchanged, allowed) & (1 - scene_pink(original))
    if not left.any():  # the common case: nothing to explain, so no further work
        return np.asarray(left, dtype=np.uint8)
    return np.asarray(left & (1 - restored_scene(cleaned, left)), dtype=np.uint8)


def restored_scene(cleaned: Image, candidates: Image) -> Image:
    """0/1 mask of the ``candidates`` pixels that lie on a solid pink object of the repaired frame
    and have that object's colour there: within ``SHADE`` of the median of the solid pixels in
    the ``SCENE_KERNEL`` window around them (a local median, since a pinkish background can join
    every object into one component)."""
    solid = scene_pink(cleaned)
    found = np.zeros(solid.shape, dtype=np.uint8)
    reach = SCENE_KERNEL // 2
    for row, column in zip(*np.nonzero(candidates & solid), strict=True):
        rows = slice(max(0, row - reach), row + reach + 1)
        columns = slice(max(0, column - reach), column + reach + 1)
        local = np.median(cleaned[rows, columns][solid[rows, columns] > 0], axis=0)
        distance = np.abs(cleaned[row, column].astype(np.int16) - local.astype(np.int16)).max()
        found[row, column] = distance <= SHADE
    return found


def flagged(counts: Sequence[int], *, gate: int = GATE) -> tuple[int, ...]:
    """Positions whose residual count reaches the gate."""
    if gate < 1:
        raise ValueError("The residual gate must be positive")
    return tuple(index for index, count in enumerate(counts) if count >= gate)
