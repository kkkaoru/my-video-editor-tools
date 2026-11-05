"""A multi-speaker delivery's version-2 caption plan (``docs/caption-plan.md`` "Multiple speakers").

The multi-speaker flow (``replay.speaker_subtitles`` cues on the delivery clock) gets the same
live-drawn captions as one speaker: ``<movie stem>.captions.json`` version 2 beside the delivery,
each speaker's icon pinned beside it (``<movie stem>.speaker-<id>.png``) and the web viewer's
``<transcript>.speakers.json`` derived from the plan's speaker table. The single-speaker
``replay.caption_plan`` is shared only for validation and writing.

- **Titles.** A speaker's consecutive cues that grow one text (each starting where the previous
  ends and leading with its whole text) are one revealing title, as ``motion_reveal_stages``
  chains them: each grapheme appears on the first frame of the stage that shows it. Different
  speakers are never merged. A cue shows on the frames ``ceil(seconds * rate)`` its times start
  and end on, as the ASS burn shows it; a title no frame shows is left out and counted.
- **Speakers.** ``left``, ``middle`` and ``right`` in lane order with the lanes of
  ``speaker_subtitles.STYLES`` (scaled to the canvas width), coloured by the given colour or the
  lane fill (``colorBottom``) over that colour mixed halfway to white (``colorTop``).
  ``unassigned`` cues are grey, centred titles without a speaker.
- **Style.** Style 20 (the recording pipeline's 82.9558 pt finish) scaled to 40 pt a 1920 px
  canvas, so a lane holds about 12 characters, with the unassigned grey stops.
- **Position.** Every title is written at its lane's bottom base, unstacked: the position
  ``caption_layout_file.resolve`` gives in ``bottom`` mode with the default margin. Readers
  stack overlapping unassigned titles.

    python -m replay.speaker_caption_plan build CUES.csv MOVIE (--burned | --unburned)
        [--speakers SPEAKERS.json] [--ffprobe FFPROBE]

``SPEAKERS.json`` is ``{"speakers": [{"id", "name", "color"?, "icon"?, "iconSha256"?}]}`` (an
absolute icon path with its SHA-256); without it the lanes are named 左, 中央 and 右. Fewer than
two speakers is refused: that is a single-speaker plan.
"""

import argparse
import hashlib
import json
import math
from collections.abc import Sequence
from fractions import Fraction
from pathlib import Path

import regex

from replay import caption_layout_file, caption_plan
from replay.caption_native_request import style
from replay.caption_plan import graphemes, publish_once, read_json, rows
from replay.core import ReplayError
from replay.recordings import record
from replay.speaker_subtitles import (
    LANE_FILLS,
    SPEAKER_NAMES,
    STYLES,
    SpeakerCue,
    clean_text,
    read_cues,
    wrap_text,
)

LANES = ("left", "middle", "right")
REFERENCE_WIDTH = 1920  # the canvas ``speaker_subtitles.STYLES`` places its lanes on
LANE_FONT_SIZE = 40.0  # at the reference width
STYLE20_FONT_SIZE, STYLE20_STROKE, STYLE20_SHADOW = (
    82.95580110497238, 0.4713397790055249, 0.9426795580110497,
)  # fmt: skip
UNASSIGNED_COLOURS = {"gradientTop": "#E4E4E4", "gradientBottom": "#C8C8C8"}
TABLE_KEYS = frozenset({"id", "name"})
TABLE_OPTIONAL = frozenset({"color", "icon", "iconSha256"})
SPEAKERS_SUFFIX = ".srt.speakers.json"


def hex_colour(rgb: Sequence[int]) -> str:
    return "#" + "".join(f"{channel:02X}" for channel in rgb)


def lighter(colour: str) -> str:
    """``colour`` mixed halfway to white, each channel ``(c + 256) // 2`` (style 20's rule)."""
    return hex_colour([(int(colour[index : index + 2], 16) + 256) // 2 for index in (1, 3, 5)])


def lane_style(width: int) -> dict[str, object]:
    """Style 20 scaled to the lane font size on a ``width``-pixel canvas, in unassigned grey."""
    size = LANE_FONT_SIZE * width / REFERENCE_WIDTH
    scale = size / STYLE20_FONT_SIZE
    return style(font_size=size, stroke_scale=STYLE20_STROKE * scale,
                 shadow_scale=STYLE20_SHADOW * scale, palette=UNASSIGNED_COLOURS)  # fmt: skip


def lane_base(height: int, size: float) -> float:
    """A one-line title's Motion y at the bottom: ``bottom`` mode with the default margin."""
    box = caption_layout_file.EM_LINE_HEIGHT * size
    placed = height - caption_layout_file.DEFAULTS["margin"] - box
    return height / 2 - (
        placed + caption_layout_file.PADDING + caption_layout_file.EM_ASCENT * size
    )


def speaker_table(value: object, used: set[str]) -> list[dict[str, object]]:
    """The validated table in lane order, each speaker with its colours and lane; every used
    speaker must be in it."""
    table = rows(record(value)["speakers"], "speakers")
    for item in table:
        if not TABLE_KEYS <= set(item) <= TABLE_KEYS | TABLE_OPTIONAL or item["id"] not in LANES:
            raise ReplayError(f"A speaker has id ({', '.join(LANES)}), name, color?, icon?")
        if ("icon" in item) != ("iconSha256" in item):
            raise ReplayError(f"Speaker {item['id']!r} needs its icon with its SHA-256")
        if "icon" in item and not Path(str(item["icon"])).is_absolute():
            raise ReplayError(f"Speaker {item['id']!r} icon must be an absolute path")
    ids = [str(item["id"]) for item in table]
    if ids != sorted(ids, key=LANES.index) or len(set(ids)) != len(ids):
        raise ReplayError("The speaker table lists each lane once, left to right")
    if not used <= set(ids):
        raise ReplayError(f"Speakers {sorted(used - set(ids))} are not in the speaker table")
    return table


def default_table(used: set[str]) -> dict[str, object]:
    return {"speakers": [{"id": lane, "name": SPEAKER_NAMES[lane]} for lane in LANES
                         if lane in used]}  # fmt: skip


def frame_of(seconds: float, rate: int) -> int:
    """The first frame shown at or after ``seconds`` (cue times are centiseconds)."""
    return math.ceil(Fraction(f"{seconds:.6f}") * rate)


def chains(cues: Sequence[SpeakerCue]) -> list[list[SpeakerCue]]:
    """One speaker's cues, in time order, grouped into growing texts."""
    grouped: list[list[SpeakerCue]] = []
    for cue in sorted(cues, key=lambda item: (item.start, item.end)):
        text = clean_text(cue.text).replace("\n", "")
        if grouped:
            last = grouped[-1][-1]
            before = clean_text(last.text).replace("\n", "")
            if cue.start == last.end and len(text) > len(before) and text.startswith(before):
                grouped[-1].append(cue)
                continue
        grouped.append([cue])
    return grouped


def title(chain: list[SpeakerCue], rate: int) -> dict[str, object] | None:
    """The chain's revealing title (its final text wrapped as the ASS burn wraps it), or None
    when no frame shows it."""
    start, end = frame_of(chain[0].start, rate), frame_of(chain[-1].end, rate)
    if end <= start:
        return None
    text = wrap_text(clean_text(chain[-1].text))
    shown = [(graphemes(clean_text(cue.text).replace("\n", "")), frame_of(cue.start, rate))
             for cue in chain]  # fmt: skip
    onsets: list[int] = []
    count = 0  # graphemes of the flat text placed so far
    for grapheme in regex.findall(r"\X", text):
        if grapheme != "\n":
            count += 1
        # A line break appears with the grapheme before it.
        onsets.append(next(frame for length, frame in shown if length >= max(count, 1)))
    onsets = [min(max(frame, start), end - 1) for frame in onsets]
    return {"text": text, "startFrame": start, "endFrame": end, "graphemeFrames": onsets}


def build(
    cues: Sequence[SpeakerCue], table: object, movie: Path, *, burned: bool, ffprobe: str
) -> tuple[dict[str, object], dict[str, Path], int]:
    """The validated version-2 plan of ``movie``, the icon files to pin beside it (by their
    name there) and how many titles no frame shows."""
    used = {cue.speaker for cue in cues} - {"unassigned"}
    if len(used) < caption_plan.SPEAKERS_VERSION:
        raise ReplayError("Fewer than two speakers: that is a single-speaker plan")
    speakers = speaker_table(table, used)
    width, height, rate, frames = caption_plan.probed(movie, ffprobe)
    common = lane_style(width)
    base = lane_base(height, float(str(common["fontSize"])))
    lanes = {str(item["id"]): (STYLES[str(item["id"])][0] - REFERENCE_WIDTH / 2)
             * width / REFERENCE_WIDTH for item in speakers}  # fmt: skip
    titles: list[dict[str, object]] = []
    dropped = 0
    for speaker in [*LANES, "unassigned"]:
        for chain in chains([cue for cue in cues if cue.speaker == speaker]):
            shown = title(chain, rate)
            if shown is None:
                dropped += 1
                continue
            titles.append({**shown, "x": lanes.get(speaker, 0.0), "y": base,
                           **({"speaker": speaker} if speaker in lanes else {})})  # fmt: skip
    titles.sort(key=lambda item: int(str(item["startFrame"])))  # stable: lane order on ties
    icons: dict[str, Path] = {}
    table_out: list[dict[str, object]] = []
    for item in speakers:
        lane = str(item["id"])
        colour = str(item.get("color", hex_colour(LANE_FILLS[lane]))).upper()
        entry: dict[str, object] = {"id": lane, "name": item["name"], "colorTop": lighter(colour),
                                    "colorBottom": colour, "laneX": lanes[lane]}  # fmt: skip
        if "icon" in item:
            name = f"{movie.stem}.speaker-{lane}.png"
            icons[name] = Path(str(item["icon"]))
            entry |= {"icon": name, "iconSha256": item["iconSha256"]}
        table_out.append(entry)
    plan = caption_plan.validate({
        "version": caption_plan.SPEAKERS_VERSION, "clock": caption_plan.CLOCK,
        "movie": movie.name, "burned": burned, "width": width, "height": height, "rate": rate,
        "frames": frames, "style": common, "titles": titles, "nameplate": None,
        **caption_plan.margins(width, height),
        "speakers": table_out,
    })  # fmt: skip
    return plan, icons, dropped


def speakers_json(plan: dict[str, object]) -> dict[str, object]:
    """The web viewer's ``<transcript>.speakers.json``, derived from the plan's speakers."""
    return {"speakers": {str(item["id"]): {"label": item["name"], "color": item["colorBottom"]}
                         for item in rows(plan["speakers"], "speakers")}}  # fmt: skip


def write(plan: dict[str, object], movie: Path, icons: dict[str, Path]) -> list[Path]:
    """Pin each speaker icon (verified by SHA-256), then write the plan and the derived
    ``speakers.json`` beside the movie, each once."""
    caption_plan.validate(plan)
    written = []
    for item in rows(plan["speakers"], "speakers"):
        if "icon" not in item:
            continue
        data = icons[str(item["icon"])].read_bytes()
        if hashlib.sha256(data).hexdigest() != item["iconSha256"]:
            raise ReplayError(f"{icons[str(item['icon'])]} is not speaker {item['id']}'s icon")
        publish_once(movie.with_name(str(item["icon"])), data)
        written.append(movie.with_name(str(item["icon"])))
    written.append(caption_plan.write(plan, movie.parent))
    derived = movie.with_name(f"{movie.stem}{SPEAKERS_SUFFIX}")
    text = json.dumps(speakers_json(plan), ensure_ascii=False, indent=1) + "\n"
    publish_once(derived, text.encode("utf-8"))
    return [*written, derived]


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    built = commands.add_parser("build")
    built.add_argument("cues", type=Path)
    built.add_argument("movie", type=Path)
    burned = built.add_mutually_exclusive_group(required=True)
    burned.add_argument("--burned", dest="burned", action="store_true")
    burned.add_argument("--unburned", dest="burned", action="store_false")
    built.add_argument("--speakers", type=Path)
    built.add_argument("--ffprobe", default="ffprobe")
    args = parser.parse_args(argv)
    cues = read_cues(args.cues)
    used = {cue.speaker for cue in cues} - {"unassigned"}
    table = read_json(args.speakers) if args.speakers else default_table(used)
    movie = args.movie.resolve()
    plan, icons, dropped = build(cues, table, movie, burned=args.burned, ffprobe=args.ffprobe)
    for path in write(plan, movie, icons):
        print(path)
    print(json.dumps({"titles": len(rows(plan["titles"], "titles")), "unshown": dropped}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
