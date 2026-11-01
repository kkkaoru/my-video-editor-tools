"""One clip joining chosen ranges of a finished delivery, listed by the viewers as its own video.

Each range edge that falls inside a caption moves out to that caption's own edge when it is at
most SNAP_SECONDS away (so no burned-in caption is cut mid-line), else to the nearer cue edge.
The ranges are joined in order, each with a short audio fade so the joins do not click. The clip
is a recording folder beside the delivery's, ``<folder>-<name>/delivery/<stamp>_JST_<folder>-
<name>-review.mp4``, with the delivery's transcript cut and joined the same way and a record of
the requested and adjusted ranges (``highlight.json``).

    python -m replay.highlight_clip DELIVERY.mp4 NAME START-END [START-END ...]

Times are ``M:SS`` or seconds, on the delivery's own clock.
"""

import json
import subprocess
import sys
from collections.abc import Sequence
from pathlib import Path

from replay.core import ReplayError
from replay.source_lead_in import BITRATE
from replay.vertical_excerpt import AUDIO_BITRATE, Cue, cues_in, srt_cues, srt_text

SNAP_SECONDS = 3.0
FADE_SECONDS = 0.015
RECORD = "highlight.json"

Span = tuple[float, float]


def seconds(text: str) -> float:
    minutes, _, rest = text.rpartition(":")
    try:
        return (int(minutes) * 60 if minutes else 0) + float(rest)
    except ValueError as error:
        raise ReplayError(f"Unreadable time: {text}") from error


def span(text: str) -> Span:
    start, separator, end = text.partition("-")
    if not separator or seconds(start) >= seconds(end):
        raise ReplayError(f"A range is START-END with START before END: {text}")
    return seconds(start), seconds(end)


def snapped(edge: float, cues: Sequence[Cue], *, start: bool) -> float:
    """``edge`` moved out of the cue it cuts, if any (see the module docstring)."""
    for first, last, _ in cues:
        if first < edge < last:
            outward = first if start else last
            if abs(outward - edge) <= SNAP_SECONDS:
                return outward
            return first if edge - first <= last - edge else last
    return edge


def adjusted(spans: Sequence[Span], cues: Sequence[Cue], duration: float) -> list[Span]:
    result = [
        (max(0.0, snapped(a, cues, start=True)), min(duration, snapped(b, cues, start=False)))
        for a, b in spans
    ]
    if any(b0 > a1 for (_, b0), (a1, _) in zip(result, result[1:], strict=False)):
        raise ReplayError("The ranges overlap once kept captions whole; give them in order")
    return result


def joined_cues(cues: Sequence[Cue], spans: Sequence[Span]) -> list[Cue]:
    joined: list[Cue] = []
    offset = 0.0
    for item in spans:
        joined += cues_in(cues, item, offset)
        offset += item[1] - item[0]
    return joined


def command(movie: Path, spans: Sequence[Span], output: Path, *, ffmpeg: str) -> list[str]:
    parts, labels = [], []
    for index, (start, end) in enumerate(spans):
        out = end - start - FADE_SECONDS
        fade = f"afade=t=in:d={FADE_SECONDS},afade=t=out:st={out:.3f}:d={FADE_SECONDS}"
        parts += [
            f"[0:v]trim={start:.3f}:{end:.3f},setpts=PTS-STARTPTS[v{index}]",
            f"[0:a]atrim={start:.3f}:{end:.3f},asetpts=PTS-STARTPTS,{fade}[a{index}]",
        ]
        labels.append(f"[v{index}][a{index}]")
    graph = ";".join([*parts, f"{''.join(labels)}concat=n={len(spans)}:v=1:a=1[v][a]"])
    return [
        ffmpeg, "-v", "error", "-nostdin", "-i", str(movie), "-filter_complex", graph,
        "-map", "[v]", "-map", "[a]", "-c:v", "hevc_videotoolbox", "-b:v", BITRATE,
        "-tag:v", "hvc1", "-c:a", "aac", "-b:a", AUDIO_BITRATE, "-movflags", "+faststart",
        str(output),
    ]  # fmt: skip


def duration(movie: Path, ffprobe: str) -> float:
    return float(
        subprocess.run(
            [
                ffprobe,
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "csv=p=0",
                str(movie),
            ],
            capture_output=True,
            text=True,
            check=True,
        ).stdout  # fmt: skip
    )


def make(movie: Path, name: str, requested: Sequence[Span], *, ffmpeg: str, ffprobe: str) -> Path:
    folder_name = movie.parents[1].name
    stamp, separator, _ = movie.name.partition("_JST_")
    if not separator or movie.parent.name != "delivery" or not name.isascii() or "/" in name:
        raise ReplayError("Give a delivery movie (<stamp>_JST_… in a delivery folder) and a name")
    cues = srt_cues(movie.with_suffix(".srt").read_text(encoding="utf-8"))
    spans = adjusted(requested, cues, duration(movie, ffprobe))
    folder = f"{folder_name}-{name}"
    output = movie.parents[2] / folder / "delivery" / f"{stamp}_JST_{folder}-review.mp4"
    if output.exists():
        raise ReplayError(f"{output} exists; choose another name")
    output.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    partial = output.with_name(f"partial-{output.name}")
    partial.unlink(missing_ok=True)
    subprocess.run(command(movie, spans, partial, ffmpeg=ffmpeg), check=True)
    partial.rename(output)
    output.with_suffix(".srt").write_text(srt_text(joined_cues(cues, spans)), encoding="utf-8")
    (output.parents[1] / RECORD).write_text(
        json.dumps(
            {"version": 1, "source": str(movie), "requested": [list(item) for item in requested],
             "adjusted": [list(item) for item in spans]}, indent=1,
        ) + "\n",
        encoding="utf-8",
    )  # fmt: skip
    return output


def main(argv: Sequence[str] | None = None) -> int:
    arguments = list(sys.argv[1:] if argv is None else argv)
    if len(arguments) < 3:
        raise SystemExit("Usage: python -m replay.highlight_clip DELIVERY.mp4 NAME START-END ...")
    movie, name, *ranges = arguments
    print(make(Path(movie), name, [span(item) for item in ranges], ffmpeg="ffmpeg",
               ffprobe="ffprobe"))  # fmt: skip
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
