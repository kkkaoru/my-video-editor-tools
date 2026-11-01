"""Learn source-specific subtitle bands; retain sampled evidence and semantic limits."""

from collections import Counter, defaultdict
from collections.abc import Mapping
from dataclasses import dataclass
from difflib import SequenceMatcher
from functools import lru_cache
from math import floor

from .caption_io import FrameObservation
from .caption_masks import TextDetection, TextRectangle
from .subtitle_repair_audit import MIN_CONFIDENCE

# A fixed on-screen label sits in one (x, width) cell and OCR only misreads it: at least half of a
# band's Japanese lines share a cell and most of those read as variants of one text (measured:
# labels 0.95-1.0, caption bands 0.05-0.33 of lines at least 0.75 similar to the commonest reading).
LABEL_CELL = 24
LABEL_CELL_SHARE = 0.5
LABEL_VARIANT_SHARE = 0.8
LABEL_SIMILARITY = 0.75
# An interface row (a scrolling sound or gift menu) moves, so it shares no cell, but its few
# texts come back in frame after frame: a band is a label when at least half of its Japanese
# lines read like a line in at least a tenth (and two) of the band's other profile frames.
# Measured on 21 recordings' profiles (10-25): caption bands 0.00 of lines (no line above 0.02
# of the other frames), interface and label bands 0.78-1.00.
RECUR_FRAME_SHARE = 0.1
RECUR_MIN_FRAMES = 2
RECUR_LINE_SHARE = 0.5
COPY_HEIGHT_RATIO = 1.5  # two copies of one label differ in height by at most this factor
# Across audited frames a lone label is interface text when it moved: a copy of about its size
# lies more than half and at most six of its heights away in a neighbouring audited frame. A
# caption stays put, and a button with the same word sits elsewhere on screen (farther away).
MOVE_MIN_HEIGHTS = 0.5
MOVE_MAX_HEIGHTS = 6.0
# A line of at most this many characters may be a glyph or two OCR cut off a label.
SHORT_READING = 2
# How many audit reaches apart two reads of one still label may be (still_label_reads).
LABEL_GAP_REACHES = 4
# A still label's reads lie within this share of its height vertically (measured 10-28: 0-4 px
# on 25-40 px labels; a caption fragment and a label passing below it, 17-20 px).
STILL_SHIFT_HEIGHTS = 0.25
# Interface text that moves (a falling gift name) belongs to no band: the profile keeps every
# Japanese reading that recurs like a label anywhere (the vocabulary), and a line is interface
# text only when it reads like a vocabulary entry AND repeats beside itself in its frame (another
# copy of about its size elsewhere in the frame, or two copies of one label OCR merged into one
# line). A burned-in caption is one line at one place, so a caption that says a vocabulary word
# stays a caption, even beside small interface buttons with the same word (half its height).
# Measured 10-25: 星降りメリー recurred in 45 of 63 profile frames; none of 52 distinct caption
# residual texts matched the vocabulary.


@dataclass(frozen=True, slots=True)
class SubtitleBands:
    width: int
    height: int
    primary: tuple[int, ...]
    secondary: tuple[int, ...]
    # Readings that recur across the profile like labels (``interface_vocabulary``).
    vocabulary: tuple[str, ...] = ()


def japanese_letters(text: str) -> bool:
    return (
        sum(
            "ぁ" <= character <= "ゖ" or "ァ" <= character <= "ヺ" or "一" <= character <= "鿿"
            for character in text
        )
        >= 2
    )


def _band(line: TextDetection) -> int:
    return floor((line.rectangle.y + line.rectangle.height / 2) / 32)


def _eligible(line: TextDetection, *, width: int, height: int) -> bool:
    box = line.rectangle
    if box.x + box.width > width + 0.001 or box.y + box.height > height + 0.001:
        raise ValueError("Profile detection exceeds canvas")
    return (
        line.confidence >= 0.3
        and 16 <= box.height <= 96
        and any(character.isalpha() for character in line.text)
    )


def learn_subtitle_bands(frames: tuple[FrameObservation, ...]) -> SubtitleBands:
    """Seed variable centered Japanese lines and nearby lower translated lines.

    This is a geometry/script heuristic, not semantic or exhaustive subtitle
    detection. An OCR search ROI is not itself a mask. Repeated labels, including
    changing decimal counters/spacing, cannot seed subtitle masks. Roman text
    above the Japanese bands cannot seed them either; detected text is untouched.
    """
    if not frames or len(frames) > 50000:
        raise ValueError("Invalid subtitle profile observation budget")
    width, height = frames[0].width, frames[0].height
    if not 1 <= width <= 8192 or not 1 <= height <= 8192:
        raise ValueError("Invalid profile canvas")
    counts: dict[int, int] = defaultdict(int)
    texts: dict[int, set[str]] = defaultdict(set)
    jp_counts: dict[int, int] = defaultdict(int)
    jp_texts: dict[int, set[str]] = defaultdict(set)
    cells: dict[int, list[tuple[tuple[int, int], str]]] = defaultdict(list)
    readings: dict[int, list[tuple[int, str]]] = defaultdict(list)
    for index, frame in enumerate(frames):
        if (frame.width, frame.height) != (width, height) or len(frame.detections) > 64:
            raise ValueError("Inconsistent profile observations")
        for line in frame.detections:
            eligible = _eligible(line, width=width, height=height)
            box = line.rectangle
            if (
                not eligible
                or box.width < width / 10
                or box.x >= width * 0.55
                or box.x + box.width <= width * 0.45
            ):
                continue
            band = _band(line)
            # Counters changing within a fixed label are not lexical subtitle variety.
            text = "".join(
                character
                for character in line.text
                if not character.isdecimal() and not character.isspace()
            )
            counts[band] += 1
            if len(texts[band]) < 5:
                texts[band].add(text)
            if japanese_letters(line.text):
                jp_counts[band] += 1
                cells[band].append(
                    ((round(box.x / LABEL_CELL), round(box.width / LABEL_CELL)), text)
                )
                readings[band].append((index, text))
                if len(jp_texts[band]) < 5:
                    jp_texts[band].add(text)
    labels = {
        band
        for band, lines in cells.items()
        if _fixed_label(lines) or _recurring_label(readings[band])
    }
    primary = tuple(
        sorted(
            band
            for band in jp_counts
            if jp_counts[band] >= 10 and len(jp_texts[band]) >= 5 and band not in labels
        )
    )
    if not primary:
        raise ValueError("No variable Japanese subtitle bands; refuse a blanket mask")
    secondary = tuple(
        sorted(
            band
            for band in counts
            if band not in primary
            and band not in labels
            and counts[band] >= 10
            and len(texts[band]) >= 5
            and any(0 < band - source <= 6 for source in primary)
        )
    )
    return SubtitleBands(width, height, primary, secondary, interface_vocabulary(frames))


def _reading(text: str) -> str:
    """The text without digits and spaces: counters and spacing are not lexical variety."""
    return "".join(c for c in text if not c.isdecimal() and not c.isspace())


def _similar(left: str, right: str) -> bool:
    matcher = SequenceMatcher(None, left, right)
    return (
        matcher.real_quick_ratio() >= LABEL_SIMILARITY
        and matcher.quick_ratio() >= LABEL_SIMILARITY
        and matcher.ratio() >= LABEL_SIMILARITY
    )


def interface_vocabulary(frames: tuple[FrameObservation, ...]) -> tuple[str, ...]:
    """Japanese readings that recur like a label: a similar reading in at least a tenth (and two)
    of the other profile frames, wherever in the frame they are."""
    seen: dict[str, set[int]] = defaultdict(set)
    for index, frame in enumerate(frames):
        for line in frame.detections:
            if japanese_letters(line.text) and _eligible(
                line, width=frame.width, height=frame.height
            ):
                seen[_reading(line.text)].add(index)
    needed = max(RECUR_MIN_FRAMES, RECUR_FRAME_SHARE * (len(frames) - 1))
    readings = sorted(seen)
    found: list[str] = []
    for reading in readings:
        where = set().union(*(seen[other] for other in readings if _similar(reading, other)))
        if len(where) - 1 >= needed:
            found.append(reading)
    return tuple(found)


@lru_cache(maxsize=65536)
def _in_vocabulary(reading: str, vocabulary: tuple[str, ...]) -> bool:
    return any(_similar(reading, entry) for entry in vocabulary)


@lru_cache(maxsize=65536)
def known_label(text: str, vocabulary: tuple[str, ...]) -> bool:
    """The text reads like a vocabulary label, or like a part of one (two characters or more)."""
    reading = _reading(text)
    return len(reading) >= 2 and (
        _in_vocabulary(reading, vocabulary) or any(_copies(reading, entry) for entry in vocabulary)
    )


@lru_cache(maxsize=65536)
def _copies(left: str, right: str) -> bool:
    """One reading is (like) a part of the other: two copies of one label, one cut by OCR."""
    short, long = sorted((left, right), key=len)
    if len(short) < 2:
        return False
    return any(
        _similar(short, long[start : start + len(short)])
        for start in range(len(long) - len(short) + 1)
    )


@lru_cache(maxsize=65536)
def _merged_repeat(reading: str, vocabulary: tuple[str, ...]) -> bool:
    """Two copies of one vocabulary label that OCR read as one line."""
    pairs = [reading[start : start + 2] for start in range(len(reading) - 1)]
    if len(set(pairs)) == len(pairs):  # two copies repeat a character pair; most lines do not
        return False
    return any(
        _in_vocabulary(reading[:cut], vocabulary)
        and _in_vocabulary(reading[cut:], vocabulary)
        and _copies(reading[:cut], reading[cut:])
        for cut in range(2, len(reading) - 1)
    )


def interface_text(frame: FrameObservation, vocabulary: tuple[str, ...]) -> set[int]:
    """Indices of the frame's lines that are moving interface text (see the vocabulary rule)."""
    if not vocabulary:
        return set()
    readings = [_reading(line.text) for line in frame.detections]
    japanese = [index for index, line in enumerate(frame.detections) if japanese_letters(line.text)]
    merged = {index for index in japanese if _merged_repeat(readings[index], vocabulary)}
    known = [index for index in japanese if _in_vocabulary(readings[index], vocabulary)]
    heights = [line.rectangle.height for line in frame.detections]
    return merged | {
        index
        for index in known
        if any(
            other != index
            and max(heights[index], heights[other])
            <= COPY_HEIGHT_RATIO * min(heights[index], heights[other])
            and _copies(readings[index], readings[other])
            for other in known
        )
    }


def audited_interface(
    observed: Mapping[int, FrameObservation],
    frame: int,
    index: int,
    vocabulary: tuple[str, ...],
    reach: int,
) -> bool:
    """The ``index``-th line of audited ``frame`` is interface text: flagged in its own frame
    (``interface_text``), a vocabulary label that moved since or until a neighbouring audited frame
    (within ``reach`` frames), or a glyph or two inside such a label's box nearby."""
    current = observed[frame]
    if index in interface_text(current, vocabulary):
        return True
    line = current.detections[index]
    reading = _reading(line.text)
    near = [k for k in range(frame - reach, frame + reach + 1) if k != frame and k in observed]
    if len(reading) > SHORT_READING:
        return (
            japanese_letters(line.text)
            and _in_vocabulary(reading, vocabulary)
            and any(_moved_copy(line, reading, observed[k]) for k in near)
        )
    if not reading:
        return False
    return any(
        reading in _reading(other.text)
        and len(_reading(other.text)) > SHORT_READING
        and _within(line.rectangle, other.rectangle)
        and audited_interface(observed, k, position, vocabulary, reach)
        for k in (frame, *near)
        for position, other in enumerate(observed[k].detections)
        if not (k == frame and position == index)
    )


def still_label_reads(
    observed: Mapping[int, FrameObservation],
    frame: int,
    index: int,
    vocabulary: tuple[str, ...],
    reach: int,
) -> list[tuple[int, TextDetection]]:
    """Vocabulary-label reads (audited frame, line) at the place of the ``index``-th line of
    audited ``frame``, a line read below ``MIN_CONFIDENCE`` or read as a vocabulary label: in
    audited frames within ``reach``, directly or through more such reads at that place
    (``_same_place``).

    A gift name sitting still neither moves nor repeats in its frame, and its source box can hold
    caption colour that is not its own: a caption crossing it (then OCR misreads it, 桐おさき as
    上分に目) or pink stickers behind it. Where one read of the label holds no caption colour in
    the source, the line is that label; a caption, even one saying a vocabulary word, holds caption
    colour at every read (the caller checks). A confident read of other text (more than
    ``SHORT_READING`` characters) ends the chain.
    """

    def label(line: TextDetection) -> bool:
        return japanese_letters(line.text) and _in_vocabulary(_reading(line.text), vocabulary)

    def linked(line: TextDetection) -> bool:
        # A glyph or two read confidently (な of なつとん) says nothing against the label either.
        return (
            line.confidence < MIN_CONFIDENCE
            or label(line)
            or len(_reading(line.text)) <= SHORT_READING
        )

    seen = {(frame, index)}
    pending = [(frame, index)]
    found: list[tuple[int, TextDetection]] = []
    while pending:
        at, position = pending.pop()
        line = observed[at].detections[position]
        if not linked(line):
            continue
        # A label sits still for seconds, while OCR often reads nothing at its place behind a
        # caption: each direction links to the nearest audited frame with a read at the place,
        # at most LABEL_GAP_REACHES audit reaches away (never past a read there).
        hop = LABEL_GAP_REACHES * reach
        for direction in (range(at - 1, at - hop - 1, -1), range(at + 1, at + hop + 1)):
            for k in (k for k in direction if k in observed):
                places = [
                    (other, candidate)
                    for other, candidate in enumerate(observed[k].detections)
                    if _same_place(line.rectangle, candidate.rectangle)
                ]
                for other, candidate in places:
                    if (k, other) in seen:
                        continue
                    seen.add((k, other))
                    pending.append((k, other))
                    if label(candidate):
                        found.append((k, candidate))
                if places:
                    break
    return found


def _same_place(first: TextRectangle, second: TextRectangle) -> bool:
    """About the same size, shifted at most ``MOVE_MIN_HEIGHTS`` of a height sideways (a misread
    label's box varies with its reading) and ``STILL_SHIFT_HEIGHTS`` of it vertically: not moved,
    and not a caption line beside it (that is most of a line height above or below)."""
    height = max(first.height, second.height)
    across = abs(first.x + first.width / 2 - second.x - second.width / 2)
    vertical = abs(first.y + first.height / 2 - second.y - second.height / 2)
    return (
        height <= COPY_HEIGHT_RATIO * min(first.height, second.height)
        and max(first.width, second.width) <= COPY_HEIGHT_RATIO * min(first.width, second.width)
        and across <= MOVE_MIN_HEIGHTS * height
        and vertical <= STILL_SHIFT_HEIGHTS * height
    )


def _moved_copy(line: TextDetection, reading: str, other: FrameObservation) -> bool:
    box = line.rectangle
    centre = (box.x + box.width / 2, box.y + box.height / 2)
    for candidate in other.detections:
        rectangle = candidate.rectangle
        if max(box.height, rectangle.height) > COPY_HEIGHT_RATIO * min(
            box.height, rectangle.height
        ):
            continue
        shift = max(
            abs(rectangle.x + rectangle.width / 2 - centre[0]),
            abs(rectangle.y + rectangle.height / 2 - centre[1]),
        )
        if MOVE_MIN_HEIGHTS * box.height < shift <= MOVE_MAX_HEIGHTS * box.height and _copies(
            reading, _reading(candidate.text)
        ):
            return True
    return False


def _within(inner: TextRectangle, outer: TextRectangle) -> bool:
    """``inner`` lies inside ``outer`` grown by ``outer``'s height each way."""
    grow = outer.height
    return (
        outer.x - grow <= inner.x
        and inner.x + inner.width <= outer.x + outer.width + grow
        and outer.y - grow <= inner.y
        and inner.y + inner.height <= outer.y + outer.height + grow
    )


def _fixed_label(lines: list[tuple[tuple[int, int], str]]) -> bool:
    """Most lines in one geometry cell, read as variants of one text: a label, not captions."""
    cell, count = Counter(key for key, _ in lines).most_common(1)[0]
    if count < len(lines) * LABEL_CELL_SHARE:
        return False
    texts = [text for key, text in lines if key == cell]
    reading = Counter(texts).most_common(1)[0][0]
    variants = sum(
        SequenceMatcher(None, reading, text).ratio() >= LABEL_SIMILARITY for text in texts
    )
    return variants >= len(texts) * LABEL_VARIANT_SHARE


def _recurring_label(lines: list[tuple[int, str]]) -> bool:
    """Most lines read like a line of many other profile frames: interface text, not captions."""
    frames = {index for index, _ in lines}
    needed = max(RECUR_MIN_FRAMES, RECUR_FRAME_SHARE * (len(frames) - 1))
    recurring = 0
    for index, text in lines:
        seen = {
            other
            for other, reading in lines
            if other != index and SequenceMatcher(None, text, reading).ratio() >= LABEL_SIMILARITY
        }
        recurring += len(seen) >= needed
    return recurring >= len(lines) * RECUR_LINE_SHARE


def _linked(left: TextDetection, right: TextDetection) -> bool:
    first, second = left.rectangle, right.rectangle
    vertical = abs(first.y + first.height / 2 - second.y - second.height / 2)
    horizontal = abs(first.x + first.width / 2 - second.x - second.width / 2)
    return 0 < vertical <= max(first.height, second.height) * 1.6 and horizontal <= max(
        80, min(first.width, second.width) / 4
    )


def profiled_detections(
    frame: FrameObservation, profile: SubtitleBands
) -> tuple[TextDetection, ...]:
    """Unusual upper/lower lines require a concurrent, close caption-line chain."""
    if (frame.width, frame.height) != (profile.width, profile.height) or len(frame.detections) > 64:
        raise ValueError("Frame differs from learned profile")
    bands = set(profile.primary + profile.secondary)
    if not bands:
        raise ValueError("Empty subtitle band profile")
    interface = interface_text(frame, profile.vocabulary)
    candidates = tuple(
        line
        for index, line in enumerate(frame.detections)
        if _eligible(line, width=frame.width, height=frame.height) and index not in interface
    )
    accepted = [line for line in candidates if _band(line) in bands]
    pending = [
        line
        for line in candidates
        if min(bands) - 1 <= _band(line) <= max(bands) + 2 and line not in accepted
    ]
    while pending:
        additions = [line for line in pending if any(_linked(line, anchor) for anchor in accepted)]
        if not additions:
            break
        accepted.extend(additions)
        pending = [line for line in pending if line not in additions]
    return tuple(line for line in candidates if line in accepted)
