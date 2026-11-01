"""9:16 excerpts of a finished landscape delivery (``video-config.json`` ``verticals``).

The picture is the source's centre (the source height at 9:16, scaled to the 1080x1920 canvas)
with the character's own window (``inset``, source pixels) scaled alike and placed at the bottom
centre while it is on screen. The captions are the delivery's own pages placed again on the
vertical canvas, centred above the window and without the nameplate: the caption steps of
``replay.recording_caption_steps`` run unchanged on a sibling pipeline (``vertical-v1`` beside
the recording's ``pipeline.json``) whose upstream receipts and inputs are the landscape
recording's (hard links), so the cuts, pages and reveal timing are the delivery's. Each range is
then cut from that vertical movie with the published delivery's own audio (its music restored),
so picture and sound share one clock. Each excerpt is a recording folder of its own,
``<recording>-vertical-MMSS-MMSS/delivery/<stamp>_JST_<that folder>-review.mp4``, beside the
recording's in its delivery library, with the delivery's transcript cut to the same range (the
viewers list one canonical delivery per folder).

The window counts as on screen while its pixels resemble their own median over the recording
(normalized correlation of a grey 32x32 copy sampled each second, median over PRESENCE_SECONDS,
at least PRESENT): a full-screen scene or a black transition drops it.

    python -m replay.vertical_excerpt PIPELINE.json
"""

import json
import os
import re
import subprocess
import sys
from collections.abc import Sequence
from dataclasses import dataclass
from fractions import Fraction
from pathlib import Path

import numpy as np

from replay import video_config
from replay.core import ReplayError
from replay.recordings import record
from replay.source_lead_in import BITRATE, COLOUR, video_pts, video_stream
from replay.source_vertical import crop_width

CANVAS = (1080, 1920)
CAPTION_TOP = 120  # the default placement's region top
CAPTION_ORIGIN = 0  # unused when centred: the region is the full width, symmetric about it
CAPTION_GAP = 24  # between the lowest caption line and the window
WINDOW_BOTTOM = 48  # between the window and the canvas bottom
SAMPLE = 32
PRESENCE_SECONDS = 9
PRESENT = 0.5
AUDIO_BITRATE = "256k"
Cue = tuple[float, float, tuple[str, ...]]
SRT_TIME = re.compile(r"(\d+):(\d{2}):(\d{2}),(\d{3})")
SIBLING = "vertical-v1"
MEDIA_ROOT = "verticals"
# Receipts the re-placed captions depend on (recording_caption_steps.DEPENDENCIES).
UPSTREAM = ("paging", "yolo-track", "onsets", "cleanup")
LINKED = (
    "cut-plan-v1",
    "paging-v1/prepared/paging",
    "speech-onsets-v1.json",
    "video-clock.json",
    "delivery-audio-v1/tempo/candidate.wav",
    "delivery-audio-v1/tempo/receipt.json",
)
STEPS = ("placement", "caption-parts", "caption-render", "compose", "finish")


def even(value: float) -> int:
    return 2 * round(value / 2)


@dataclass(frozen=True, slots=True)
class Geometry:
    """Where the source's centre and its window land on the vertical canvas."""

    crop: tuple[int, int, int]  # width, height, x of the source centre
    inset: tuple[int, int, int, int]  # x, y, width, height in source pixels
    placed: tuple[int, int, int, int]  # x, y, width, height on the canvas

    def layout(self) -> dict[str, object]:
        """The caption layout: centred above the window, no nameplate. A centred caption is
        measured against a region from 0 to ``regionRight``, so the full width keeps it
        symmetric."""
        return {
            "version": 1, "canvas": list(CANVAS), "align": "centre", "originX": CAPTION_ORIGIN,
            "regionTop": CAPTION_TOP, "regionBottom": self.placed[1] - CAPTION_GAP,
            "regionRight": CANVAS[0],
        }  # fmt: skip


def geometry(size: tuple[int, int], inset: tuple[int, int, int, int]) -> Geometry:
    width, height = size
    x, y, inset_width, inset_height = inset
    if x + inset_width > width or y + inset_height > height:
        raise ReplayError("The inset lies outside the source frame")
    crop = crop_width(width, height, (9, 16))
    scale = CANVAS[0] / crop
    placed_width, placed_height = even(inset_width * scale), even(inset_height * scale)
    if placed_width > CANVAS[0] or placed_height + WINDOW_BOTTOM > CANVAS[1] // 2:
        raise ReplayError("The inset is too large for the bottom of the vertical canvas")
    return Geometry(
        (crop, height, even((width - crop) / 2)),
        inset,
        (even((CANVAS[0] - placed_width) / 2), CANVAS[1] - WINDOW_BOTTOM - placed_height,
         placed_width, placed_height),
    )  # fmt: skip


def presence(correlations: Sequence[float]) -> list[bool]:
    """Per second, whether the window is on screen (see the module docstring)."""
    half = PRESENCE_SECONDS // 2
    values = np.pad(np.asarray(correlations, dtype=np.float64), half, mode="edge")
    windows = np.lib.stride_tricks.sliding_window_view(values, PRESENCE_SECONDS)
    return [bool(item) for item in np.median(windows, axis=1) >= PRESENT]


def correlations(source: Path, inset: tuple[int, int, int, int], ffmpeg: str) -> list[float]:
    """Each second's grey window against the median window, as a normalized correlation."""
    x, y, width, height = inset
    raw = subprocess.run(
        [ffmpeg, "-v", "error", "-nostdin", "-i", str(source), "-vf",
         f"fps=1,crop={width}:{height}:{x}:{y},scale={SAMPLE}:{SAMPLE},format=gray",
         "-f", "rawvideo", "-"],
        capture_output=True, check=True,
    ).stdout  # fmt: skip
    frames = np.frombuffer(raw, np.uint8).reshape(-1, SAMPLE * SAMPLE).astype(np.float64)
    if not len(frames):
        raise ReplayError("The source gave no frames for the inset")
    normal = (frames - frames.mean(1, keepdims=True)) / (frames.std(1, keepdims=True) + 1e-6)
    template = np.median(normal, axis=0)
    template = (template - template.mean()) / (template.std() + 1e-6)
    return [float(item) for item in normal @ template / template.size]


def spans(flags: Sequence[bool]) -> list[tuple[int, int]]:
    """The [start, end) seconds of each run of True."""
    runs: list[tuple[int, int]] = []
    for second, flag in enumerate(flags):
        if flag and runs and runs[-1][1] == second:
            runs[-1] = (runs[-1][0], second + 1)
        elif flag:
            runs.append((second, second + 1))
    return runs


def enable(runs: Sequence[tuple[int, int]], start: float) -> str:
    """The overlay's ``enable`` expression for runs offset by the stream's first timestamp."""
    terms = [f"between(t,{start + a:.3f},{start + b:.3f})" for a, b in runs]
    return "+".join(terms) or "0"


def compose_command(
    source: Path,
    output: Path,
    shape: Geometry,
    expression: str,
    stream: dict[str, object],
    *,
    ffmpeg: str,
) -> list[str]:
    if "'" in expression:
        raise ReplayError("The overlay expression must not contain a quote")
    crop_w, crop_h, crop_x = shape.crop
    x, y, width, height = shape.inset
    placed_x, placed_y, placed_w, placed_h = shape.placed
    graph = (
        f"[0:v]split=2[a][b];"
        f"[a]crop={crop_w}:{crop_h}:{crop_x}:0,scale={CANVAS[0]}:{CANVAS[1]}:flags=lanczos[bg];"
        f"[b]crop={width}:{height}:{x}:{y},scale={placed_w}:{placed_h}:flags=lanczos[in];"
        f"[bg][in]overlay={placed_x}:{placed_y}:enable='{expression}'[v]"
    )
    colour = [
        argument
        for key, option in COLOUR
        if isinstance(stream.get(key), str) and stream[key] != "unknown"
        for argument in (option, str(stream[key]))
    ]
    return [
        ffmpeg, "-v", "error", "-nostdin", "-i", str(source), "-filter_complex", graph,
        "-map", "[v]", "-an", "-fps_mode", "passthrough",
        "-enc_time_base", str(Fraction(str(stream["time_base"]))),
        "-c:v", "hevc_videotoolbox", "-b:v", BITRATE, "-tag:v", "hvc1", *colour,
        "-movflags", "+faststart", str(output),
    ]  # fmt: skip


def composed(
    source: Path, directory: Path, inset: tuple[int, int, int, int], *, ffmpeg: str, ffprobe: str
) -> tuple[Path, Geometry]:
    """The vertical picture of the whole source, made once (its record names how)."""
    stream = video_stream(source, ffprobe)
    probe = subprocess.run(
        [ffprobe, "-v", "error", "-select_streams", "v", "-show_entries", "stream=width,height",
         "-of", "json", str(source)],
        capture_output=True, text=True, check=True,
    ).stdout  # fmt: skip
    found = record(json.loads(probe)["streams"][0])
    shape = geometry((int(str(found["width"])), int(str(found["height"]))), inset)
    output, receipt = directory / source.name, directory / "vertical.json"
    expected = {"source": str(source), "sourceSize": source.stat().st_size,
                "crop": list(shape.crop), "inset": list(shape.inset),
                "placed": list(shape.placed)}  # fmt: skip
    if receipt.exists() and output.exists():
        recorded = record(json.loads(receipt.read_text(encoding="utf-8")))
        if {key: recorded.get(key) for key in expected} == expected:
            return output, shape
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    pts = video_pts(source, ffprobe)
    start = float(pts[0] * Fraction(str(stream["time_base"])))
    runs = spans(presence(correlations(source, inset, ffmpeg)))
    partial = directory / f"partial-{output.name}"
    partial.unlink(missing_ok=True)
    subprocess.run(
        compose_command(source, partial, shape, enable(runs, start), stream, ffmpeg=ffmpeg),
        check=True,
    )
    if video_pts(partial, ffprobe) != pts:
        raise ReplayError("The vertical picture moved a source frame; it is not used")
    partial.rename(output)
    receipt.write_text(json.dumps({**expected, "insetSeconds": runs}, indent=1) + "\n", "utf-8")
    return output, shape


def link_tree(source: Path, target: Path) -> None:
    """Hard-link a file, or every file of a tree, to the same place under ``target``."""
    if source.is_file():
        target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        if not target.exists():
            os.link(source, target)
        return
    for path in source.rglob("*"):
        if path.is_file():
            link_tree(path, target / path.relative_to(source))


def sibling(pipeline_path: Path, picture: Path, layout: dict[str, object]) -> Path:
    """The vertical sibling pipeline: the recording's settings with its own work, steps,
    captions layout and clean picture; upstream receipts copied, inputs hard-linked."""
    data = record(json.loads(pipeline_path.read_text(encoding="utf-8")))
    root = pipeline_path.parent / SIBLING
    work, steps = Path(str(data["work"])), Path(str(data["steps"]))
    vertical = {**data, "work": str(root / "work"), "steps": str(root / "steps")}
    captions = record(json.loads((pipeline_path.parent / "captions.json").read_text("utf-8")))
    captions = {key: value for key, value in captions.items() if key != "voiceLoudness"}
    captions["layout"] = layout
    captions["mediaRoot"] = str(Path(str(captions["mediaRoot"])).parent / MEDIA_ROOT)
    (root / "steps" / "logs").mkdir(mode=0o700, parents=True, exist_ok=True)
    for name, value in (("pipeline.json", vertical), ("captions.json", captions)):
        (root / name).write_text(json.dumps(value, indent=1, ensure_ascii=False) + "\n", "utf-8")
    for step in UPSTREAM:
        receipt = record(json.loads((steps / f"{step}.json").read_text(encoding="utf-8")))
        if step == "cleanup":
            receipt["outputs"] = {**record(receipt["outputs"]), "cleanSource": str(picture)}
        (root / "steps" / f"{step}.json").write_text(json.dumps(receipt, indent=2) + "\n", "utf-8")
    for name in LINKED:
        link_tree(work / name, root / "work" / name)
    return root / "pipeline.json"


def excerpt(
    vertical: Path, delivery: Path, span: tuple[float, float], output: Path, *, ffmpeg: str
) -> None:
    """``span`` of the vertical movie with the delivery's own audio, frame-exact."""
    start, end = (f"{bound:.3f}" for bound in span)
    partial = output.with_name(f"partial-{output.name}")
    partial.unlink(missing_ok=True)
    subprocess.run(
        [ffmpeg, "-v", "error", "-nostdin", "-ss", start, "-to", end, "-i", str(vertical),
         "-ss", start, "-to", end, "-i", str(delivery), "-map", "0:v", "-map", "1:a",
         "-c:v", "hevc_videotoolbox", "-b:v", BITRATE, "-tag:v", "hvc1",
         "-c:a", "aac", "-b:a", AUDIO_BITRATE, "-movflags", "+faststart", str(partial)],
        check=True,
    )  # fmt: skip
    partial.rename(output)


def srt_seconds(stamp: str) -> float:
    match = SRT_TIME.fullmatch(stamp.strip())
    if match is None:
        raise ReplayError(f"Malformed transcript time: {stamp}")
    hours, minutes, seconds, millis = (int(part) for part in match.groups())
    return hours * 3600 + minutes * 60 + seconds + millis / 1000


def srt_stamp(seconds: float) -> str:
    minutes, millis = divmod(round(seconds * 1000), 60000)
    return f"{minutes // 60:02d}:{minutes % 60:02d}:{millis // 1000:02d},{millis % 1000:03d}"


def srt_cues(text: str) -> list[Cue]:
    """Every cue of an SRT transcript: start and end seconds and its text lines."""
    cues: list[Cue] = []
    for block in re.split(r"\n\s*\n", text.strip()):
        lines = block.splitlines()
        if len(lines) < 3 or "-->" not in lines[1]:
            raise ReplayError("The delivery transcript holds a malformed cue")
        first, last = (srt_seconds(part) for part in lines[1].split("-->"))
        cues.append((first, last, tuple(lines[2:])))
    return cues


def srt_text(cues: Sequence[Cue]) -> str:
    return (
        "\n\n".join(
            "\n".join([str(number), f"{srt_stamp(first)} --> {srt_stamp(last)}", *lines])
            for number, (first, last, lines) in enumerate(cues, start=1)
        )
        + "\n"
    )


def cues_in(cues: Sequence[Cue], span: tuple[float, float], offset: float) -> list[Cue]:
    """The cues inside ``span``, clipped to it and moved so the span starts at ``offset``."""
    start, end = span
    return [
        (max(first, start) - start + offset, min(last, end) - start + offset, lines)
        for first, last, lines in cues
        if last > start and first < end
    ]


def transcript_range(text: str, span: tuple[float, float]) -> str:
    """The cues of an SRT transcript inside ``span``, clipped to it and timed from its start."""
    return srt_text(cues_in(srt_cues(text), span, 0.0))


def clock(seconds: float) -> str:
    minutes, rest = divmod(round(seconds), 60)
    return f"{minutes:02d}{rest:02d}"


def run(given: Path) -> list[Path]:
    pipeline_path = given.resolve()
    data = record(json.loads(pipeline_path.read_text(encoding="utf-8")))
    edit_root, python = Path(str(data["editRoot"])), str(data["python"])
    ffmpeg, ffprobe = str(data["ffmpeg"]), str(data["ffprobe"])
    present, value = video_config.setting(edit_root, "verticals")
    if not present:
        raise ReplayError(f"{edit_root.name} asks for no verticals")
    inset, ranges = video_config.verticals(value)
    publish = record(json.loads((Path(str(data["steps"])) / "publish.json").read_text("utf-8")))
    delivery = Path(str(record(publish["outputs"])["delivery"]))
    captions = record(json.loads((pipeline_path.parent / "captions.json").read_text("utf-8")))
    media = Path(str(captions["mediaRoot"])).parent / MEDIA_ROOT / pipeline_path.parent.name
    picture, shape = composed(
        Path(str(data["source"])), media / "source", inset, ffmpeg=ffmpeg, ffprobe=ffprobe
    )
    vertical = sibling(pipeline_path, picture, shape.layout())
    for step in STEPS:
        if not (vertical.parent / "steps" / f"{step}.json").exists():
            subprocess.run(
                [python, "-B", "-m", "replay.recording_caption_steps", step, str(vertical)],
                check=True,
            )
    finish = record(json.loads((vertical.parent / "steps" / "finish.json").read_text("utf-8")))
    movie = Path(str(record(record(finish["outputs"])["movie"])["path"]))
    outputs = []
    recording, transcript = delivery.parents[3], delivery.with_suffix(".srt").read_text("utf-8")
    stamp = delivery.name.split("_JST_")[0]
    for span in ranges:
        folder = f"{recording.name}-vertical-{clock(span[0])}-{clock(span[1])}"
        output = recording.parent / folder / "delivery" / f"{stamp}_JST_{folder}-review.mp4"
        output.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        if not output.exists():
            excerpt(movie, delivery, span, output, ffmpeg=ffmpeg)
        output.with_suffix(".srt").write_text(transcript_range(transcript, span), "utf-8")
        outputs.append(output)
    return outputs


def main(argv: Sequence[str] | None = None) -> int:
    arguments = list(sys.argv[1:] if argv is None else argv)
    if len(arguments) != 1:
        raise SystemExit("Usage: python -m replay.vertical_excerpt PIPELINE.json")
    for path in run(Path(arguments[0])):
        print(path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
