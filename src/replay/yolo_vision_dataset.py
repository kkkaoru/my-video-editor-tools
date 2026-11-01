"""Vision-labelled YOLO frames and a caption-line evaluation, by the cleanup's own caption rule.

A recording is named by its cleanup work directory: each chunk configuration gives the source,
the exact PTS clock, the native OCR tool and the recording's caption profile. A chunk that
escalated already holds every-frame Vision OCR (``attempt-full``); other frames are read by the
same native session (Vision accurate, Japanese and English, full frame). A Vision line is a
caption when ``profiled_detections`` (the cleanup's band, label and interface rules) accepts it;
every other line is interface text, kept beside the labels as the negative of the false-box
check. Face boxes come from the pinned detector (pseudo labels above ``FACE_SURE``).

A frame is left out when its labels are uncertain: a face of middling confidence, unreadable
text inside the caption bands, a caption line holding a fragment of an interface line of about
its size in the same frame that reads like the profile's interface vocabulary (Vision merged
moving interface text, such as a falling gift name, into the caption line), or a detector
caption box touching no Vision line (Vision may have missed a caption there). Nothing here is
specific to a recording; the manifest is the only per-recording input.
"""

import argparse
import json
import os
import sys
from collections.abc import Callable, Iterator, Mapping, Sequence
from dataclasses import dataclass
from difflib import SequenceMatcher
from math import ceil, floor
from pathlib import Path
from typing import Any

import cv2
import numpy as np
from numpy.typing import NDArray

from replay.caption_io import FrameObservation, native_ocr_frames
from replay.caption_masks import TextDetection
from replay.core import ReplayError
from replay.mps_graph_cache import released_mps_graphs
from replay.subtitle_cleanup_plan import CaptionSearchProfile
from replay.subtitle_clock import VideoFrameClock
from replay.subtitle_profile import COPY_HEIGHT_RATIO, _band, _eligible, profiled_detections
from replay.subtitle_sampled_chunk import ocr_band, session_ocr
from replay.subtitle_sampled_job import _clock, _profile
from replay.subtitle_yolo_regions import MASK_PAD, chunk_frames, extend_to_glyphs
from replay.yolo_joint_dataset import merge_row
from replay.yolo_joint_eval import Box, Prediction, iou, predictions_from
from replay.yolo_labels import CAPTION, CLASS_NAMES, FACE, PixelBox, yolo_row
from replay.yolo_track import TRAIN_WIDTH
from replay.yolo_train import check_checkpoint, check_settings

ROLES = ("train", "val", "test")
CONFIDENCE = 0.25  # the production detector floor
FACE_SURE = 0.5  # a face at or above this is a label; between the floor and this, the frame drops
CAPTION_SURE = 0.5  # a detector caption box this sure must touch some Vision line
COVERED = 0.5  # share of a caption line's area inside predicted boxes that counts it found
BATCH = 32
FRAGMENT_MIN = 3  # characters an interface-vocabulary fragment needs inside a caption line

Image = NDArray[np.uint8]
Detect = Callable[[list[Image]], list[Prediction]]


@dataclass(frozen=True, slots=True)
class FrameLabels:
    """Canvas-pixel corners of one frame's caption lines and interface lines."""

    captions: tuple[Box, ...]
    interface: tuple[Box, ...]
    unreadable: bool
    interface_fragment: bool = False


@dataclass(frozen=True, slots=True)
class Recording:
    name: str
    work: Path
    role: str
    chunks: tuple[int, ...]
    frame_step: int


def corners(line: TextDetection) -> Box:
    box = line.rectangle
    return (box.x, box.y, box.x + box.width, box.y + box.height)


def holds_fragment(text: str, vocabulary: Sequence[str]) -> bool:
    """The text shares a run of at least half of some vocabulary entry (and ``FRAGMENT_MIN``)."""
    for entry in vocabulary:
        match = SequenceMatcher(None, entry, text, autojunk=False).find_longest_match()
        if match.size >= max(FRAGMENT_MIN, ceil(len(entry) / 2)):
            return True
    return False


def merged(line: TextDetection, copies: Sequence[TextDetection]) -> bool:
    """The line holds a fragment of an interface copy of about its height."""
    height = line.rectangle.height
    return holds_fragment(
        line.text,
        [
            copy.text
            for copy in copies
            if max(height, copy.rectangle.height)
            <= COPY_HEIGHT_RATIO * min(height, copy.rectangle.height)
        ],
    )


def frame_labels(frame: FrameObservation, profile: CaptionSearchProfile) -> FrameLabels:
    """Caption lines by the cleanup's profile rule (words of one row joined), the rest interface.

    ``unreadable`` marks text inside the caption bands that the profile could not judge (too
    faint, too small or without letters): such a frame may hold an unlabelled caption.
    """
    accepted = profiled_detections(frame, profile.bands)
    bands = profile.bands.primary + profile.bands.secondary
    rows = merge_row(
        [(floor(x0), floor(y0), ceil(x1), ceil(y1)) for x0, y0, x1, y1 in map(corners, accepted)]
    )
    others = [line for line in frame.detections if line not in accepted]
    unreadable = any(
        min(bands) - 1 <= _band(line) <= max(bands) + 2
        and not _eligible(line, width=frame.width, height=frame.height)
        and any(character.isalnum() for character in line.text)
        for line in others
    )
    # Interface text of this frame that reads like the profile's recurring vocabulary: a caption
    # line holding a fragment of a copy of about its own size is a Vision merge of that text into
    # the caption line (small fixed buttons with the same words are not copies).
    moving = [line for line in others if holds_fragment(line.text, profile.bands.vocabulary)]
    return FrameLabels(
        tuple((float(a), float(b), float(c), float(d)) for a, b, c, d in rows),
        tuple(corners(line) for line in others),
        unreadable,
        any(merged(line, moving) for line in accepted),
    )


def scaled(box: Box, factor: float) -> Box:
    return (box[0] * factor, box[1] * factor, box[2] * factor, box[3] * factor)


def touches(box: Box, others: Sequence[Box]) -> bool:
    return any(iou(box, other) > 0 for other in others)


def drop_reason(labels: FrameLabels, prediction: Prediction, *, scale: float) -> str | None:
    """Why a frame's labels are uncertain, or None; ``scale`` maps detector pixels to canvas."""
    if any(CONFIDENCE <= score < FACE_SURE for _, score in prediction.faces):
        return "uncertainFace"
    if labels.unreadable:
        return "unreadableBandText"
    if labels.interface_fragment:
        return "interfaceFragmentCaption"
    lines = labels.captions + labels.interface
    # Predictions carry no caption scores here; a box below CAPTION_SURE was already filtered.
    if any(not touches(scaled(box, scale), lines) for box in prediction.captions):
        return "unreadCaptionBox"
    return None


def label_rows(
    labels: FrameLabels, prediction: Prediction, *, scale: float, size: tuple[int, int]
) -> list[str]:
    """YOLO rows in detector-image pixels: sure faces, then the caption lines."""
    width, height = size

    def row(label: int, box: Box) -> str | None:
        left, top = max(0.0, box[0]), max(0.0, box[1])
        right, bottom = min(float(width), box[2]), min(float(height), box[3])
        if right <= left or bottom <= top:
            return None
        return yolo_row(label, PixelBox(left, top, right - left, bottom - top), canvas=size)

    rows = [row(FACE, box) for box, score in prediction.faces if score >= FACE_SURE]
    rows += [row(CAPTION, scaled(box, 1 / scale)) for box in labels.captions]
    return [item for item in rows if item is not None]


def existing_observations(root: Path, clock: VideoFrameClock) -> dict[int, FrameObservation]:
    """Vision OCR already stored under ``root`` (complete session children), by frame index.

    ``root`` is an escalated chunk's ``attempt-full/ocr`` or an earlier build's ``ocr`` chunk.
    """
    times = {float(pts * clock.time_base): index for index, pts in enumerate(clock.pts)}
    found: dict[int, FrameObservation] = {}
    for child in sorted(root.glob("child-*")):
        if not (child / "native" / "complete.json").is_file():
            continue
        for block in sorted((child / "native").glob("block-*.json")):
            if block.stem.count("-") != 1:
                continue
            for frame in native_ocr_frames(json.loads(block.read_text(encoding="utf-8"))):
                index = times.get(frame.requested_time)
                if index is None or frame.actual_time != frame.requested_time:
                    raise ReplayError("Stored OCR frame does not sit on the chunk clock")
                found[index] = frame
    return found


def parse_manifest(path: Path) -> tuple[list[Recording], dict[str, object]]:
    data: object = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict) or not isinstance(data.get("recordings"), list):
        raise ReplayError("The manifest needs a recordings list")
    recordings: list[Recording] = []
    for item in data["recordings"]:
        if not isinstance(item, dict):
            raise ReplayError("Each recording must be an object")
        name, work, role = item.get("name"), item.get("cleanWork"), item.get("role")
        chunks, step = item.get("chunks"), item.get("frameStep")
        if not isinstance(name, str) or not name.isidentifier() or role not in ROLES:
            raise ReplayError("A recording needs a plain-identifier name and a known role")
        if not isinstance(work, str) or not isinstance(step, int) or step < 1:
            raise ReplayError("A recording needs a cleanup work directory and a positive frameStep")
        if (
            not isinstance(chunks, list)
            or not chunks
            or not all(type(c) is int and c >= 0 for c in chunks)
        ):
            raise ReplayError("A recording needs a list of chunk numbers")
        recordings.append(Recording(name, Path(work), str(role), tuple(chunks), step))
    if len({item.name for item in recordings}) != len(recordings):
        raise ReplayError("Recording names must be unique")
    return recordings, data


class Builder:
    """Writes one recording's chunks; OCR and detection are injected for tests."""

    def __init__(
        self,
        output: Path,
        detect: Detect,
        *,
        ocr: Callable[..., dict[int, FrameObservation]] = session_ocr,
        frames: Callable[..., Iterator[Image]] = chunk_frames,
        reuse: Path | None = None,
    ) -> None:
        self.output, self.detect, self.ocr, self.frames = output, detect, ocr, frames
        # An earlier build's ``ocr`` directory: its frames need no new recognition.
        self.reuse = reuse

    def chunk(
        self,
        recording: Recording,
        number: int,
        profile: CaptionSearchProfile,
        counts: dict[str, int],
    ) -> None:
        config = json.loads(
            (recording.work / "configs" / f"chunk-{number:05d}.json").read_text(encoding="utf-8")
        )
        clock = _clock(config["clock"])
        canvas = (int(config["width"]), int(config["height"]))
        indices = list(range(0, len(clock.pts), recording.frame_step))
        name = f"chunk-{number:05d}"
        observed = existing_observations(
            recording.work / "state" / name / "attempt-full" / "ocr", clock
        )
        # A re-run chunk's earlier state is kept under interrupted/: its source OCR still holds.
        for moved in sorted((recording.work / "interrupted").glob(f"{name}-*")):
            observed = existing_observations(moved / "attempt-full" / "ocr", clock) | observed
        if self.reuse is not None:
            stored = existing_observations(self.reuse / recording.name / name, clock)
            observed = stored | observed
        missing = [index for index in indices if index not in observed]
        if missing:
            observed.update(
                self.ocr(
                    Path(config["native"]),
                    Path(config["source"]),
                    clock,
                    missing,
                    canvas=canvas,
                    work=self.output / "ocr" / recording.name / f"chunk-{number:05d}",
                    root=Path(config["checkoutRoot"]),
                    workers=2,
                    band=ocr_band(profile.measured_envelope, canvas),
                )
            )
        counts["newOcrFrames"] += len(missing)
        needed = set(indices)
        head = VideoFrameClock(clock.pts[: indices[-1] + 1], clock.time_base, clock.last_duration)
        images = [
            (index, image)
            for index, image in enumerate(
                self.frames(
                    Path(config["source"]),
                    head,
                    canvas=canvas,
                    ffmpeg=str(config.get("ffmpeg", "ffmpeg")),
                )
            )
            if index in needed
        ]
        if len(images) != len(indices):
            raise ReplayError("The chunk decode did not give every sampled frame")
        scale = canvas[0] / TRAIN_WIDTH
        for first in range(0, len(images), BATCH):
            group = images[first : first + BATCH]
            for (index, image), prediction in zip(
                group, self.detect([im for _, im in group]), strict=True
            ):
                labels = frame_labels(observed[index], profile)
                reason = drop_reason(labels, prediction, scale=scale)
                if reason is not None:
                    counts[reason] = counts.get(reason, 0) + 1
                    continue
                self.write(
                    recording.name,
                    f"{recording.name}_{number:05d}_{index:03d}",
                    image,
                    labels,
                    prediction,
                    scale,
                )
                counts["frames"] += 1
                counts["captionLines"] += len(labels.captions)
                counts["framesWithCaption"] += bool(labels.captions)

    def write(
        self,
        name: str,
        stem: str,
        image: Image,
        labels: FrameLabels,
        prediction: Prediction,
        scale: float,
    ) -> None:
        size = (image.shape[1], image.shape[0])
        folders = [self.output / kind / name for kind in ("images", "labels", "vision")]
        for folder in folders:
            folder.mkdir(parents=True, exist_ok=True, mode=0o700)
        if not cv2.imwrite(str(folders[0] / f"{stem}.jpg"), image, [cv2.IMWRITE_JPEG_QUALITY, 92]):
            raise ReplayError("A frame could not be written")
        rows = label_rows(labels, prediction, scale=scale, size=size)
        (folders[1] / f"{stem}.txt").write_text(
            "".join(f"{row}\n" for row in rows), encoding="utf-8"
        )
        vision = {"canvasScale": scale, "captions": labels.captions, "interface": labels.interface}
        (folders[2] / f"{stem}.json").write_text(json.dumps(vision), encoding="utf-8")

    def recording(self, recording: Recording) -> dict[str, int]:
        configs = sorted((recording.work / "configs").glob("chunk-*.json"))
        if not configs:
            raise ReplayError("The cleanup work directory has no chunk configurations")
        profile = _profile(
            Path(json.loads(configs[0].read_text(encoding="utf-8"))["profileDirectory"])
        )
        counts = {"frames": 0, "captionLines": 0, "framesWithCaption": 0, "newOcrFrames": 0}
        for number in recording.chunks:
            self.chunk(recording, number, profile, counts)
        print(recording.name, json.dumps(counts), flush=True)
        return counts


def data_yaml(output: Path, recordings: Sequence[Recording], include: Mapping[str, object]) -> str:
    """The Ultralytics description; ``include`` adds image-list files of other datasets per role."""
    lines = [f"path: {output}"]
    for role in ROLES:
        files = [
            str(path)
            for item in recordings
            if item.role == role
            for path in sorted((output / "images" / item.name).glob("*.jpg"))
        ]
        extra = include.get(role, [])
        if not isinstance(extra, list):
            raise ReplayError("include must map a role to image-list files")
        for listing in extra:
            files += Path(str(listing)).read_text(encoding="utf-8").split()
        if files:
            (output / f"{role}.txt").write_text(
                "".join(f"{item}\n" for item in files), encoding="utf-8"
            )
            lines.append(f"{role}: {output / f'{role}.txt'}")
    lines.append("names:")
    lines.extend(f"  {index}: {name}" for index, name in enumerate(CLASS_NAMES))
    return "\n".join(lines) + "\n"


def covered(line: Box, boxes: Sequence[Box]) -> bool:
    """At least ``COVERED`` of the line's area lies inside the union of the boxes."""
    left, top = floor(line[0]), floor(line[1])
    width, height = max(1, ceil(line[2]) - left), max(1, ceil(line[3]) - top)
    mask = np.zeros((height, width), dtype=bool)
    for box in boxes:
        x0, y0 = max(0, floor(box[0]) - left), max(0, floor(box[1]) - top)
        x1, y1 = min(width, ceil(box[2]) - left), min(height, ceil(box[3]) - top)
        if x1 > x0 and y1 > y0:
            mask[y0:y1, x0:x1] = True
    return bool(mask.mean() >= COVERED)


def box_of(value: object) -> Box:
    if not isinstance(value, list) or len(value) != 4:
        raise ReplayError("A stored Vision box must be four numbers")
    left, top, right, bottom = (float(item) for item in value)
    return (left, top, right, bottom)


def score(
    vision: Mapping[str, Any], prediction: Prediction, grown: Prediction, tally: dict[str, int]
) -> None:
    """Add one frame: caption lines found or missed, and caption boxes on interface text only.

    ``grown`` holds the boxes as production repairs them (grown along their glyphs); with the
    mask padding they give the production recall beside the raw detector recall.
    """
    factor = float(vision["canvasScale"])
    boxes = [scaled(box, factor) for box in prediction.captions]
    masks = [
        (left - MASK_PAD, top - MASK_PAD, right + MASK_PAD, bottom + MASK_PAD)
        for left, top, right, bottom in (scaled(box, factor) for box in grown.captions)
    ]
    captions = [box_of(value) for value in vision["captions"]]
    interface = [box_of(value) for value in vision["interface"]]
    found = [covered(line, boxes) for line in captions]
    repaired = [covered(line, masks) for line in captions]
    tally["linesRepaired"] += sum(repaired)
    tally["framesAllUnrepaired"] += bool(captions) and not any(repaired)
    tally["frames"] += 1
    tally["lines"] += len(captions)
    tally["linesFound"] += sum(found)
    tally["framesWithCaption"] += bool(captions)
    tally["framesAllMissed"] += bool(captions) and not any(found)
    tally["boxes"] += len(boxes)
    tally["interfaceFalseBoxes"] += sum(
        touches(box, interface) and not touches(box, captions) for box in boxes
    )


def evaluate(detect: Detect, dataset: Path, name: str) -> dict[str, object]:
    files = sorted((dataset / "images" / name).glob("*.jpg"))
    if not files:
        raise ReplayError("No evaluation images for the recording")
    keys = (
        "frames",
        "lines",
        "linesFound",
        "framesWithCaption",
        "framesAllMissed",
        "boxes",
        "interfaceFalseBoxes",
        "linesRepaired",
        "framesAllUnrepaired",
    )
    tally = dict.fromkeys(keys, 0)
    for first in range(0, len(files), BATCH):
        group = files[first : first + BATCH]
        images = [np.asarray(cv2.imread(str(path)), dtype=np.uint8) for path in group]
        for path, image, prediction in zip(group, images, detect(images), strict=True):
            vision = json.loads(
                (dataset / "vision" / name / f"{path.stem}.json").read_text(encoding="utf-8")
            )
            grown = tuple(extend_to_glyphs(image, box) for box in prediction.captions)
            score(vision, prediction, Prediction(prediction.faces, grown), tally)
    return {
        **tally,
        "lineRecall": round(tally["linesFound"] / max(1, tally["lines"]), 4),
        "missedLines": tally["lines"] - tally["linesFound"],
        "productionRecall": round(tally["linesRepaired"] / max(1, tally["lines"]), 4),
        "productionMissedLines": tally["lines"] - tally["linesRepaired"],
    }


def detector(
    weights: Path,
    digest: str,
    config_directory: Path,
    work: Path,
    *,
    device: str,
    caption_floor: float,
) -> Detect:
    """The checkpoint as a batch detector at the production size; nothing is downloaded."""
    check_checkpoint(weights, digest)
    check_settings(config_directory, work)
    os.environ["YOLO_CONFIG_DIR"] = str(config_directory)
    os.environ["ULTRALYTICS_OFFLINE"] = "1"
    from ultralytics.models.yolo.model import YOLO

    model = YOLO(str(weights))

    def detect(images: list[Image]) -> list[Prediction]:
        results = list(
            model.predict(images, imgsz=TRAIN_WIDTH, device=device, conf=CONFIDENCE, verbose=False)
        )
        return [
            sure_captions(item, result, caption_floor)
            for item, result in zip(predictions_from(results), results, strict=True)
        ]

    return detect


def sure_captions(prediction: Prediction, result: Any, caption_floor: float) -> Prediction:
    """The prediction keeping only caption boxes at or above ``caption_floor``."""
    boxes = result.boxes
    kept = tuple(
        (float(c[0]), float(c[1]), float(c[2]), float(c[3]))
        for label, value, c in zip(
            boxes.cls.tolist(), boxes.conf.tolist(), boxes.xyxy.tolist(), strict=True
        )
        if int(label) == CAPTION and value >= caption_floor
    )
    return Prediction(prediction.faces, kept)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    build = commands.add_parser("build")
    build.add_argument("manifest", type=Path)
    check = commands.add_parser("evaluate")
    check.add_argument("dataset", type=Path)
    check.add_argument("output", type=Path)
    check.add_argument("--recording", action="append", required=True)
    for command in (build, check):
        command.add_argument("--weights", type=Path, required=True)
        command.add_argument("--weights-sha256", required=True)
        command.add_argument("--config-directory", type=Path, required=True)
        command.add_argument("--work", type=Path, required=True)
        command.add_argument("--device", choices=("mps", "cpu"), default="mps")
    args = parser.parse_args(argv)
    if args.command == "build":
        recordings, manifest = parse_manifest(args.manifest)
        output = Path(str(manifest.get("output")))
        if output.exists():
            raise ReplayError("Refusing to overwrite the dataset")
        output.mkdir(mode=0o700, parents=True)
        with released_mps_graphs():
            detect = detector(
                args.weights,
                args.weights_sha256,
                args.config_directory,
                args.work,
                device=args.device,
                caption_floor=CAPTION_SURE,
            )
            reuse = manifest.get("reuseOcr")
            if reuse is not None and not isinstance(reuse, str):
                raise ReplayError("reuseOcr must be an earlier build's ocr directory")
            builder = Builder(output, detect, reuse=None if reuse is None else Path(reuse))
            summary = {
                item.name: {"role": item.role, **builder.recording(item)} for item in recordings
            }
        include = manifest.get("include", {})
        if not isinstance(include, dict):
            raise ReplayError("include must be an object")
        (output / "data.yaml").write_text(data_yaml(output, recordings, include), encoding="utf-8")
        (output / "summary.json").write_text(json.dumps(summary, indent=1), encoding="utf-8")
        print(json.dumps(summary))
        return 0
    if args.output.exists():
        raise ReplayError("Refusing to overwrite the output")
    with released_mps_graphs():
        detect = detector(
            args.weights,
            args.weights_sha256,
            args.config_directory,
            args.work,
            device=args.device,
            caption_floor=CONFIDENCE,
        )
        result = {name: evaluate(detect, args.dataset, name) for name in args.recording}
    document = {"weights": str(args.weights), "sha256": args.weights_sha256, "recordings": result}
    args.output.write_text(json.dumps(document, indent=1), encoding="utf-8")
    print(json.dumps(document, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
