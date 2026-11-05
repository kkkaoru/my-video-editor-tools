"""A delivery's face track: the recording's YOLO face samples on the delivery clock.

``<movie stem>.faces.json`` beside the delivery movie lets a player place captions above the
speaker's face (``replay.caption_layout_file``, ``docs/caption-plan.md``). It is built only from
pinned pipeline evidence:

- the YOLO track the ``yolo-track`` step pinned (``yolo-track-v2.json``: every second decoded
  source frame, its highest-confidence face box and the number of faces above the floor; a v3
  track also every face box, which makes a version-2 face track: every face of each sample);
- the source frame clock (``video-clock.json``): each decoded frame's first constant-rate slot,
  by ``replay.frame_slots.first_slots``, exactly as the compose reads the source;
- the cut plan the ``cut-plan`` step pinned: a slot inside a kept cut lands on delivery frame
  ``output_start_frame + slot - source_start_frame``; samples in removed footage are dropped.

Boxes are placed from the tracked source's pixels on the delivery canvas exactly as composition
fits and centres the source (``resizeMode: fit``); no sample is thinned, moved or invented.
"""

import json
from fractions import Fraction
from pathlib import Path

from replay import caption_plan, frame_slots, yolo_track
from replay.caption_plan import integer, read_json, real, rows, sha256
from replay.core import ReplayError
from replay.delivery_media import read_clock
from replay.recordings import record

VERSION = 1
SUFFIX = ".faces.json"
FACES_KEYS = frozenset({"version", "clock", "rate", "width", "height", "source", "samples"})
SOURCE_KEYS = frozenset({"model", "modelSha256", "file", "sha256"})
SAMPLE_KEYS = frozenset({"frame", "faceCount", "box"})
ALL_FACES_VERSION = 2  # every face of a sample (``docs/caption-plan.md`` "Multiple speakers")
ALL_FACES_SAMPLE_KEYS = frozenset({"frame", "boxes"})
BOX_KEYS = frozenset({"x", "y", "w", "h", "score"})
PIXEL_DIGITS, SCORE_DIGITS = 2, 4


def pinned(root: Path, step: str, key: str) -> Path:
    """An output a step receipt pinned, refused when its content changed."""
    pin = record(record(read_json(root / "steps" / f"{step}.json")["outputs"])[key])
    path = Path(str(pin["path"]))
    if sha256(path) != pin["sha256"]:
        raise ReplayError(f"{path} differs from the {step} receipt's pin")
    return path


def delivery_frames(cuts: list[dict[str, object]]) -> tuple[dict[int, int], int]:
    """Source slot -> delivery frame over the kept cuts, and the delivery's frame count."""
    frames: dict[int, int] = {}
    end = 0
    for cut in cuts:
        start = integer(cut["source_start_frame"], "source_start_frame")
        stop = integer(cut["source_end_frame"], "source_end_frame")
        output = integer(cut["output_start_frame"], "output_start_frame")
        if output != end or stop <= start:
            raise ReplayError("The cut plan's output frames are not contiguous")
        frames.update((slot, output + slot - start) for slot in range(start, stop))
        end = output + stop - start
    return frames, end


def box(value: object) -> dict[str, object] | None:
    if value is None:
        return None
    data = record(value)
    if set(data) != BOX_KEYS:
        raise ReplayError(f"A face box has exactly {', '.join(sorted(BOX_KEYS))}")
    for key in BOX_KEYS:
        real(data[key], key)
    if real(data["w"], "w") <= 0 or real(data["h"], "h") <= 0:
        raise ReplayError("A face box needs a positive size")
    return data


def validate(value: object) -> dict[str, object]:
    """A version-1 (the best face and the face count) or version-2 (every face) face track,
    its samples strictly ordered by delivery frame."""
    faces = record(value)
    version = faces.get("version")
    if (
        set(faces) != FACES_KEYS
        or version not in (VERSION, ALL_FACES_VERSION)
        or faces["clock"] != "delivery"
    ):
        raise ReplayError(
            f"A face track has exactly {', '.join(sorted(FACES_KEYS))}, version 1 or 2"
        )
    for key in ("rate", "width", "height"):
        if integer(faces[key], key) <= 0:
            raise ReplayError(f"The face track's {key} must be positive")
    if set(record(faces["source"])) != SOURCE_KEYS:
        raise ReplayError(f"A face track source has exactly {', '.join(sorted(SOURCE_KEYS))}")
    previous = -1
    for sample in rows(faces["samples"], "samples"):
        if version == ALL_FACES_VERSION:
            if set(sample) != ALL_FACES_SAMPLE_KEYS:
                raise ReplayError("A version-2 face sample has exactly boxes and frame")
            if integer(sample["frame"], "frame") <= previous:
                raise ReplayError(f"Face sample at frame {sample['frame']} is out of order")
            for found in rows(sample["boxes"], "boxes"):
                box(found)
            previous = int(str(sample["frame"]))
            continue
        if set(sample) != SAMPLE_KEYS:
            raise ReplayError(f"A face sample has exactly {', '.join(sorted(SAMPLE_KEYS))}")
        frame, count = integer(sample["frame"], "frame"), integer(sample["faceCount"], "faceCount")
        if frame <= previous or count < 0 or (box(sample["box"]) is None) != (count == 0):
            raise ReplayError(f"Face sample at frame {frame} is out of order or inconsistent")
        previous = frame
    return faces


def build(work: Path, movie: Path) -> dict[str, object]:
    """The validated face track of ``movie`` from the recording's pinned evidence."""
    root = work.parent
    caption_plan.recorded_movie(root, movie)
    track_path = pinned(root, "yolo-track", "track")
    track = read_json(track_path)
    if track.get("protocol") not in yolo_track.PROTOCOLS or track.get("provenance") != "yolo":
        raise ReplayError("Not a timestamped (v2) YOLO face track")
    source = (work / "source.sha256").read_text(encoding="utf-8").split()[0]
    if record(track["source"])["sha256"] != source:
        raise ReplayError("The YOLO track is of another source")
    clock = read_clock(work / "video-clock.json")
    if Fraction(str(track["timeBase"])) != clock.time_base:
        raise ReplayError("The YOLO track and the frame clock use other time bases")
    cuts = record(json.loads(pinned(root, "cut-plan", "cuts").read_text(encoding="utf-8")))
    slot_frames, total = delivery_frames(rows(cuts["cuts"], "cuts"))
    ffprobe = str(read_json(root / "pipeline.json")["ffprobe"])
    width, height, rate, frames = caption_plan.probed(movie, ffprobe)
    if total != frames or integer(cuts["fps"], "fps") != rate:
        raise ReplayError(f"{movie} is not the cut plan's length and rate")
    tracked = integer(track["width"], "width"), integer(track["height"], "height")
    # An even crop width (608 for 9:16 of 1080, ``source_vertical``) may miss the canvas's shape
    # by up to one source pixel; anything more is another shape.
    if abs(tracked[0] * height - width * tracked[1]) > height:
        raise ReplayError("The YOLO track's frame is not the delivery canvas's shape")
    scale = min(width / tracked[0], height / tracked[1])
    origin = (width - tracked[0] * scale) / 2, (height - tracked[1] * scale) / 2
    slots = frame_slots.first_slots(clock.pts, clock.time_base, rate)
    every = track["protocol"] == yolo_track.PROTOCOL  # a v3 track keeps every face
    samples: list[dict[str, object]] = []
    for row in rows(track["frames"], "frames"):
        index = integer(row["frame"], "frame")
        if not 0 <= index < len(clock.pts) or clock.pts[index] != row["pts"]:
            raise ReplayError(f"YOLO frame {index} is not the frame clock's")
        slot = slots[index]
        frame = None if slot is None else slot_frames.get(slot)
        if frame is None:
            continue  # never shown, or cut from the delivery
        if every:
            boxes = [scaled(face, scale, origin) for face in rows_of_faces(row["faces"])]
            samples.append({"frame": frame, "boxes": boxes})
            continue
        face = row["face"]
        samples.append({"frame": frame, "faceCount": integer(row["faceCount"], "faceCount"),
                        "box": None if face is None else scaled(face, scale, origin)})  # fmt: skip
    weights = record(track["weights"])
    return validate({
        "version": ALL_FACES_VERSION if every else VERSION, "clock": "delivery", "rate": rate,
        "width": width, "height": height,
        "source": {"model": Path(str(weights["path"])).name, "modelSha256": weights["sha256"],
                   "file": track_path.name, "sha256": sha256(track_path)},
        "samples": samples,
    })  # fmt: skip


def scaled(face: object, scale: float, origin: tuple[float, float]) -> dict[str, object]:
    """A YOLO face ``[x1, y1, x2, y2, score]`` as a canvas box."""
    values = [real(item, "face") for item in rows_of(face)]
    return {
        "x": round(origin[0] + values[0] * scale, PIXEL_DIGITS),
        "y": round(origin[1] + values[1] * scale, PIXEL_DIGITS),
        "w": round((values[2] - values[0]) * scale, PIXEL_DIGITS),
        "h": round((values[3] - values[1]) * scale, PIXEL_DIGITS),
        "score": round(values[4], SCORE_DIGITS),
    }  # fmt: skip


def rows_of_faces(value: object) -> list[object]:
    if not isinstance(value, list):
        raise ReplayError("A v3 YOLO row's faces must be a list")
    return value


def rows_of(value: object) -> list[object]:
    if not isinstance(value, list) or len(value) != len(BOX_KEYS):
        raise ReplayError("A YOLO face is [x1, y1, x2, y2, score]")
    return value


def path(movie: Path) -> Path:
    return movie.with_name(f"{movie.stem}{SUFFIX}")


def write(faces: dict[str, object], movie: Path) -> Path:
    """Write the face track beside its movie once (an identical one is kept)."""
    validate(faces)
    target = path(movie)
    text = json.dumps(faces, ensure_ascii=False, separators=(",", ":"), allow_nan=False) + "\n"
    caption_plan.publish_once(target, text.encode("utf-8"))
    return target


def read(movie: Path) -> dict[str, object] | None:
    target = path(movie)
    return validate(json.loads(target.read_text(encoding="utf-8"))) if target.is_file() else None
