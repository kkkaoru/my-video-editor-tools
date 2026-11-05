"""Whisper.cpp's warped token times on the recognized text, which stays as it is.

MLX Whisper's token times sit up to half a second from the voice. A second recognition with
whisper.cpp ``-dtw`` (the build patched by ``scripts/release/whisper-cpp.patch`` keeps flash
attention) gives every token a time from the decoder's attention (mean error 0.22 s
against 0.52 s for plain token times, measured against Nemotron in the Transcribe Captions app).
Its text is only a key: characters of both recognitions are matched, and the labels move onto the
matched times.

``t_dtw`` marks where a token ends: against Nemotron it sits 0.01 s from a one-character token's
end and 0.13 s after its start, further with each character. A token therefore starts after the
token before it, or, after a pause, as long before its end as its beats take at the local rate.

    python -m replay.asr_dtw_timing LABELS WAV BINARY BINARY-SHA256 MODEL MODEL-SHA256 WORK OUTPUT
"""

import argparse
import json
import math
import statistics
import subprocess
import wave
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path

from replay.core import ReplayError
from replay.recordings import record
from replay.stages import file_digest

VERSION = 1
TOKEN_TIMING_ARGS = ("-dtw", "large.v3.turbo")
LANGUAGE = "ja"
MILLIS_PER_SECOND = 1000.0
DTW_TICKS_PER_SECOND = 100.0
# Speech stays above about five syllables a second: a token after a pause started at most this
# long per beat before its end.
LONGEST_ONSET_SECONDS_PER_BEAT = 0.2
KANJI_BEATS = 2
SILENT_BEATS = frozenset("ぁぃぅぇぉゃゅょァィゥェォャュョ")
LOCAL_RATE_TOKENS = 5  # tokens on each side that tell how fast the speech around a token is
# A token is said in at most this much per character, so the last one before a pause ends.
LONGEST_SPOKEN_CHARACTER_SECONDS = 0.25
# Flash attention's segment bounds are coarse: pushing a token back onto them piles tokens up.
SEGMENT_MARGIN_SECONDS = 1.0
ALIGNMENT_BAND_CHARACTERS = 200  # how far the two recognitions' texts may drift apart
SHORTEST_LABEL_SECONDS = 0.01  # consumers refuse an empty label
STEP_MATCH, STEP_UP, STEP_LEFT = 1, 2, 3


@dataclass(frozen=True, slots=True)
class TimedToken:
    text: str
    start: float
    end: float
    warped: bool


def spoken_beats(text: str) -> int:
    """About how many beats a text takes: a kanji is usually read in two, a small kana adds none."""
    return sum(KANJI_BEATS if "㐀" <= c <= "鿿" else 0 if c in SILENT_BEATS else 1 for c in text)


def warped_end(ticks: float, bounds: tuple[float, float], previous: float | None) -> float:
    """Warping is reliable inside its segment; a first or last token can land seconds outside
    it (64 s for a token of a segment ending at 62.4 s), so it comes back into the segment."""
    warped = ticks / DTW_TICKS_PER_SECOND
    if warped < bounds[0] - SEGMENT_MARGIN_SECONDS:
        return bounds[0]
    if warped <= bounds[1] + SEGMENT_MARGIN_SECONDS:
        return warped
    if previous is None:
        return bounds[1]
    return min(previous + LONGEST_SPOKEN_CHARACTER_SECONDS, bounds[1])


def segment_tokens(segment: dict[str, object]) -> list[tuple[bytes, float, bool, float]]:
    """A segment's tokens as raw text bytes, a time, whether it is warped, and its plain end."""
    offsets = record(segment["offsets"])
    bounds = (
        float(str(offsets["from"])) / MILLIS_PER_SECOND,
        float(str(offsets["to"])) / MILLIS_PER_SECOND,
    )
    tokens = segment["tokens"]
    if not isinstance(tokens, list):
        raise ReplayError("A whisper.cpp segment has no tokens")
    result: list[tuple[bytes, float, bool, float]] = []
    previous: float | None = None
    for raw in tokens:
        token = record(raw)
        text = str(token["text"]).encode("latin-1", "replace")
        if not text or (text.startswith(b"[_") and text.endswith(b"]")):
            continue
        times = record(token["offsets"])
        plain_end = float(str(times["to"])) / MILLIS_PER_SECOND
        ticks = float(str(token.get("t_dtw", -1)))
        warped = ticks >= 0
        at = (
            warped_end(ticks, bounds, previous)
            if warped
            else float(str(times["from"])) / MILLIS_PER_SECOND
        )
        previous = at
        result.append((text, at, warped, plain_end))
    return result


def parse_tokens(raw: bytes) -> list[TimedToken]:
    """Tokens of whisper.cpp's full JSON. Latin-1 keeps every byte, because it splits UTF-8 across
    tokens and writes the fragments unescaped; a token is complete once its bytes decode."""
    document = record(json.loads(raw.decode("latin-1")))
    segments = document.get("transcription")
    if not isinstance(segments, list):
        raise ReplayError("The whisper.cpp JSON has no transcription")
    tokens: list[TimedToken] = []
    pending = b""
    start = 0.0
    warped = False
    for segment in segments:
        for text, at, is_warped, plain_end in segment_tokens(record(segment)):
            if not pending:
                start, warped = at, is_warped
            pending += text
            try:
                decoded = pending.decode("utf-8")
            except UnicodeDecodeError:
                continue
            pending = b""
            tokens.append(
                TimedToken(decoded, start, start if warped else max(plain_end, start), warped)
            )
    return opened(tokens)


def local_beat_seconds(spans: Sequence[tuple[float, int]], index: int) -> float:
    """How long a beat takes around token ``index``: the median over nearby tokens said without a
    pause, since a token after a pause has no span of its own to measure."""
    first = max(index - LOCAL_RATE_TOKENS, 1)
    last = min(index + LOCAL_RATE_TOKENS + 1, len(spans))
    rates = [
        (spans[near][0] - spans[near - 1][0]) / spans[near][1]
        for near in range(first, last)
        if near != index and spans[near][1] > 0
    ]
    usable = [rate for rate in rates if 0 < rate <= LONGEST_ONSET_SECONDS_PER_BEAT]
    return statistics.median_high(usable) if usable else LONGEST_ONSET_SECONDS_PER_BEAT


def opened(tokens: Sequence[TimedToken]) -> list[TimedToken]:
    """Warped tokens with a start: ``t_dtw`` read as each token's end. Others keep their span."""
    spans: list[tuple[float, int]] = []
    latest = 0.0
    for token in tokens:
        latest = max(latest, token.end)
        spans.append((latest, spoken_beats(token.text.strip())))
    result: list[TimedToken] = []
    previous_end = 0.0
    for index, token in enumerate(tokens):
        if token.end > token.start or not token.warped:
            previous_end = max(previous_end, token.end)
            result.append(token)
            continue
        end = spans[index][0]
        length = spans[index][1] * local_beat_seconds(spans, index)
        result.append(TimedToken(token.text, max(end - length, previous_end), end, True))
        previous_end = end
    return result


def timed_characters(tokens: Sequence[TimedToken]) -> list[tuple[str, float]]:
    """One time for each spoken character of ``tokens``."""
    characters: list[tuple[str, float]] = []
    for token in tokens:
        spoken = [c for c in token.text if not c.isspace()]
        step = (token.end - token.start) / max(len(spoken), 1)
        characters.extend((c, token.start + step * index) for index, c in enumerate(spoken))
    return characters


def matched_characters(text: str, other: str) -> list[int | None]:
    """For each character of ``text``, the index in ``other`` of the character it matches: a longest
    common subsequence inside a band, since both are decodings of the same audio."""
    rows, columns = len(text), len(other)
    band = ALIGNMENT_BAND_CHARACTERS + abs(rows - columns)
    width = 2 * band + 1
    steps = bytearray((rows + 1) * width)
    previous = [0] * (width + 2)
    for row in range(1, rows + 1):
        current = [0] * (width + 2)
        character = text[row - 1]
        offset = row * width
        for slot in range(max(0, band - row + 1), min(width, columns + band - row + 1)):
            column = row + slot - band
            up, left = previous[slot + 2], current[slot]
            same = character == other[column - 1]
            diagonal = previous[slot + 1] + same
            if same and diagonal >= max(up, left):
                current[slot + 1], steps[offset + slot] = diagonal, STEP_MATCH
            elif up >= left:
                current[slot + 1], steps[offset + slot] = up, STEP_UP
            else:
                current[slot + 1], steps[offset + slot] = left, STEP_LEFT
        previous = current
    matched: list[int | None] = [None] * rows
    row, column = rows, columns
    while row > 0 and column > 0:
        slot = column + band - row
        step = steps[row * width + slot] if 0 <= slot < width else 0
        if step == STEP_MATCH:
            matched[row - 1] = column - 1
            row, column = row - 1, column - 1
        elif step == STEP_LEFT:
            column -= 1
        else:
            row -= 1
    return matched


def time_at(position: int, anchors: Sequence[tuple[int, float]]) -> float | None:
    """The time of character ``position``, between the matched characters around it."""
    low, high = 0, len(anchors)
    while low < high:
        middle = (low + high) // 2
        if anchors[middle][0] < position:
            low = middle + 1
        else:
            high = middle
    if low < len(anchors) and anchors[low][0] == position:
        return anchors[low][1]
    before = anchors[low - 1] if low else None
    after = anchors[low] if low < len(anchors) else None
    if before is not None and after is not None:
        share = (position - before[0]) / (after[0] - before[0])
        return before[1] + (after[1] - before[1]) * share
    if before is not None:
        return before[1]
    return after[1] if after is not None else None


def floor_ms(seconds: float) -> float:
    """Rounded down to the millisecond labels are written in, so rounding cannot leave the audio."""
    return math.floor(seconds * MILLIS_PER_SECOND) / MILLIS_PER_SECOND


def closed(
    labels: Sequence[dict[str, object]], starts: Sequence[float], duration: float
) -> list[dict[str, object]]:
    """Labels on their new starts, never stepping back: each ends where the next starts or when its
    characters have been said, whichever comes first, none is empty, and all lie in the audio."""
    moved: list[float] = []
    latest = 0.0
    for start in starts:
        latest = max(start, latest)
        moved.append(latest)
        latest += SHORTEST_LABEL_SECONDS
    result: list[dict[str, object]] = []
    for index, (label, unbounded) in enumerate(zip(labels, moved, strict=True)):
        start = min(unbounded, duration - SHORTEST_LABEL_SECONDS)
        said = start + len(str(label["text"]).strip()) * LONGEST_SPOKEN_CHARACTER_SECONDS
        end = min(said, moved[index + 1]) if index + 1 < len(moved) else said
        kept = {key: value for key, value in label.items() if key != "onsetSnap"}
        result.append({
            **kept,
            "start": min(round(start, 3), floor_ms(duration - SHORTEST_LABEL_SECONDS)),
            "end": min(round(max(end, start + SHORTEST_LABEL_SECONDS), 3), floor_ms(duration)),
        })  # fmt: skip
    return result


def retimed(
    labels: Sequence[dict[str, object]], tokens: Sequence[TimedToken], duration: float
) -> tuple[list[dict[str, object]], int, int]:
    """Labels on the tokens' clock, with how many of their characters matched and how many exist.
    A character only one recognition heard shares the time between its matched neighbours."""
    other = timed_characters(tokens)
    own = "".join(c for label in labels for c in str(label["text"]) if not c.isspace())
    if not other or not own:
        raise ReplayError("The timing recognition or the labels have no text")
    keys = "".join(c for c, _ in other)
    matched = matched_characters(own, keys)
    anchors = [(index, other[at][1]) for index, at in enumerate(matched) if at is not None]
    starts: list[float] = []
    position = 0
    for label in labels:
        found = time_at(position, anchors)
        if found is None:
            raise ReplayError("No character of the labels matched the timing recognition")
        starts.append(found)
        position += sum(not c.isspace() for c in str(label["text"]))
    return closed(labels, starts, duration), len(anchors), len(own)


def recognize(wav: Path, binary: Path, model: Path, work: Path) -> bytes:
    """The warped recognition's JSON; a finished one in ``work`` is reused."""
    base = work / "dtw"
    output = base.with_suffix(".json")
    if not output.exists():
        work.mkdir(mode=0o700, parents=True, exist_ok=True)
        partial = work / "partial"
        subprocess.run(
            [str(binary), "-m", str(model), "-f", str(wav), "-l", LANGUAGE, "-ojf", "-pp",
             *TOKEN_TIMING_ARGS, "-of", str(partial)],
            check=True, stdout=subprocess.DEVNULL,
        )  # fmt: skip
        partial.with_suffix(".json").replace(output)
    return output.read_bytes()


def audio_seconds(wav: Path) -> float:
    with wave.open(str(wav), "rb") as stream:
        return stream.getnframes() / stream.getframerate()


def write(
    labels_path: Path, raw: bytes, duration: float, model_sha256: str, output: Path
) -> dict[str, object]:
    """The labels' own document with warped times, refused when a recognition did not match."""
    document = record(json.loads(labels_path.read_text(encoding="utf-8")))
    labels = document["labels"]
    if not isinstance(labels, list):
        raise ReplayError(f"{labels_path} has no labels")
    moved, matched, total = retimed([record(item) for item in labels], parse_tokens(raw), duration)
    timed = {
        **document,
        "labels": moved,
        "parentLabelsFile": str(labels_path),
        "parentLabelsSha256": file_digest(labels_path),
        "tokenTiming": {
            "version": VERSION, "method": "whisper.cpp-dtw", "modelSha256": model_sha256,
            "matchedCharacters": matched, "characters": total,
        },
    }  # fmt: skip
    staged = output.with_name(f".{output.name}.partial")
    staged.write_text(json.dumps(timed, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    staged.replace(output)
    return record(timed["tokenTiming"])


def verified(path: Path, sha256: str) -> Path:
    if file_digest(path) != sha256:
        raise ReplayError(f"{path} is not the pinned file")
    return path


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("labels", "wav", "binary", "binary_sha256", "model", "model_sha256", "work",
                 "output"):  # fmt: skip
        parser.add_argument(name)
    args = parser.parse_args(argv)
    binary = verified(Path(args.binary), args.binary_sha256)
    model = verified(Path(args.model), args.model_sha256)
    raw = recognize(Path(args.wav), binary, model, Path(args.work))
    duration = audio_seconds(Path(args.wav))
    print(json.dumps(write(Path(args.labels), raw, duration, args.model_sha256, Path(args.output))))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
