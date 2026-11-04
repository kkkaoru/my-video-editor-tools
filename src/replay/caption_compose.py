"""Compose the clean source and the alpha caption parts into the final video, resumably.

The generic form of the per-recording ``compose_prepare.py``, ``worker-*.sh``, ``finish.sh``
and ``verify_join2.py`` scripts:

- ``plan``: split the cut list into ``media_edit`` requests within the schema limits
  (``MAX_CLIPS`` clips, ``MAX_LAYERS`` alpha layers, ``MAX_GROUP_SECONDS`` per request); a cut
  is subdivided wherever more alpha parts would overlap one request. For each alpha part the
  render used is the one whose packet count equals its planned frames;
- ``render``: run the group requests, ``workers`` at a time. A group whose output already holds
  its planned frames is reused; an unfinished group's partial output is removed and redone;
- ``finish``: encode the tempo audio once (``render --tempo`` encodes it beside the render) and
  join the groups losslessly with it (``native_join`` checks, verified packet count), the index
  reserved at the front instead of ``+faststart``'s second pass over the file. A finished join
  is reused.

A recording's caption layout may give another frame (``plan --canvas``) and a nameplate movie
(``plan --badge``, ``replay.caption_layout``) laid over every group from its start, which then
holds one alpha part less. A delivery of the original mix levels it by its voice
(``--loudness-reference``, the tempo voice): the gain toward the target is measured on the voice
and applied to the mix, whose peaks the limiter holds at the ceiling.

Every request and output stays under the pipeline's directories; inputs are only read.
"""

import argparse
import json
import math
import os
import re
import shutil
import subprocess
import sys
from collections.abc import Callable, Sequence
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from fractions import Fraction
from pathlib import Path
from tempfile import TemporaryDirectory

from replay import frame_slots
from replay.core import ReplayError
from replay.delivery_media import read_clock
from replay.media import quicktime_video_tags
from replay.native_join import PartFile, concat_list, probe_video, verify_parts
from replay.recordings import record
from replay.replay_preparation_plan import DERIVATION_RECORDS

RATE = 30
MAX_CLIPS = 60
MAX_LAYERS = 16
# The native media_edit server stops an edit after 300 s; a 300 s group rendered near real time
# and timed out under load (10/6, 2026-10-23), so a group stays well inside that budget.
MAX_GROUP_SECONDS = 120
WIDTH, HEIGHT = 1080, 1920
# The encode rate follows the recording's original: SOURCE_RATE_FACTOR times its video bits per
# pixel at the output size, within [MIN_BIT_RATE, MAX_BIT_RATE]. Against an 80 Mb/s reference
# (2026-10-23; TikTok 3.26 and 4.85 Mb/s originals, a YouTube AV1 original) every chosen rate
# kept VMAF within 1.5 of the former fixed 20 Mb/s, its 1st percentile >= 95 and caption-band
# SSIM within 0.005; at 8 Mb/s the YouTube window lost 0.0054, hence the 10 Mb/s floor.
SOURCE_RATE_FACTOR = 2.5
MIN_BIT_RATE = 10_000_000
MAX_BIT_RATE = 20_000_000
PROBE_TIMEOUT = 120
REQUEST_TIMEOUT = 7200
# Ticks of the native 1/60000 s timescale a retry ends each clip early by (far below a frame).
TRIM_TICKS = 2
NATIVE_TICKS = 60000
# How far, in native ticks, the last clip may move to end on the reader's range (2000 = 1 frame).
ALIGN_TICKS = 8
# A clip stops this far inside the source's video track, so aligning its end stays inside too.
END_MARGIN_SECONDS = ALIGN_TICKS / NATIVE_TICKS
AUDIO_RATE = 44100
AUDIO_ARGUMENTS = ("-c:a", "aac", "-b:a", "256k", "-ar", str(AUDIO_RATE), "-ac", "2")
# One EBU R128 pass over the tempo voice (integrated loudness, true peak) sets one static gain.
INTEGRATED = re.compile(r"\bI:\s+(-?[0-9]+(?:\.[0-9]+)?|-inf) LUFS")
TRUE_PEAK = re.compile(r"\bPeak:\s+(-?[0-9]+(?:\.[0-9]+)?|-inf) dBFS")
LOUDNESS_TIMEOUT = 3600
OVERSAMPLED_RATE = 4 * AUDIO_RATE
# Headroom below the ceiling for the AAC encode's overshoot: a −1.5 dBFS limit at 4x delivered
# −1.0 dBTP on a quiet recording's lifted voice (2026-10-24), so the limit sits 1 dB below.
LIMITER_MARGIN_DB = 1.0
LIMITER_ATTACK_MS = 1
LIMITER_RELEASE_MS = 10
MAXIMUM_CORRECTION_DB = 3.0
# The joined index reserved ahead of the media: each video frame and its ~1.44 AAC packets cost
# at most 32 bytes of sample tables apiece (Eg9 measured 36 bytes per frame in all). +faststart
# instead rewrote the whole joined file (21 GB: 222 s against 124 s, identical packets).
MOOV_BYTES_PER_FRAME = 96
MOOV_MARGIN = 1 << 20

Packets = Callable[[Path], int]
Call = Callable[[Path, Path], None]


@dataclass(frozen=True, slots=True)
class Loudness:
    """Voice level policy: one static gain toward ``target`` LUFS (integrated), lowered so the
    true peak stays at or below ``ceiling`` dBTP; no compression or limiting."""

    target: float
    ceiling: float


@dataclass(frozen=True, slots=True)
class Cut:
    """Source frames ``[start, end)`` placed at output frame ``output``, each slot read
    ``phase`` slots after its middle (``frame_slots``)."""

    start: int
    end: int
    output: int
    phase: Fraction = Fraction(0)

    @property
    def length(self) -> int:
        return self.end - self.start


@dataclass(frozen=True, slots=True)
class Part:
    """An alpha caption part covering output frames ``[offset, offset + frames)``."""

    name: str
    offset: int
    frames: int
    render: Path

    @property
    def end(self) -> int:
        return self.offset + self.frames


def seconds(frames: int) -> float:
    return float(Fraction(frames, RATE))


def sampled(frames: int) -> float:
    """The middle of frame slot ``frames``: where a selection reads that slot's frame.

    A selection that starts exactly on a frame boundary may be rounded to the previous frame,
    and a variable-rate source presents frames a fraction of a slot away from the grid; the
    middle of the slot falls inside the intended frame in both cases.
    """
    return float((Fraction(frames) + Fraction(1, 2)) / RATE)


def packet_count(path: Path) -> int:
    """Video packets of ``path``, or -1 when it cannot be probed."""
    try:
        return probe_video(path).packets
    except ReplayError:
        return -1


def load_cuts(path: Path) -> list[Cut]:
    rows = record(json.loads(path.read_text(encoding="utf-8"))).get("cuts")
    if not isinstance(rows, list) or not rows:
        raise ReplayError("The cut list is empty")
    cuts = []
    for row in rows:
        item = record(row)
        cuts.append(
            Cut(
                int(str(item["source_start_frame"])),
                int(str(item["source_end_frame"])),
                int(str(item["output_start_frame"])),
            )
        )
    return cuts


def complete_render(directory: Path, frames: int, packets: Packets) -> Path:
    """The newest render under ``directory`` whose packet count equals the planned frames."""
    found = [
        movie
        for movie in sorted(directory.glob("compressor-*/motion-alpha*.mov"))
        if movie.stat().st_size > 0 and packets(movie) == frames
    ]
    if not found:
        raise ReplayError(f"No complete render under {directory}")
    return found[-1]


def recorded_render(state: Path, directory: Path, frames: int, packets: Packets) -> Path:
    """The output a Compressor job state records as successful, holding the planned frames.

    A movie left by a job Compressor reported as failed is never used, even when complete.
    """
    saved = record(json.loads(state.read_text(encoding="utf-8")))
    output = Path(str(saved.get("output")))
    if saved.get("status") != "Successful" or not output.is_relative_to(directory):
        raise ReplayError(f"No successful Compressor render recorded in {state}")
    if not output.is_file() or packets(output) != frames:
        raise ReplayError(f"The recorded render {output} does not hold its planned frames")
    return output


def load_parts(path: Path, packets: Packets, states: Path | None = None) -> list[Part]:
    """The planned parts and their renders; with ``states``, only recorded successful renders."""
    parts = []
    for row in json.loads(path.read_text(encoding="utf-8")):
        item = record(row)
        name, frames = str(item["name"]), int(str(item["frames"]))
        directory = Path(str(item["renderDir"]))
        render = (
            complete_render(directory, frames, packets)
            if states is None
            else recorded_render(states / f"{name}.json", directory, frames, packets)
        )
        parts.append(Part(name, int(str(item["offsetFrame"])), frames, render))
    return parts


def overlapping(parts: Sequence[Part], low: int, high: int) -> int:
    return sum(1 for part in parts if part.offset < high and part.end > low)


def split_cut(cut: Cut, parts: Sequence[Part], layers: int = MAX_LAYERS) -> list[Cut]:
    """The cut in pieces of at most ``MAX_GROUP_SECONDS`` and ``layers`` overlapping parts."""
    pieces: list[Cut] = []
    position = 0
    while position < cut.length:
        low = cut.output + position
        high = min(cut.output + cut.length, low + MAX_GROUP_SECONDS * RATE)
        while overlapping(parts, low, high) > layers:
            edges = sorted({x for part in parts for x in (part.offset, part.end) if low < x < high})
            fitting = [x for x in edges if overlapping(parts, low, x) <= layers]
            if not fitting:
                raise ReplayError("More alpha parts overlap one frame than a request allows")
            high = max(fitting)
        piece = high - low
        pieces.append(Cut(cut.start + position, cut.start + position + piece, low, cut.phase))
        position += piece
    return pieces


def phased(cuts: Sequence[Cut], frame_clock: Path, source: Path, packets: Packets) -> list[Cut]:
    """The cuts split where the reading phase that shows every slot's frame changes."""
    clock = read_clock(frame_clock)
    if packets(source) != len(clock.pts):
        raise ReplayError("The frame clock does not describe the composed source")
    windows = frame_slots.phase_windows(clock.pts, clock.time_base, RATE)
    return [
        Cut(run.start, run.end, cut.output + run.start - cut.start, run.phase)
        for cut in cuts
        for run in frame_slots.runs(windows, cut.start, cut.end)
    ]


def group_bounds(
    cuts: Sequence[Cut], parts: Sequence[Part], layers: int = MAX_LAYERS
) -> list[tuple[int, int]]:
    """Consecutive cut ranges that each fit one request."""

    def fits(first: int, last: int) -> bool:
        base = cuts[first].output
        end = cuts[last - 1].output + cuts[last - 1].length
        within = (end - base) <= MAX_GROUP_SECONDS * RATE
        return last - first <= MAX_CLIPS and overlapping(parts, base, end) <= layers and within

    bounds: list[tuple[int, int]] = []
    first = 0
    while first < len(cuts):
        last = first + 1
        while last < len(cuts) and fits(first, last + 1):
            last += 1
        bounds.append((first, last))
        first = last
    return bounds


def original(media: Path) -> Path | None:
    """The first non-derived file of ``media``'s recorded derivation chain, None when invalid.

    A derived source (``DERIVATION_RECORDS``) records the file it came from beside itself; its
    rate says nothing of the content (the vertical crop is a fixed-rate re-encode).
    """
    current = media
    for _ in range(len(DERIVATION_RECORDS) + 1):
        found = [current.parent / name for name in DERIVATION_RECORDS]
        found = [path for path in found if path.is_file()]
        if not found:
            return current
        try:
            data = record(json.loads(found[0].read_text(encoding="utf-8")))
        except (OSError, ValueError, ReplayError):
            return None
        origin = data.get("source")
        if len(found) != 1 or data.get("output") != str(current) or not isinstance(origin, str):
            return None
        current = Path(origin)
    return None


def bit_rate(source: Path) -> int:
    """The average encode rate for ``source``'s original; an unreadable one keeps the maximum."""
    origin = original(source)
    if origin is None:
        return MAX_BIT_RATE
    try:
        probe = subprocess.run(
            ["ffprobe", "-v", "error", "-select_streams", "v:0",
             "-show_entries", "stream=bit_rate,width,height", "-of", "json", str(origin)],
            capture_output=True, text=True, check=True, timeout=PROBE_TIMEOUT,
        )  # fmt: skip
        streams = record(json.loads(probe.stdout))["streams"]
        if not isinstance(streams, list):
            return MAX_BIT_RATE
        stream = record(streams[0])
        pixels = int(str(stream["width"])) * int(str(stream["height"]))
        scaled = SOURCE_RATE_FACTOR * int(str(stream["bit_rate"])) * WIDTH * HEIGHT / pixels
    except (OSError, subprocess.SubprocessError, ValueError, KeyError, IndexError, ReplayError):
        return MAX_BIT_RATE
    return min(MAX_BIT_RATE, max(MIN_BIT_RATE, round(scaled / 100_000) * 100_000))


def video_end(source: Path) -> float | None:
    """Where ``source``'s video track ends; ``None`` when ffprobe cannot tell."""
    try:
        probe = subprocess.run(
            ["ffprobe", "-v", "error", "-select_streams", "v:0",
             "-show_entries", "stream=duration", "-of", "json", str(source)],
            capture_output=True, text=True, check=True, timeout=PROBE_TIMEOUT,
        )  # fmt: skip
        streams = record(json.loads(probe.stdout))["streams"]
        if not isinstance(streams, list):
            return None
        return float(str(record(streams[0])["duration"]))
    except (OSError, subprocess.SubprocessError, ValueError, KeyError, IndexError, ReplayError):
        return None


def end_within_source(recipe: dict[str, object], source_end: float) -> None:
    """End clips inside the source's video track, which media_edit refuses to read past.

    A cut through the source's last frame reads half a slot past its nominal end (``sampled``),
    and a variable-rate track can also end short of the grid (youtube-1GUiocLmiFw g049 asked for
    5874.283 s of 5874.266 s). media_edit renders ``round(duration * RATE)`` frames, so the clip
    keeps just enough duration for its planned frames and starts that much earlier; a shift of
    half a slot could read another frame, so it is refused. ``render_group`` counts the frames.
    """
    shortened = False
    for clip in recipe_items(recipe, "clips"):
        selection = record(clip["selection"])
        start = float(str(selection["startSeconds"]))
        duration = float(str(selection["durationSeconds"]))
        room = source_end - END_MARGIN_SECONDS - start
        if duration <= room:
            continue
        kept = max(room, (round(duration * RATE) - 0.5) / RATE + 2 * END_MARGIN_SECONDS)
        if kept - room >= 0.5 / RATE:
            raise ReplayError(f"The source's video track ends too early for its cut: {source_end}")
        selection["startSeconds"] = start - (kept - room)
        selection["durationSeconds"] = kept
        shortened = True
    if shortened:
        cut_layers(recipe)


def request(
    cuts: Sequence[Cut],
    parts: Sequence[Part],
    source: Path,
    directory: Path,
    name: str,
    *,
    rate: int = MAX_BIT_RATE,
    canvas: tuple[int, int] = (WIDTH, HEIGHT),
    badge: Path | None = None,
) -> tuple[dict[str, object], int]:
    """A media_edit request for consecutive cuts and the frames it renders; a ``badge`` movie
    (at least a group long) is the bottom layer from the group's start to its end."""
    base, end = cuts[0].output, cuts[-1].output + cuts[-1].length
    clips = [
        {
            "sourcePath": str(source),
            "selection": {
                "startSeconds": sampled(cut.start)
                if cut.phase == 0
                else float((cut.start + Fraction(1, 2) + cut.phase) / RATE),
                "durationSeconds": seconds(cut.length),
                "rate": 1,
            },
        }
        for cut in cuts
    ]
    layers: list[dict[str, object]] = []
    if badge is not None:
        layers.append(
            {
                "sourcePath": str(badge),
                "selection": {"startSeconds": 0.0, "durationSeconds": seconds(end - base),
                              "rate": 1},
                "offsetSeconds": 0.0,
                "opacity": 1,
            }
        )  # fmt: skip
    for part in parts:
        low, high = max(part.offset, base), min(part.end, end)
        if low < high:
            layers.append(
                {
                    "sourcePath": str(part.render),
                    "selection": {
                        # Alpha parts are exact constant-rate renders: frame starts are exact.
                        "startSeconds": seconds(low - part.offset),
                        "durationSeconds": seconds(high - low),
                        "rate": 1,
                    },
                    "offsetSeconds": seconds(low - base),
                    "opacity": 1,
                }
            )
    video = {
        "width": canvas[0],
        "height": canvas[1],
        "frameRate": RATE,
        "resizeMode": "fit",
        "encoding": {"codec": "hevc", "averageBitRate": rate, "allowFrameReordering": False},
    }
    body: dict[str, object] = {
        "outputDirectory": str(directory),
        "outputName": f"{name}.mp4",
        "recipe": {"video": video, "clips": clips, "additionalVideo": layers},
    }
    return body, end - base


def write_json(path: Path, value: object) -> None:
    """Write a new file or accept an identical one, so a re-plan is idempotent."""
    text = json.dumps(value, indent=1, ensure_ascii=False) + "\n"
    if path.exists():
        if path.read_text(encoding="utf-8") != text:
            raise ReplayError(f"{path} exists with a different plan")
        return
    with path.open("x", encoding="utf-8") as stream:
        os.fchmod(stream.fileno(), 0o600)
        stream.write(text)


def plan(
    cuts_path: Path,
    parts_path: Path,
    source: Path,
    output_root: Path,
    requests: Path,
    *,
    packets: Packets | None = None,
    states: Path | None = None,
    rate_source: Path | None = None,
    frame_clock: Path | None = None,
    canvas: tuple[int, int] = (WIDTH, HEIGHT),
    badge: Path | None = None,
) -> Path:
    """Write one request per group and ``groups.json``; return ``groups.json``.

    The encode rate derives from ``rate_source`` (the recording's original; the cleaned source
    is a fixed-rate re-encode), or from ``source`` without one. With ``frame_clock`` (the
    source's decoded clock) each cut reads its slots at the phase ``frame_slots`` finds, so
    millisecond-quantized timestamps neither drop nor repeat frames.
    """
    parts = load_parts(parts_path, packets or packet_count, states)
    layers = MAX_LAYERS if badge is None else MAX_LAYERS - 1
    cuts = [piece for cut in load_cuts(cuts_path) for piece in split_cut(cut, parts, layers)]
    if frame_clock is not None:
        cuts = phased(cuts, frame_clock, source, packets or packet_count)
    requests.mkdir(mode=0o700, parents=True, exist_ok=True)
    rate = bit_rate(rate_source or source)
    source_end = video_end(source)
    rows = []
    for index, (first, last) in enumerate(group_bounds(cuts, parts, layers)):
        name = f"g{index:03d}"
        directory = output_root / name
        body, frames = request(cuts[first:last], parts, source, directory, name, rate=rate,
                               canvas=canvas, badge=badge)  # fmt: skip
        if source_end is not None:
            end_within_source(record(body["recipe"]), source_end)
        write_json(requests / f"{name}.json", body)
        rows.append({"group": index, "name": name, "frames": frames, "directory": str(directory)})
    groups = requests / "groups.json"
    write_json(groups, rows)
    return groups


def group_output(directory: Path, name: str) -> list[Path]:
    return sorted(directory.glob(f"edit-*/{name}.mp4"))


def render_group(row: dict[str, object], requests: Path, call: Call, packets: Packets) -> str:
    """Reuse a finished group, otherwise clear its partial output and render it."""
    name, frames = str(row["name"]), int(str(row["frames"]))
    directory = Path(str(row["directory"]))
    outputs = group_output(directory, name)
    if len(outputs) == 1 and packets(outputs[0]) == frames:
        return "reused"
    if directory.exists():
        shutil.rmtree(directory)
    directory.mkdir(mode=0o700, parents=True)
    response = requests / f"{name}.response.json"
    response.unlink(missing_ok=True)
    try:
        call(aligned(requests, name), response)
    except (subprocess.CalledProcessError, ReplayError):
        # AVFoundation can refuse to read a composition whose clip ends on certain exact
        # boundaries ("Cannot start reading the composed video"; youtube-Eg9 g013 at 3520.05 s);
        # ending each clip TRIM_TICKS earlier reads, and the planned frame count is still checked.
        if response.exists():
            response.rename(requests / f"{name}.failed-response.json")
        shutil.rmtree(directory)
        directory.mkdir(mode=0o700, parents=True)
        call(trimmed(requests, name), response)
    outputs = group_output(directory, name)
    if len(outputs) != 1 or packets(outputs[0]) != frames:
        raise ReplayError(f"{name} did not render its planned {frames} frames")
    return "completed"


def trimmed(requests: Path, name: str) -> Path:
    """A retry request whose clips each end ``TRIM_TICKS`` of the native timescale earlier.

    Caption layers that reached the old timeline end are cut at the shorter one; media_edit
    refuses a layer that overruns the timeline.
    """
    body = record(json.loads((requests / f"{name}.json").read_text(encoding="utf-8")))
    recipe = record(body["recipe"])
    for clip in recipe_items(recipe, "clips"):
        selection = record(clip["selection"])
        selection["durationSeconds"] = float(str(selection["durationSeconds"])) - TRIM_TICKS / 60000
    cut_layers(recipe)
    align_end(recipe)
    path = requests / f"{name}.retry.json"
    path.unlink(missing_ok=True)
    write_json(path, body)
    return path


def aligned(requests: Path, name: str) -> Path:
    """The group's request, or an ``.aligned.json`` copy whose last clip ends where media_edit
    reads the whole composition."""
    original = requests / f"{name}.json"
    body = record(json.loads(original.read_text(encoding="utf-8")))
    if reads_to_its_end(record(body["recipe"])):
        return original
    align_end(record(body["recipe"]))
    path = requests / f"{name}.aligned.json"
    path.unlink(missing_ok=True)
    write_json(path, body)
    return path


def recipe_items(recipe: dict[str, object], key: str) -> list[dict[str, object]]:
    items = recipe.get(key, [])
    if not isinstance(items, list):
        raise ReplayError(f"The recipe's {key} is not a list")
    return [record(item) for item in items]


def seconds_of(item: dict[str, object], key: str) -> float:
    return float(str(record(item["selection"])[key]))


def clips_seconds(recipe: dict[str, object]) -> float:
    return sum(seconds_of(clip, "durationSeconds") for clip in recipe_items(recipe, "clips"))


def cut_layers(recipe: dict[str, object]) -> None:
    """Shorten caption layers that end past the clips' timeline to end on it."""
    total = clips_seconds(recipe)
    for layer in recipe_items(recipe, "additionalVideo"):
        selection = record(layer["selection"])
        end = float(str(layer["offsetSeconds"])) + float(str(selection["durationSeconds"]))
        if end > total:
            selection["durationSeconds"] = float(str(selection["durationSeconds"])) - (end - total)


def native_ticks(seconds: float) -> int:
    """media_edit's ``CMTime(seconds:)``: truncated, so 4.3666… s is 261999 ticks."""
    return math.floor(seconds * NATIVE_TICKS)


def planned_seconds(seconds: float) -> float:
    """media_edit's planned clip duration: rounded to whole ticks."""
    return math.floor(seconds * NATIVE_TICKS + 0.5) / NATIVE_TICKS


def reads_to_its_end(recipe: dict[str, object]) -> bool:
    """Whether media_edit's video composition and its tracks reach the timeline it reads.

    Truncated clip and layer ends can sum one tick short of the truncated timeline; the reader
    then refuses the composition (AVFoundation -11841, "Cannot start reading the composed
    video"; 10/28 g048).
    """
    cursor, spans = 0.0, []
    for clip in recipe_items(recipe, "clips"):
        spans.append((cursor, seconds_of(clip, "durationSeconds")))
        cursor += planned_seconds(seconds_of(clip, "durationSeconds"))
    for layer in recipe_items(recipe, "additionalVideo"):
        spans.append((float(str(layer["offsetSeconds"])), seconds_of(layer, "durationSeconds")))
    instructions = max(native_ticks(at) + native_ticks(planned_seconds(raw)) for at, raw in spans)
    tracks = max(native_ticks(at) + native_ticks(raw) for at, raw in spans)
    return min(instructions, tracks) >= native_ticks(cursor)


def align_end(recipe: dict[str, object]) -> None:
    """Move the last clip's end by the fewest ticks (well under a frame) for which media_edit
    reads its composition to the end; caption layers past a shorter timeline are cut there."""
    if reads_to_its_end(recipe):
        return
    selection = record(recipe_items(recipe, "clips")[-1]["selection"])
    ticks = round(float(str(selection["durationSeconds"])) * NATIVE_TICKS)
    for moved in (ticks + sign * step for step in range(1, ALIGN_TICKS + 1) for sign in (1, -1)):
        selection["durationSeconds"] = moved / NATIVE_TICKS
        cut_layers(recipe)
        if reads_to_its_end(recipe):
            return
    raise ReplayError("No clip end within the alignment bound reads the whole composition")


def media_edit_call(server: Path, server_sha256: str) -> Call:
    def call(arguments: Path, response: Path) -> None:
        subprocess.run(
            [sys.executable, "-B", "-m", "replay.apple_pro_apps_call", "media_edit",
             str(arguments), str(response), "--server", str(server),
             "--server-sha256", server_sha256, "--timeout", str(REQUEST_TIMEOUT)],
            check=True,
        )  # fmt: skip

    return call


def groups_of(requests: Path) -> list[dict[str, object]]:
    return [record(row) for row in json.loads((requests / "groups.json").read_text("utf-8"))]


def render(
    requests: Path, call: Call, *, workers: int, packets: Packets | None = None
) -> dict[str, str]:
    """Render every group, ``workers`` at a time, longest first; return each group's result."""
    rows = sorted(groups_of(requests), key=lambda row: -int(str(row["frames"])))
    count = packets or packet_count
    with ThreadPoolExecutor(max_workers=workers) as pool:
        results = pool.map(lambda row: render_group(row, requests, call, count), rows)
        return {str(row["name"]): result for row, result in zip(rows, results, strict=True)}


def voice(output: Path) -> Path:
    """Where the joined movie's encoded tempo audio lives."""
    return output.parent / "audio" / "tempo.m4a"


def measured(path: Path, *, ffmpeg: str, chain: str = "") -> tuple[float, float]:
    """Integrated loudness (LUFS) and true peak (dBTP) of an audio file, after the filter
    ``chain`` when one is given, from one ebur128 pass."""
    filters = f"{chain}," if chain else ""
    done = subprocess.run(
        [ffmpeg, "-nostdin", "-hide_banner", "-nostats", "-i", str(path),
         "-af", f"{filters}ebur128=peak=true:framelog=quiet", "-f", "null", "-"],
        capture_output=True, text=True, check=True, timeout=LOUDNESS_TIMEOUT,
    )  # fmt: skip
    integrated, peak = INTEGRATED.findall(done.stderr), TRUE_PEAK.findall(done.stderr)
    if not integrated or not peak or "-inf" in (integrated[-1], peak[-1]):
        raise ReplayError(f"No measurable voice loudness in {path}")
    return float(integrated[-1]), float(peak[-1])


def level_chain(gain: float, loudness: Loudness) -> str:
    """Float conversion, the static gain, then a lookahead peak limiter run at 4x the rate (so its
    sample peaks bound the true peak) a margin below the ceiling for the AAC encode; no other
    dynamics. The limiter's delay is compensated, so the voice stays on its clock."""
    limit = 10 ** ((loudness.ceiling - LIMITER_MARGIN_DB) / 20)
    return (
        f"aformat=sample_fmts=flt,volume={gain:.2f}dB,aresample={OVERSAMPLED_RATE},"
        f"alimiter=limit={limit:.6f}:attack={LIMITER_ATTACK_MS}:release={LIMITER_RELEASE_MS}"
        f":level=disabled:latency=enabled,aresample={AUDIO_RATE}"
    )


def encode_audio(
    tempo: Path,
    audio: Path,
    *,
    ffmpeg: str,
    loudness: Loudness | None = None,
    reference: Path | None = None,
) -> Path:
    """Encode the tempo audio once; an existing encode is reused.

    With a ``loudness`` policy the voice is measured once and given the static gain toward the
    target, then limited at the ceiling (``level_chain``). Limiting lowers the integrated level
    of a voice with loud bursts, so the gain is corrected once by the shortfall measured through
    the chain. Both measurements and the gains are written beside the encode
    (``<audio>.loudness.json``). With a ``reference`` (the voice of a mix that keeps its music)
    the gain is measured on the reference alone and applied to ``tempo`` without the correction;
    the mix's own levels before and through the chain are recorded beside it.
    """
    if audio.exists():
        return audio
    audio.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    level: list[str] = []
    if loudness is not None and reference is not None:
        integrated, peak = measured(reference, ffmpeg=ffmpeg)
        gain = round(loudness.target - integrated, 2)
        level = ["-af", level_chain(gain, loudness)]
        mix, mix_peak = measured(tempo, ffmpeg=ffmpeg)
        limited, limited_peak = measured(tempo, ffmpeg=ffmpeg, chain=level[1])
        write_json(audio.with_name(f"{audio.name}.loudness.json"), {
            "reference": str(reference), "integratedLufs": integrated, "truePeakDbtp": peak,
            "gainDb": gain, "mixLufs": mix, "mixTruePeakDbtp": mix_peak,
            "limitedLufs": limited, "limitedTruePeakDbtp": limited_peak,
            "targetLufs": loudness.target, "ceilingDbtp": loudness.ceiling, "limiter": level[1],
        })  # fmt: skip
    elif loudness is not None:
        integrated, peak = measured(tempo, ffmpeg=ffmpeg)
        first = round(loudness.target - integrated, 2)
        limited, _ = measured(tempo, ffmpeg=ffmpeg, chain=level_chain(first, loudness))
        gain = round(first + min(loudness.target - limited, MAXIMUM_CORRECTION_DB), 2)
        level = ["-af", level_chain(gain, loudness)]
        write_json(audio.with_name(f"{audio.name}.loudness.json"), {
            "integratedLufs": integrated, "truePeakDbtp": peak, "firstGainDb": first,
            "limitedLufs": limited, "gainDb": gain, "targetLufs": loudness.target,
            "ceilingDbtp": loudness.ceiling, "limiter": level[1],
        })  # fmt: skip
    staged = audio.with_name(".tempo.partial.m4a")
    staged.unlink(missing_ok=True)
    subprocess.run(
        [ffmpeg, "-nostdin", "-v", "error", "-i", str(tempo), *level, *AUDIO_ARGUMENTS,
         str(staged)],
        check=True,
    )  # fmt: skip
    staged.rename(audio)
    return audio


def join(parts: Sequence[PartFile], *, output: Path, audio: Path, ffmpeg: str) -> int:
    """Stream-copy the verified parts and the audio into a new ``output`` with a front index."""
    if output.exists():
        raise ReplayError("Refusing to overwrite the joined output")
    stream = verify_parts(parts)  # each part holds exactly its planned frames
    expected = sum(part.frames for part in parts)
    with TemporaryDirectory(dir=output.parent, prefix=".join-") as work:
        script = Path(work) / "parts.ffconcat"
        script.write_text(concat_list(parts, RATE, stream.time_base), encoding="utf-8")
        staged = Path(work) / output.name
        subprocess.run(
            [ffmpeg, "-nostdin", "-v", "error", "-n", "-f", "concat", "-safe", "0",
             "-i", str(script), "-i", str(audio), "-map", "0:v:0", "-map", "1:a:0", "-c", "copy",
             "-video_track_timescale", str(stream.time_base.denominator),
             *quicktime_video_tags(parts[0].path),
             "-moov_size", str(MOOV_MARGIN + MOOV_BYTES_PER_FRAME * expected),
             "-write_btrt", "0", str(staged)],
            check=True,
        )  # fmt: skip
        if probe_video(staged).packets != expected:
            raise ReplayError("The joined output does not hold the planned frames")
        os.link(staged, output)
    return expected


def finish(
    requests: Path, tempo: Path, output: Path, *, ffmpeg: str, packets: Packets | None = None,
    loudness: Loudness | None = None, reference: Path | None = None,
) -> int:  # fmt: skip
    """Encode the tempo audio once and join every group with it; reuse a finished join."""
    rows = sorted(groups_of(requests), key=lambda row: int(str(row["group"])))
    expected = sum(int(str(row["frames"])) for row in rows)
    count = packets or packet_count
    if output.exists() and count(output) == expected:
        return expected
    output.unlink(missing_ok=True)
    audio = encode_audio(tempo, voice(output), ffmpeg=ffmpeg, loudness=loudness,
                         reference=reference)  # fmt: skip
    files = []
    for row in rows:
        name = str(row["name"])
        found = group_output(Path(str(row["directory"])), name)
        if len(found) != 1:
            raise ReplayError(f"{name} is missing")
        files.append(PartFile(found[0], int(str(row["frames"]))))
    return join(files, output=output, audio=audio, ffmpeg=ffmpeg)


def level_arguments(command: argparse.ArgumentParser) -> None:
    command.add_argument("--loudness-target", type=float, help="voice target, integrated LUFS")
    command.add_argument("--true-peak-ceiling", type=float, help="voice true-peak ceiling, dBTP")
    command.add_argument(
        "--loudness-reference", type=Path, help="the voice setting the gain of a kept-music mix"
    )


def level_policy(parser: argparse.ArgumentParser, args: argparse.Namespace) -> Loudness | None:
    """The voice level policy of ``--loudness-target`` with ``--true-peak-ceiling``, if given."""
    if (args.loudness_target is None) != (args.true_peak_ceiling is None):
        parser.error("--loudness-target and --true-peak-ceiling go together")
    if args.loudness_target is None:
        return None
    return Loudness(target=args.loudness_target, ceiling=args.true_peak_ceiling)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    planned = commands.add_parser("plan")
    for name in ("cuts", "parts", "source", "output_root", "requests"):
        planned.add_argument(name, type=Path)
    planned.add_argument("--states", type=Path, help="Compressor job states of the parts")
    planned.add_argument("--rate-source", type=Path, help="the original the encode rate follows")
    planned.add_argument("--frame-clock", type=Path, help="the source's decoded video clock")
    planned.add_argument(
        "--canvas", type=int, nargs=2, default=(WIDTH, HEIGHT), metavar=("WIDTH", "HEIGHT")
    )
    planned.add_argument("--badge", type=Path, help="a nameplate movie over every group")
    rendered = commands.add_parser("render")
    rendered.add_argument("requests", type=Path)
    rendered.add_argument("--server", type=Path, required=True)
    rendered.add_argument("--server-sha256", required=True)
    rendered.add_argument("--workers", type=int, default=3)
    rendered.add_argument("--tempo", type=Path, help="tempo audio to encode beside the render")
    rendered.add_argument("--audio", type=Path, help="where the encoded tempo audio goes")
    rendered.add_argument("--ffmpeg", default="ffmpeg")
    level_arguments(rendered)
    finished = commands.add_parser("finish")
    finished.add_argument("requests", type=Path)
    finished.add_argument("tempo", type=Path)
    finished.add_argument("output", type=Path)
    finished.add_argument("--ffmpeg", default="ffmpeg")
    level_arguments(finished)
    args = parser.parse_args(argv)
    if args.command == "plan":
        arguments = (args.cuts, args.parts, args.source, args.output_root, args.requests)
        print(
            plan(
                *arguments,
                states=args.states,
                rate_source=args.rate_source,
                frame_clock=args.frame_clock,
                canvas=(args.canvas[0], args.canvas[1]),
                badge=args.badge,
            )
        )
    elif args.command == "render":
        call = media_edit_call(args.server, args.server_sha256)
        if (args.tempo is None) != (args.audio is None):
            parser.error("--tempo and --audio go together")
        with ThreadPoolExecutor(max_workers=1) as pool:
            # The audio encode (one CPU, ~2 min for a 2 h recording) needs only the tempo; beside
            # the encoder-bound render it leaves finish only the join.
            encoded = None
            if args.tempo is not None:
                encoded = pool.submit(encode_audio, args.tempo, args.audio, ffmpeg=args.ffmpeg,
                                      loudness=level_policy(parser, args),
                                      reference=args.loudness_reference)  # fmt: skip
            print(json.dumps(render(args.requests, call, workers=args.workers)))
            if encoded is not None:
                encoded.result()
    else:
        print(finish(args.requests, args.tempo, args.output, ffmpeg=args.ffmpeg,
                     loudness=level_policy(parser, args),
                     reference=args.loudness_reference))  # fmt: skip
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
