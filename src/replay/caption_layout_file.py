"""A delivery's caption position: where live-drawn captions go, as an editable intent.

``<movie stem>.caption-layout.json`` beside the delivery movie names a kind of position (the
plan's own, above the face, bottom, top or centre) plus a pixel offset; the user, an agent
(``python -m replay.caption_position``) or a viewer edits it, and the web viewer, the Mac
TranscriptViewer and caption-burn resolve it with the same rule (``resolve``, specified in
``docs/caption-plan.md``). Without the file every title stays where the plan puts it.

The title box is computed from the style font's em metrics, as ``caption_fixed_y_job``'s
envelope: its top is the 24 px padding plus Noto Sans JP's 1.16 em ascender above the first
baseline, its height one line's native measurement (150 px at the calibrated 82.9558 pt) plus
1.448 em (the line advance) a further line, so every implementation gets it without measuring
glyphs. The defaults reproduce the burned
placement: above the face by the 60 px clearance, inside a 120 px margin, with one face
(``faceCount == 1``) scoring at least 0.5, held across an unobserved title only from both sides
within 150 px (``replay.caption_fixed_y.person_top_with_hold``).
"""

import json
import math
from fractions import Fraction
from pathlib import Path

from replay import caption_faces, caption_plan
from replay.caption_fixed_y import UnknownPerson, person_top_with_hold
from replay.caption_plan import read_json, real, rows
from replay.core import ReplayError, write_private_text
from replay.fcp_tracking_caption import TrackingPosition
from replay.recordings import record

VERSION = 1
SUFFIX = ".caption-layout.json"
MODES = ("plan", "aboveFace", "bottom", "top", "center")
DEFAULTS: dict[str, float] = {
    "offsetX": 0.0, "offsetY": 0.0, "margin": 120.0, "faceGap": 60.0, "minScore": 0.5,
    "maxDrift": 150.0,
}  # fmt: skip
NON_NEGATIVE = frozenset({"margin", "faceGap", "maxDrift"})
EM_ASCENT, EM_ADVANCE = 1.16, 1.448
# One line's native measurement at the style-20 calibration: 150 px at 82.95580110497238 pt.
EM_LINE_HEIGHT = 150 / 82.95580110497238
PADDING = 24.0
SPEAKERS_VERSION = 2  # per-speaker options (``docs/caption-plan.md`` "Multiple speakers")
STACK_GAP = 12.0  # between stacked unassigned titles, as ``speaker_subtitles._LaneStack``
ICON_HEIGHT, ICON_GAP, ICON_RING = 0.08, 0.02, 4 / 150  # speaker icons, of the canvas height


def options(value: dict[str, object]) -> None:
    """A layout's (or one speaker's) mode and numbers, each in range when given."""
    if "mode" in value and value["mode"] not in MODES:
        raise ReplayError(f"A caption layout mode is one of {', '.join(MODES)}")
    for key in set(value) & set(DEFAULTS):
        number = real(value[key], key)
        if key in NON_NEGATIVE and number < 0 or key == "minScore" and not 0 <= number <= 1:
            raise ReplayError(f"Caption layout {key} is out of range")


def validate(value: object) -> dict[str, object]:
    """A version-1 layout (a mode and optional numbers; absent ones take ``DEFAULTS``), or a
    version-2 one that may also give a speaker its own options (``speakers``)."""
    layout = record(value)
    version = layout.get("version")
    keys = {"version", "mode", *DEFAULTS} | ({"speakers"} if version == SPEAKERS_VERSION else set())
    if not {"version", "mode"} <= set(layout) <= keys:
        raise ReplayError(f"A caption layout has version, mode and {', '.join(DEFAULTS)}")
    if version not in (VERSION, SPEAKERS_VERSION):
        raise ReplayError("A caption layout is version 1 or 2")
    options(layout)
    for speaker, given in record(layout.get("speakers", {})).items():
        entry = record(given)
        if not set(entry) <= {"mode", *DEFAULTS}:
            raise ReplayError(
                f"Caption layout speaker {speaker!r} has mode and {', '.join(DEFAULTS)}"
            )
        options(entry)
    return layout


def path(movie: Path) -> Path:
    return movie.with_name(f"{movie.stem}{SUFFIX}")


def read(movie: Path) -> dict[str, object]:
    """The movie's layout, or the plan's own position when it has none."""
    target = path(movie)
    if not target.is_file():
        return {"version": VERSION, "mode": "plan"}
    return validate(json.loads(target.read_text(encoding="utf-8")))


def write(layout: dict[str, object], movie: Path) -> Path:
    """Replace the movie's layout atomically."""
    target = path(movie)
    write_private_text(target, json.dumps(validate(layout), indent=1) + "\n")
    return target


def settings(layout: dict[str, object]) -> dict[str, float]:
    return {key: real(layout.get(key, default), key) for key, default in DEFAULTS.items()}


def bands(plan: dict[str, object]) -> dict[str, tuple[float, float]]:
    """Each speaker's horizontal band: split at the midpoints between neighbouring lane
    centres (a midpoint belongs to the right band), the outer bands open-ended."""
    width = float(str(plan["width"]))
    lanes = sorted(
        (width / 2 + real(item["laneX"], "laneX"), str(item["id"]))
        for item in rows(plan.get("speakers", []), "speakers")
    )
    edges = [(left[0] + right[0]) / 2 for left, right in zip(lanes, lanes[1:], strict=False)]
    lows, highs = [-math.inf, *edges], [*edges, math.inf]
    return {lane[1]: (low, high) for lane, low, high in zip(lanes, lows, highs, strict=False)}


def face_tops(
    faces: dict[str, object] | None, min_score: float, band: tuple[float, float] | None = None
) -> list[TrackingPosition]:
    """The face-top observations a title may use, its time the delivery frame.

    Version-1 faces: one face scoring at least ``min_score``. Version-2 faces: with a speaker's
    ``band``, exactly one face scoring that much centred in the band; without, exactly one face
    in the sample, scoring that much."""
    if faces is None:
        return []
    tops = []
    for sample in rows(faces["samples"], "samples"):
        if faces["version"] == caption_faces.VERSION:
            found = sample["box"]
            single = [found] if sample["faceCount"] == 1 and isinstance(found, dict) else []
        else:
            boxes = rows(sample["boxes"], "boxes")
            single = boxes if band is None and len(boxes) == 1 else []
            if band is not None:
                single = [
                    item for item in boxes
                    if real(item["score"], "score") >= min_score
                    and band[0] <= real(item["x"], "x") + real(item["w"], "w") / 2 < band[1]
                ]  # fmt: skip
        if len(single) == 1 and real(single[0]["score"], "score") >= min_score:
            frame = Fraction(int(str(sample["frame"])))
            found = record(single[0])
            tops.append(TrackingPosition(frame, real(found["x"], "x"), real(found["y"], "y")))
    return tops


def stacked(
    titles: list[dict[str, object]], placed: list[float], boxes: list[float]
) -> list[float]:
    """Unassigned titles of a version-2 plan stacked upwards where they overlap in time
    (``speaker_subtitles._LaneStack``): each takes the lowest bottom, from its own and just
    above each active one, that clears every active title by ``STACK_GAP``; no clamp after."""
    result = list(placed)
    active: list[tuple[float, float, int]] = []  # top, bottom, endFrame
    for index, title in enumerate(titles):
        if title.get("speaker") is not None:
            continue
        start, height = int(str(title["startFrame"])), boxes[index]
        live = [item for item in active if item[2] > start]
        candidates = sorted({placed[index] + height, *(item[0] - STACK_GAP for item in live)},
                            reverse=True)  # fmt: skip
        bottom = next(
            (
                candidate for candidate in candidates
                if all(candidate <= item[0] - STACK_GAP
                       or candidate - height >= item[1] + STACK_GAP for item in live)
            ),
            candidates[-1],
        )  # fmt: skip
        result[index] = bottom - height
        active.append((result[index], bottom, int(str(title["endFrame"]))))
    return result


def resolve(
    plan: dict[str, object], layout: dict[str, object], faces: dict[str, object] | None
) -> list[dict[str, object]]:
    """Every title's resolved Motion position and the basis its vertical position came from."""
    caption_plan.validate(plan)
    validate(layout)
    own = record(layout.get("speakers", {}))
    lanes = bands(plan)
    if not set(own) <= set(lanes):
        raise ReplayError("The caption layout names a speaker the plan does not have")
    if faces is not None:
        caption_faces.validate(faces)
        if (faces["width"], faces["height"]) != (plan["width"], plan["height"]):
            raise ReplayError("The face track is not on the plan's canvas")
    width, height = float(str(plan["width"])), float(str(plan["height"]))
    common = record(plan["style"])
    observed: dict[tuple[object, float], tuple[list[TrackingPosition], list[Fraction]]] = {}
    titles = rows(plan["titles"], "titles")
    placed: list[float] = []
    boxes: list[float] = []
    resolved: list[dict[str, object]] = []
    for title in titles:
        speaker = title.get("speaker") if lanes else None
        given = {**layout, **record(own.get(str(speaker), {}))}
        mode, values = given["mode"], settings(given)
        size = real(record(title.get("style", common))["fontSize"], "fontSize")
        lines = str(title["text"]).count("\n") + 1
        baseline = height / 2 - real(title["y"], "y")
        top = baseline - PADDING - EM_ASCENT * size
        box = EM_LINE_HEIGHT * size + (lines - 1) * EM_ADVANCE * size
        margin, basis = values["margin"], "mode"
        if mode == "plan":
            basis, place = "plan", top
        elif mode == "top":
            place = margin
        elif mode == "bottom":
            place = height - margin - box
        elif mode == "center":
            place = (height - box) / 2
        else:
            band = (
                lanes[str(speaker)]
                if speaker is not None
                and faces is not None
                and faces["version"] != caption_faces.VERSION
                else None
            )
            key = (band, values["minScore"])
            if key not in observed:
                tops = face_tops(faces, values["minScore"], band)
                observed[key] = (tops, [item.time for item in tops])
            try:
                face = person_top_with_hold(
                    observed[key][0], Fraction(int(str(title["startFrame"]))),
                    Fraction(int(str(title["endFrame"]))),
                    maximum_drift=values["maxDrift"], times=observed[key][1],
                )  # fmt: skip
                basis = "held" if face.held else "face"
                place = max(margin, min(height - margin, face.top - values["faceGap"]) - box)
            except UnknownPerson:
                basis, place = "plan", top
        placed.append(max(0.0, min(place + values["offsetY"], height - box)))
        boxes.append(box)
        x = max(-width / 2, min(real(title["x"], "x") + values["offsetX"], width / 2))
        resolved.append({"x": x, "basis": basis, "size": size})
    final = stacked(titles, placed, boxes) if lanes else placed
    return [
        {"x": item["x"], "basis": item["basis"],
         "y": height / 2 - (bottom + PADDING + EM_ASCENT * float(str(item["size"])))}
        for item, bottom in zip(resolved, final, strict=True)
    ]  # fmt: skip


def speaker_icons(plan: dict[str, object]) -> list[dict[str, object]]:
    """The squares of a version-2 plan's speaker icons (canvas pixels, top-left origin)."""
    width, height = int(str(plan["width"])), int(str(plan["height"]))
    size = math.floor(ICON_HEIGHT * height + 0.5)
    ring = max(2, math.floor(size * ICON_RING + 0.5))
    y = height - math.floor(ICON_GAP * height + 0.5) - size
    return [
        {"id": item["id"],
         "x": max(0, min(math.floor(width / 2 + real(item["laneX"], "laneX") - size / 2 + 0.5),
                         width - size)),
         "y": y, "size": size, "ring": ring}
        for item in rows(caption_plan.validate(plan).get("speakers", []), "speakers")
        if "icon" in item
    ]  # fmt: skip


def resolve_movie(movie: Path) -> list[dict[str, object]]:
    """The titles of the plan beside ``movie`` under its layout and face track."""
    plan = read_json(movie.with_name(f"{movie.stem}{caption_plan.SUFFIX}"))
    return resolve(plan, read(movie), caption_faces.read(movie))
