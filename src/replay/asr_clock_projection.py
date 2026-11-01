"""Project assembled continuous-PCM recognition onto decoded source presentation.

The pinned lineage declaration is an explicit caller contract: producer domain
verification must have completed before invoking this adapter. Hashes establish
artifact identity, not semantic correctness of an arbitrary producer command.
"""

import argparse
import json
import math
import os
import wave
from collections.abc import Sequence
from dataclasses import asdict, replace
from fractions import Fraction
from itertools import pairwise
from pathlib import Path
from typing import TextIO

from replay.audio_clock import AudioFrameClock
from replay.caption_repair import ClockLabel
from replay.core import ReplayError
from replay.input_hash_snapshot import InputHashSnapshot
from replay.recordings import record
from replay.stages import file_digest

MAX_JSON_BYTES = 128 * 1024 * 1024
CLOCK_DIGITS = 6
# The label count is bounded by the source's own duration instead of a fixed wall, so a longer
# stream needs no code change: Japanese word timestamps stay far below 8 labels per second.
LABELS_PER_SECOND = 8
MINIMUM_LABELS = 100_000


def project_labels(
    labels: tuple[ClockLabel, ...], clock: AudioFrameClock
) -> tuple[ClockLabel, ...]:
    """Keep text/provenance and unresolved points; reject reversed native support."""
    projected: list[ClockLabel] = []
    for label in labels:
        if label.start == label.end:
            start = end = clock.source_time(label.start)
        else:
            start, end = clock.source_interval(label.start, label.end)
        # Microseconds: recognition clocks carry float sums whose noise (~1e-12 s) otherwise
        # reverses equal onsets; rounding is monotonic, so real order is kept.
        projected.append(
            replace(
                label, start=round(float(start), CLOCK_DIGITS), end=round(float(end), CLOCK_DIGITS)
            )
        )
    if any(right.start < left.start for left, right in pairwise(projected)):
        raise ReplayError("Projected recognition reverses word onsets")
    return tuple(projected)


def _read(path: Path) -> dict[str, object]:
    if path.stat().st_size > MAX_JSON_BYTES:
        raise ReplayError("Projection JSON exceeds its byte budget")
    return record(json.loads(path.read_text(encoding="utf-8")))


def _number(value: object) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ReplayError("Recognition clock must be numeric")
    return float(value)


MAX_LABELS = 1_000_000  # ~140k words for a 10-hour stream (a 1-2 hour replay has ~20-40k)


def label_budget(clock: AudioFrameClock) -> int:
    """Labels this source may carry: its length times the per-second ceiling."""
    seconds = clock.sample_ends[-1] / clock.sample_rate
    return max(MINIMUM_LABELS, math.ceil(seconds * LABELS_PER_SECOND))


def _labels(document: dict[str, object]) -> tuple[ClockLabel, ...]:
    values = document.get("labels")
    if (
        document.get("version") != 1
        or document.get("clock") != "input-pcm-not-projected"
        or not isinstance(values, list)
    ):
        raise ReplayError("Expected bounded unprojected assembled recognition")
    labels: list[ClockLabel] = []
    for value in values:
        item = record(value)
        text, provenance = item.get("text"), item.get("provenance")
        if not isinstance(text, str) or not isinstance(provenance, str):
            raise ReplayError("Recognition text/provenance is missing")
        labels.append(
            ClockLabel(text, _number(item.get("start")), _number(item.get("end")), provenance)
        )
    return tuple(labels)


def _binding(value: object) -> tuple[Path, str]:
    item = record(value)
    path, digest = item.get("path"), item.get("sha256")
    if set(item) != {"path", "sha256"} or not isinstance(path, str) or not isinstance(digest, str):
        raise ReplayError("Expected explicit artifact path and SHA256")
    return Path(path), digest


def run(config: Path, output: Path) -> int:
    """Consume pinned completed lineage; never modify ASR or infer missing times.

    Lineage declares duration-preserving concatenation/resampling, with no trim,
    padding, async timestamp resampling or prior source projection. Stage receipts
    bind clock and PCM; the ASR manifest must bind the identical PCM. Source-media
    lineage and producer-domain checks remain separately retained prerequisites.
    """
    if (
        output.exists()
        or output.is_symlink()
        or output.parent != output.parent.resolve(strict=True)
    ):
        raise ReplayError("Expected a new output under a canonical directory")
    config_digest = file_digest(config)
    data = _read(config)
    if data.get("version") != 1 or data.get("inputClock") != "continuous-decoded-pcm":
        raise ReplayError("Unsupported projection clock contract")
    if data.get("transforms") != ["concatenate-decoded-frames", "duration-preserving-resample"]:
        raise ReplayError("Unproven PCM transform chain")
    names = (
        "labels",
        "pcm",
        "audioClock",
        "asrInputs",
        "clockReceipt",
        "pcmReceipt",
        "lineageEvidence",
    )
    artifacts = record(data.get("artifacts"))
    if set(artifacts) != set(names):
        raise ReplayError("Projection requires complete artifact lineage")
    bindings = {name: _binding(artifacts[name]) for name in names}
    snapshot = InputHashSnapshot.capture([(config, config_digest), *bindings.values()])
    paths = {name: binding[0] for name, binding in bindings.items()}
    for receipt_name, artifact_name in (("clockReceipt", "audioClock"), ("pcmReceipt", "pcm")):
        receipt = _read(paths[receipt_name])
        if (
            receipt.get("status") != "complete"
            or record(receipt.get("outputs")).get(str(paths[artifact_name]))
            != bindings[artifact_name][1]
        ):
            raise ReplayError("Producer receipt does not bind the completed artifact")
    assembled, manifest = _read(paths["labels"]), _read(paths["asrInputs"])
    if (
        manifest.get("clock") != "input-pcm-not-projected"
        or record(manifest.get("inputs")).get(str(paths["pcm"])) != bindings["pcm"][1]
        or record(assembled.get("parentSHA256")).get(str(paths["asrInputs"]))
        != bindings["asrInputs"][1]
    ):
        raise ReplayError("Recognition does not bind the declared PCM")
    clock = AudioFrameClock.from_probe(_read(paths["audioClock"]))
    with wave.open(str(paths["pcm"]), "rb") as stream:
        samples, rate = stream.getnframes(), stream.getframerate()
        if (
            rate != 16000
            or stream.getnchannels() != 1
            or stream.getsampwidth() != 2
            or stream.getcomptype() != "NONE"
            or samples != manifest.get("samples")
            or rate != manifest.get("sampleRate")
            or abs(Fraction(samples, rate) - Fraction(clock.sample_ends[-1], clock.sample_rate))
            > Fraction(1, rate)
        ):
            raise ReplayError("PCM is not duration-preserving mono PCM16/16k recognition input")
    labels = _labels(assembled)
    if len(labels) > label_budget(clock):
        raise ReplayError("Recognition carries more labels than its audio length allows")
    projected = project_labels(labels, clock)
    snapshot.finish()
    result = {
        "version": 1,
        "clock": "source-presentation",
        "parentSHA256": {str(path): digest for path, digest in bindings.values()},
        "configSHA256": config_digest,
        "projectionSHA256": file_digest(Path(__file__)),
        "audioClockCodeSHA256": file_digest(Path(__file__).with_name("audio_clock.py")),
        "sourceStart": str(clock.pts[0] * clock.time_base),
        "unresolvedPoints": sum(label.start == label.end for label in projected),
        "lineageIsCallerDeclared": True,
        "humanReviewed": False,
        "speechAccuracyVerified": False,
        "deliveryComplete": False,
    }
    with output.open("x", encoding="utf-8") as stream:
        os.fchmod(stream.fileno(), 0o600)
        _write(stream, {key: value for key, value in result.items() if key != "labels"}, projected)
    return len(projected)


def _write(stream: TextIO, result: dict[str, object], labels: Sequence[ClockLabel]) -> None:
    """Write the receipt with its labels one at a time: a ten-hour stream's array never
    becomes one in-memory string, so peak memory follows the label size, not the recording."""
    stream.write(json.dumps(result, ensure_ascii=False, allow_nan=False, indent=2)[:-2])
    stream.write(',\n  "labels": [')
    for index, label in enumerate(labels):
        prefix = ",\n    " if index else "\n    "
        stream.write(prefix + json.dumps(asdict(label), ensure_ascii=False, allow_nan=False))
    stream.write("\n  ]\n}\n")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("config", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args(argv)
    run(args.config, args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
