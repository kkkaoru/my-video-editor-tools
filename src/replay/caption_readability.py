"""How long each burned caption shows its whole text, measured from the rendered parts' reveals.

A caption appears grapheme by grapheme with its speech (``motion_reveal_stages``), so the time a
viewer can read the finished text is the title's end minus its last grapheme's onset, not the
title's length. 11/1 under the earlier paging showed its whole text a median 0.10 s, 75 % of its
titles under 0.3 s (2026-11-03, "字幕の表示時間が短すぎて不快"). A title's last words stay readable
while the next title carries them (paging's ``carried``): when the next title of the part starts
within ``CONTINUATION_FRAMES`` of this one's end and leads with a suffix of its text that holds its
last grapheme, that title's length is added. ``caption-parts`` records each
part's titles (``<part>.reveal.json``); this reads them all and refuses a recording whose captions
are not readable: the median whole-text time must reach ``MINIMUM_MEDIAN`` and the share of
titles under ``FLASH`` must stay within ``MAXIMUM_FLASH_SHARE``.

    python -m replay.caption_readability MOTI_DIRECTORY [--rate 30]
"""

import argparse
import json
import statistics
import sys
from collections.abc import Sequence
from dataclasses import dataclass
from fractions import Fraction
from pathlib import Path

import regex

from replay.core import ReplayError
from replay.recordings import record

GRAPHEME = regex.compile(r"\X")
CONTINUATION_FRAMES = 2
FLASH = Fraction(3, 10)
# Netflix Timed Text Style Guide (general requirements): a subtitle event lasts at least 5/6 s.
MINIMUM_MEDIAN = Fraction(5, 6)
# 11/2 with the readable, carrying pages: 12.8 % of 2,375 titles under FLASH, median 1.2 s
# (80 % and 0.1 s before); the bound leaves room for faster speakers, not for a regression.
MAXIMUM_FLASH_SHARE = Fraction(1, 5)


@dataclass(frozen=True, slots=True)
class Title:
    """A recorded title: its text without line breaks, frames and last grapheme's onset."""

    text: str
    start: int
    end: int
    last: int


def readable(directory: Path, rate: int) -> list[Fraction]:
    """Each title's whole-text on-screen seconds, over every part's reveal record."""
    seconds: list[Fraction] = []
    for path in sorted(directory.glob("*.reveal.json")):
        titles = record(json.loads(path.read_text(encoding="utf-8"))).get("titles")
        if not isinstance(titles, list):
            raise ReplayError(f"{path.name} has no titles")
        shown = [shown_title(path, record(title)) for title in titles]
        for title, following in zip(shown, [*shown[1:], None], strict=True):
            frames = title.end - title.last
            if (
                following is not None
                and abs(following.start - title.end) <= CONTINUATION_FRAMES
                and carries(title.text, following.text)
            ):
                frames += following.end - following.start
            seconds.append(Fraction(frames, rate))
    if not seconds:
        raise ReplayError(f"No caption reveal records in {directory}")
    return seconds


def shown_title(path: Path, title: dict[str, object]) -> Title:
    """A recorded title, validated."""
    frames, text = title.get("graphemeFrames"), title.get("text")
    start, end = title.get("startFrame"), title.get("endFrame")
    if (
        not isinstance(frames, list)
        or not frames
        or not isinstance(text, str)
        or not isinstance(start, int)
        or not isinstance(end, int)
    ):
        raise ReplayError(f"{path.name} has a title without its frames")
    return Title(text.replace("\n", ""), start, end, max(int(str(frame)) for frame in frames))


def carries(text: str, following: str) -> bool:
    """Whether ``following`` leads with a suffix of ``text`` that holds its last grapheme."""
    marks = GRAPHEME.findall(text)
    return any(following.startswith("".join(marks[-count:])) for count in range(1, len(marks) + 1))


def summary(seconds: Sequence[Fraction]) -> dict[str, object]:
    flashes = Fraction(sum(1 for item in seconds if item < FLASH), len(seconds))
    median = Fraction(statistics.median(seconds))
    return {
        "titles": len(seconds),
        "medianSeconds": round(float(median), 3),
        "flashShare": round(float(flashes), 3),
        "readable": median >= MINIMUM_MEDIAN and flashes <= MAXIMUM_FLASH_SHARE,
    }


def check(directory: Path, rate: int) -> dict[str, object]:
    """The summary; a ReplayError when the captions are not readable."""
    found = summary(readable(directory, rate))
    if found["readable"] is not True:
        raise ReplayError(f"Captions show their whole text too briefly: {found}")
    return found


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--rate", type=int, default=30)
    args = parser.parse_args(argv)
    found = summary(readable(args.directory, args.rate))
    print(json.dumps(found))
    return 0 if found["readable"] is True else 1


if __name__ == "__main__":
    sys.exit(main())
