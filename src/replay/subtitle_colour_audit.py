"""Decode audited frames of a cleaned candidate and its source, and count residual caption colour.

Both videos are read through ffmpeg rawvideo pipes in lockstep (one frame resident per video).
The original's recognised line boxes per frame bound the residual count so scene pixels with the
caption hue elsewhere are ignored. Frame numbers are decode order within each video.
"""

import subprocess
from collections.abc import Iterable, Iterator, Sequence
from pathlib import Path

import cv2
import numpy as np

from replay.subtitle_colour_residual import (
    SCENE_KERNEL,
    WHITE_REACH,
    beside_white,
    in_parallel,
    residual_mask,
    unchanged_pink,
)
from replay.subtitle_colour_support import BOX_MARGIN, Box, box_mask, pink
from replay.subtitle_inpaint import Image

CHANNELS = 3
MINIMUM_CAPTION_PINK = 40
# Burned-in Japanese captions are white glyphs outlined in caption pink, so their boxes hold pink
# within OUTLINE_REACH px of white; interface and scene text of caption hue does not (measured
# 10-25 on 1565 residual lines: Japanese captions mostly >= 0.05 of the box, gift labels p99
# 0.012, game text such as 檻をこわした below 0.01). English translation lines are pink fill.
MINIMUM_OUTLINED_SHARE = 0.01
# A below-floor OCR line (an unread caption on a busy background) counts as a residual when it is
# caption-styled: outlined share >= LOW_CONFIDENCE_OUTLINED_SHARE, or >= FRAMED_OUTLINED_SHARE
# for a line read with the caption frame (<< >>) or at least FRAMED_MIN_LETTERS letters. Measured
# 10-26 on 10/24's attempts: gift-label fragments (short, unframed) reach 0.026; visible captions
# measure 0.04-0.11, framed ones also 0.017-0.029. It must hold LOW_CONFIDENCE_MIN_LETTERS.
LOW_CONFIDENCE_OUTLINED_SHARE = 0.03
FRAMED_OUTLINED_SHARE = 0.015
FRAMED_MIN_LETTERS = 10
LOW_CONFIDENCE_MIN_LETTERS = 4
# A below-floor or unplanned English translation line is pink FILL, not outlined: it counts as a
# residual when its source box is at least ENGLISH_FILL_SHARE caption pink and its candidate box
# still holds GATE unchanged caption-colour pixels. Measured 10-28 on all 2166 such lines of 10/24's
# attempts: gift labels and UI text misread as Latin (over pink stickers) 0.008-0.054; English
# caption lines 0.07-0.33 (left in accepted attempts: 0.149-0.329). At least ENGLISH_MIN_LETTERS
# ASCII letters, in a box at least twice as wide as tall (a caption line, not a sticker).
ENGLISH_FILL_SHARE = 0.06
ENGLISH_MIN_LETTERS = 4
# The style rules above apply only where the recording's captions are that style: the median
# outlined share of the profile's Japanese caption lines (measured 10-26: 10/24 0.035,
# a TikTok replay 0.072, YouTube Eg9 0.135; white captions in a grey outline 0.0), over at
# least this many lines.
STYLE_MIN_MEDIAN = 0.02
STYLE_MIN_LINES = 10
PROCESS_SECONDS = 1800
# Rows around a box that a count reads: the box margin plus the scene-object opening's reach.
REACH = BOX_MARGIN + SCENE_KERNEL + 1


def _selector(indices: Sequence[int]) -> str:
    """Compact ffmpeg ``select`` expression: one term per arithmetic run of frame numbers."""
    terms: list[str] = []
    position = 0
    while position < len(indices):
        stride = indices[position + 1] - indices[position] if position + 1 < len(indices) else 1
        end = position
        while end + 1 < len(indices) and indices[end + 1] - indices[end] == stride:
            end += 1
        first, last = indices[position], indices[end]
        if end == position:
            terms.append(f"eq(n\\,{first})")
        else:
            terms.append(f"between(n\\,{first}\\,{last})*not(mod(n-{first}\\,{stride}))")
        position = end + 1
    return "+".join(terms)


def crop_origin(top: int) -> int:
    """First row an ffmpeg crop of 4:2:0 video really starts at (odd rows round down)."""
    return top - top % 2


def _frames(
    ffmpeg: str,
    video: Path,
    start: float,
    indices: Sequence[int],
    size: tuple[int, int],
    band: tuple[int, int] | None = None,
) -> Iterator[Image]:
    """Yield the requested decode-order frames (ascending, unique) as BGR arrays.

    With ``band`` (top, height) only those rows are converted and piped, cropped like the band
    audit's decode (identical to cropping the full frame at ``crop_origin(top)``).
    """
    wanted = _selector(indices)
    width, height = size
    crop = "" if band is None else f",crop={width}:{band[1]}:0:{band[0]}"
    command = [ffmpeg, "-nostdin", "-v", "error"]
    command += ["-ss", f"{start:.6f}"] if start else []
    command += ["-i", str(video), "-vf", f"select='{wanted}'{crop}", "-fps_mode", "passthrough"]
    command += ["-frames:v", str(len(indices)), "-f", "rawvideo", "-pix_fmt", "bgr24", "-"]
    height = height if band is None else band[1]
    length = width * height * CHANNELS
    with subprocess.Popen(command, stdout=subprocess.PIPE) as process:
        stream = process.stdout
        if stream is None:
            raise ValueError("ffmpeg produced no output pipe")
        for _ in indices:
            raw = stream.read(length)
            if len(raw) != length:
                raise ValueError("Decoder ended before every audited frame")
            yield np.frombuffer(raw, dtype=np.uint8).reshape(height, width, CHANNELS)
        process.wait(timeout=PROCESS_SECONDS)
        if process.returncode:
            raise ValueError("ffmpeg failed while decoding audited frames")


def colour_counts(
    *,
    ffmpeg: str,
    source: Path,
    source_start: float,
    cleaned: Path,
    indices: Sequence[int],
    boxes: Sequence[Sequence[Box]],
    size: tuple[int, int],
    band: tuple[int, int] | None = None,
    cleaned_bands: Sequence[Image] | None = None,
    source_bands: Sequence[Image] | None = None,
    excluded: Sequence[Sequence[Box]] | None = None,
    outlined: bool = False,
) -> list[int]:
    """Unchanged caption-colour pixels per audited frame (``indices`` ascending, unique).

    ``outlined`` applies ``unchanged_pink``'s outline rule (only for outlined caption styles).

    ``excluded`` (per frame, like ``boxes``) are areas not counted although inside a box: the
    boxes of interface text beside which scene pixels of caption colour sit.

    ``band`` (the band audit's rows) reads only those rows of both videos, and
    ``cleaned_bands`` reuses the band audit's decoded candidate instead of decoding it again;
    ``source_bands`` likewise holds the source's ``band`` rows decoded ahead (``_frames``).
    Both apply only when every audited box lies ``REACH`` rows inside the band, so the counts
    equal the full-frame ones; otherwise the full frames are decoded.
    """
    if list(indices) != sorted(set(indices)) or not indices:
        raise ValueError("Audited frame indices must be ascending and unique")
    if band is not None and not inside(band, [boxes[index] for index in indices]):
        band, cleaned_bands, source_bands = None, None, None
    origin = 0 if band is None else crop_origin(band[0])
    shape = (size[1] if band is None else band[1], size[0])
    originals = (
        iter(source_bands)
        if band is not None and source_bands is not None
        else _frames(ffmpeg, source, source_start, indices, size, band)
    )
    repaired = (
        iter(cleaned_bands)
        if cleaned_bands is not None
        else _frames(ffmpeg, cleaned, 0.0, indices, size, band)
    )

    def counted(item: tuple[int, Image, Image]) -> int:
        index, original, clean = item
        allowed = box_mask(shape, (0, origin), boxes[index])
        if excluded is not None and excluded[index]:
            allowed &= 1 - box_mask(shape, (0, origin), excluded[index])
        return unchanged_pink(clean, original, allowed, outlined=outlined)

    return list(in_parallel(counted, zip(indices, originals, repaired, strict=True)))


def residual_boxes(
    *,
    ffmpeg: str,
    source: Path,
    source_start: float,
    cleaned: Path,
    indices: Sequence[int],
    boxes: Sequence[Sequence[Box]],
    size: tuple[int, int],
) -> dict[int, list[Box]]:
    """Bounding boxes of the residual caption-colour components of the given frames only.

    Decodes just those frames (the gate's flagged ones), so a corrective repair can target them.
    """
    if list(indices) != sorted(set(indices)) or not indices:
        raise ValueError("Frame indices must be ascending and unique")
    shape = (size[1], size[0])
    found: dict[int, list[Box]] = {}
    pairs = zip(
        indices,
        _frames(ffmpeg, source, source_start, indices, size),
        _frames(ffmpeg, cleaned, 0.0, indices, size),
        strict=True,
    )
    for index, original, clean in pairs:
        mask = residual_mask(clean, original, box_mask(shape, (0, 0), boxes[index]))
        count, _, stats, _ = cv2.connectedComponentsWithStats(mask, connectivity=8)
        found[index] = [
            (int(x), int(y), int(x + width), int(y + height))
            for x, y, width, height, _ in stats[1:count].tolist()
        ]
    return found


def inside(band: tuple[int, int], boxes: Sequence[Sequence[Box]]) -> bool:
    """Every box lies ``REACH`` rows inside the rows a crop of ``band`` really holds."""
    first = crop_origin(band[0])
    last = first + band[1]
    return all(first + REACH <= box[1] and box[3] + REACH <= last for row in boxes for box in row)


class SourceMasks:
    """Per source frame, its caption-pink and outlined (pink beside white) masks, bit-packed.

    The source-only line checks ask for the same audited frames again and again (one decode per
    call cost 68% of an audit, 10/31 chunk 50: 279 decodes); this decodes each frame once, together
    with every frame ``expect`` named, and keeps ~0.5 MB per frame. Rows of the source's audit
    band decoded already (``add_bands``) serve a line lying ``BAND_MARGIN`` rows inside them
    without decoding the full frame (both masks are per pixel or reach ``WHITE_REACH`` rows).
    """

    BAND_MARGIN = WHITE_REACH + 1

    def __init__(self, ffmpeg: str, source: Path, source_start: float, size: tuple[int, int]):
        self._decode = (ffmpeg, source, source_start, size)
        self._expected: set[int] = set()
        self._pink: dict[int, Image] = {}
        self._outlined: dict[int, Image] = {}
        self._bands: dict[int, tuple[int, Image, Image]] = {}

    def expect(self, indices: Iterable[int]) -> None:
        """Frames a later check is likely to ask for: decoded with the first missing one."""
        self._expected.update(indices)

    def add_bands(self, indices: Sequence[int], images: Sequence[Image], origin: int) -> None:
        """The source's band rows (starting at canvas row ``origin``) of these frames."""
        for index, image in zip(indices, images, strict=True):
            fill = pink(image)
            self._bands[index] = (
                origin,
                np.packbits(fill, axis=1),
                np.packbits(fill & beside_white(image), axis=1),
            )

    def _banded(self, index: int, rows: slice) -> bool:
        held = self._bands.get(index)
        if held is None:
            return False
        first, last = held[0] + self.BAND_MARGIN, held[0] + held[1].shape[0] - self.BAND_MARGIN
        return first <= int(rows.start) and int(rows.stop) <= last

    def prepare(self, lines: Iterable[tuple[int, slice]]) -> None:
        """Decode, at once, every frame these (frame, rows) lines need in full."""
        wanted = {index for index, rows in lines if not self._banded(index, rows)}
        wanted -= self._pink.keys()
        if not wanted:
            return
        missing = sorted(wanted | (self._expected - self._pink.keys() - self._bands.keys()))
        ffmpeg, source, start, size = self._decode
        for index, image in zip(
            missing, _frames(ffmpeg, source, start, missing, size), strict=True
        ):
            fill = pink(image)
            self._pink[index] = np.packbits(fill, axis=1)
            self._outlined[index] = np.packbits(fill & beside_white(image), axis=1)

    def _rows(self, index: int, rows: slice, outlined: bool) -> Image:
        width = self._decode[3][0]
        if self._banded(index, rows):
            origin, fill, outline = self._bands[index]
            band = slice(int(rows.start) - origin, int(rows.stop) - origin)
            return np.unpackbits((outline if outlined else fill)[band], axis=1, count=width)
        self.prepare([(index, rows)])
        held = self._outlined if outlined else self._pink
        return np.unpackbits(held[index][rows], axis=1, count=width)

    def pink(self, index: int, rows: slice) -> Image:
        return self._rows(index, rows, outlined=False)

    def outlined(self, index: int, rows: slice) -> Image:
        return self._rows(index, rows, outlined=True)


def _line_rows(
    lines: Sequence[tuple[int, tuple[float, float, float, float]]], height: int
) -> list[tuple[int, slice]]:
    return [
        (frame, slice(max(0, int(top)), min(height, int(top + box_height) + 1)))
        for frame, (_left, top, _width, box_height) in lines
    ]


def source_pink_counts(
    *,
    ffmpeg: str,
    source: Path,
    source_start: float,
    lines: Sequence[tuple[int, tuple[float, float, float, float]]],
    size: tuple[int, int],
    masks: SourceMasks | None = None,
) -> list[int]:
    """Caption-colour pixels of the SOURCE frame inside each (frame, box) line, in input order.

    A line recognised in a repaired frame that holds no caption-coloured source pixels was never
    caption text (interface text behind the caption, for example); one that does stays a residual.
    """
    if not lines:
        raise ValueError("No lines to classify")
    width, height = size
    masks = masks or SourceMasks(ffmpeg, source, source_start, size)
    masks.prepare(_line_rows(lines, height))
    counts: list[int] = []
    for frame, (left, top, box_width, box_height) in lines:
        x0, y0 = max(0, int(left)), max(0, int(top))
        x1, y1 = min(width, int(left + box_width) + 1), min(height, int(top + box_height) + 1)
        counts.append(int(masks.pink(frame, slice(y0, y1))[:, x0:x1].sum()))
    return counts


def source_outlined_shares(
    *,
    ffmpeg: str,
    source: Path,
    source_start: float,
    lines: Sequence[tuple[int, tuple[float, float, float, float]]],
    size: tuple[int, int],
    masks: SourceMasks | None = None,
) -> list[float]:
    """Share of each (frame, box) line's SOURCE pixels that are caption pink beside white."""
    if not lines:
        raise ValueError("No lines to classify")
    width, height = size
    masks = masks or SourceMasks(ffmpeg, source, source_start, size)
    masks.prepare(_line_rows(lines, height))
    shares: list[float] = []
    for frame, (left, top, box_width, box_height) in lines:
        x0, y0 = max(0, int(left)), max(0, int(top))
        x1, y1 = min(width, int(left + box_width) + 1), min(height, int(top + box_height) + 1)
        area = max(1, (x1 - x0) * (y1 - y0))
        shares.append(float(masks.outlined(frame, slice(y0, y1))[:, x0:x1].sum()) / area)
    return shares


def line_shares(
    *,
    ffmpeg: str,
    source: Path,
    lines: Sequence[tuple[float, tuple[float, float, float, float]]],
    size: tuple[int, int],
) -> list[float]:
    """``source_outlined_shares`` of (source seconds, box) lines: one decode per distinct time."""
    times = sorted({at for at, _ in lines})
    found: dict[float, list[float]] = {}
    for at in times:
        boxes = [box for when, box in lines if when == at]
        found[at] = source_outlined_shares(
            ffmpeg=ffmpeg, source=source, source_start=at,
            lines=[(0, box) for box in boxes], size=size,
        )  # fmt: skip
    return [share for at in times for share in found[at]]


def outlined_style(shares: Sequence[float]) -> bool:
    """The captions are white glyphs outlined in caption pink (the pinned colour key's style):
    only then do the outline rules apply; otherwise every residual counts plainly."""
    ordered = sorted(shares)
    return len(ordered) >= STYLE_MIN_LINES and ordered[len(ordered) // 2] >= STYLE_MIN_MEDIAN
