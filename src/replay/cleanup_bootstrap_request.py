"""Write the cleanup-bootstrap request for one recording from a reference request.

The request names the dated original, its frame clock, its stream metadata, the pinned native
tool and the policy fields (decoder, model, sample counts, chunking, workers) that every recording
shares. Everything recording-specific is read from the recording's preparation root; the policy
comes from a reference request, never from per-video values. Outputs are new files only.

The one derived policy value is the profile sample count: the reference count keeps its density
(one sample per ``PROFILE_SPACING`` seconds) on longer sources, up to the planner's cap. Measured
2026-10-26 on a 10.2 h source: 64 samples (one per 575 s) learned one caption band and a 125 px
envelope; 128 or more learned the second band, 307 (one per 120 s) a 159 px envelope.
"""

import argparse
import hashlib
import json
import math
import os
import subprocess
import sys
from pathlib import Path

from replay.core import ReplayError
from replay.replay_cleanup_plan import MAX_PROFILE_SAMPLES
from replay.repository import root as checkout_root

ROOT = checkout_root()
POLICY = ("decoder", "model", "profileSamples", "nativeTimescale", "chunkFrames", "workers")
# The reference's 64 profile samples were set on ~2 h recordings: about one per two minutes.
PROFILE_SPACING = 120.0


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def pin(path: Path) -> dict[str, str]:
    return {"path": str(path.resolve()), "sha256": sha256(path)}


def source_digest(preparation: Path, source: Path) -> str:
    """The sha256 the replay preparation recorded for `source`."""
    for row in json.loads(preparation.read_text(encoding="utf-8"))["inputs"]:
        if row["path"] == str(source):
            return str(row["sha256"])
    raise ReplayError("The preparation does not pin this source")


def write_new(path: Path, data: object) -> None:
    with path.open("x", encoding="utf-8") as stream:
        os.fchmod(stream.fileno(), 0o600)
        json.dump(data, stream, indent=1)


def probe(source: Path, ffprobe: Path) -> dict[str, object]:
    result = subprocess.run(
        [str(ffprobe), "-v", "error", "-show_streams", "-show_format", "-of", "json", str(source)],
        capture_output=True,
        check=True,
        timeout=120,
    )
    loaded: object = json.loads(result.stdout)
    if not isinstance(loaded, dict):
        raise ReplayError("ffprobe did not return an object")
    return loaded


def profile_samples(reference: object, probed: dict[str, object]) -> int:
    """The reference sample count, raised to one per ``PROFILE_SPACING`` s of the source."""
    form = probed.get("format")
    duration = form.get("duration") if isinstance(form, dict) else None
    if type(reference) is not int or not isinstance(duration, str):
        raise ReplayError("The reference sample count or the source duration is missing")
    return max(reference, min(MAX_PROFILE_SAMPLES, math.ceil(float(duration) / PROFILE_SPACING)))


def build(
    work: Path, source: Path, reference: Path, native: Path, ffprobe: Path, output: Path
) -> Path:
    """Create `output/` with the stream metadata and the request; return the request path."""
    if output.exists() or output.is_symlink():
        raise ReplayError("Refusing to overwrite the output directory")
    for name in ("preparation.json", "video-clock.json"):
        if not (work / name).is_file():
            raise ReplayError(f"The preparation root has no {name}")
    shared = json.loads(reference.read_text(encoding="utf-8"))
    if not all(key in shared for key in POLICY):
        raise ReplayError("The reference request lacks policy fields")
    digest = source_digest(work / "preparation.json", source)
    clock = work / "video-clock.json"
    probed = probe(source, ffprobe)
    samples = profile_samples(shared["profileSamples"], probed)
    output.mkdir(mode=0o700)
    metadata = output / "source-stream-metadata-v1.json"
    write_new(
        metadata,
        {
            "sourceSHA256": digest,
            "frameClock": pin(clock),
            "streams": probed["streams"],
            "format": probed["format"],
        },
    )
    request = {
        "version": 1,
        "source": {"path": str(source), "sha256": digest},
        "frameClock": pin(clock),
        "sourceMetadata": pin(metadata),
        "native": pin(native),
        **{key: shared[key] for key in POLICY},
        "profileSamples": samples,
    }
    target = output / "BOOTSTRAP-REQUEST-v1.json"
    write_new(target, request)
    return target


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("work", type=Path, help="the recording's preparation root")
    parser.add_argument("source", type=Path, help="the dated original")
    parser.add_argument("reference", type=Path, help="an existing BOOTSTRAP-REQUEST json")
    parser.add_argument("output", type=Path, help="new directory (cleanup-preparation-v1)")
    parser.add_argument("--native", type=Path, required=True)
    parser.add_argument("--ffprobe", type=Path, default=Path("/opt/homebrew/bin/ffprobe"))
    args = parser.parse_args(argv)
    print(build(args.work, args.source, args.reference, args.native, args.ffprobe, args.output))
    return 0


if __name__ == "__main__":
    sys.exit(main())
