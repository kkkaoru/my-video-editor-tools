"""Make a delivery's Compressor YouTube encode its master and drop the delivery it came from.

The user decided (2026-11-02, "Compressor でエンコードした動画が正として、エンコード前の編集済み
動画の保管は不要です") that the encode is the master. Before anything is removed the encode is
verified against its delivery: the same frame size (an allowed YouTube size), the same number of
video frames, a clean full decode, and every sampled
frame (one each SAMPLE_FRAMES) within MINIMUM_PSNR of the delivery's frame at the same index.
The proof is ``metadata/reencode-verification.json`` beside the master.

A pipeline delivery (one with ``metadata/pipeline-evidence.json``) is published again through
``pipeline_completion.publish`` as revision ``youtube-<sha12>``: the same evidence, every stage
check runs on the master's own bytes, and the older revisions under its name are superseded
(``release_older``); the master must pass ``pipeline_completion.assess`` before the delivery is
deleted. An excerpt (no evidence) gets the same verification and moves to
``revisions/youtube-<sha12>/delivery`` with its transcript. The master is the encode's video and
the delivery's own audio stream-copied into an MP4 (no second encode): Compressor resamples the
audio to 48 kHz and its AAC priming trims the tail by up to a few frames, which the one-frame
audio check of the completion gate refuses, while the delivery's audio is the very source of it
and already passed that check. Its saved summaries, excitement levels and caption plan (with its
face track, nameplate icon and position) come along. The
delivery and every hard link of it (``final/candidate.mp4``), the remux's staging name and the
encode are then deleted, each logged in ``deletions.md``.

    python -m replay.youtube_master [--ffmpeg F] [--ffprobe P] DELIVERY.mp4 [DELIVERY.mp4 ...]
"""

import argparse
import json
import os
import re
import shutil
import subprocess
from collections.abc import Sequence
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path

from replay import movie_decode, youtube_encode
from replay import pipeline_completion as completion
from replay import recording_viewer_steps as viewer
from replay.core import ReplayError
from replay.recordings import record
from replay.repository import root as checkout_root
from replay.source_lead_in import sha256

SAMPLE_FRAMES = 30
MINIMUM_PSNR = 35.0
INTENT = (
    "2026-11-02 user: Compressor でエンコードした動画が正として、"
    "エンコード前の編集済み動画の保管は不要です"
)
RECORD = "reencode-verification.json"
# The caption plan, face track, nameplate icon and position the viewers draw captions from: the
# master has the delivery's frames, so they hold for it unchanged.
SIDECARS = (
    ".srt.excitement.json", ".captions.json", ".faces.json", ".nameplate.png",
    ".caption-layout.json",
)  # fmt: skip
SUMMARIES = ".srt.summaries"
DELETIONS = Path("edits/work/recording-pipeline/deletions.md")
PSNR = re.compile(r"average:([0-9.]+|inf)")


@dataclass(frozen=True, slots=True)
class Probe:
    size: tuple[int, int]
    frames: int
    video_seconds: float
    audio: tuple[float, ...]


def probe(movie: Path, ffprobe: str) -> Probe:
    found = record(json.loads(subprocess.run(
        [ffprobe, "-v", "error", "-count_packets", "-show_entries",
         "stream=codec_type,width,height,nb_read_packets,duration", "-of", "json", str(movie)],
        capture_output=True, text=True, check=True,
    ).stdout))  # fmt: skip
    listed = found.get("streams")
    streams = [record(item) for item in listed] if isinstance(listed, list) else []
    video = [item for item in streams if item.get("codec_type") == "video"]
    if len(video) != 1:
        raise ReplayError(f"{movie.name} does not hold exactly one video stream")
    return Probe(
        (int(str(video[0]["width"])), int(str(video[0]["height"]))),
        int(str(video[0]["nb_read_packets"])),
        float(str(video[0]["duration"])),
        tuple(
            float(str(item["duration"])) for item in streams if item.get("codec_type") == "audio"
        ),
    )


def sampled_psnr(delivery: Path, encode: Path, ffmpeg: str) -> float:
    """The mean PSNR of every SAMPLE_FRAMES-th frame, matched by frame index."""
    pick = f"select='not(mod(n\\,{SAMPLE_FRAMES}))',setpts=N"
    result = subprocess.run(
        [ffmpeg, "-v", "info", "-nostdin", "-i", str(encode), "-i", str(delivery),
         "-filter_complex", f"[0:v]{pick}[a];[1:v]{pick}[b];[a][b]psnr", "-f", "null", "-"],
        capture_output=True, text=True, check=True,
    )  # fmt: skip
    found = PSNR.findall(result.stderr)
    if not found:
        raise ReplayError("ffmpeg reported no PSNR")
    return float("inf") if found[-1] == "inf" else float(found[-1])


def verified(delivery: Path, encode: Path, *, ffmpeg: str, ffprobe: str) -> dict[str, object]:
    """The proof that ``encode`` shows ``delivery`` frame for frame (see the docstring)."""
    original, made = probe(delivery, ffprobe), probe(encode, ffprobe)
    frame = 1 / max(1.0, made.frames / max(made.video_seconds, 1e-9))
    checks = {
        "size": made.size == original.size and youtube_encode.allowed(made.size),
        "frames": made.frames == original.frames,
        "audio": len(original.audio) == 1
        and abs(original.audio[0] - original.video_seconds) <= frame,
        "duration": abs(made.video_seconds - original.video_seconds) <= frame,
        "decodes": movie_decode.decodes(encode, ffmpeg=ffmpeg, ffprobe=ffprobe, frames=made.frames),
    }
    failed = [name for name, passed in checks.items() if not passed]
    if failed:
        raise ReplayError(f"{encode.name} does not match {delivery.name}: {', '.join(failed)}")
    psnr = sampled_psnr(delivery, encode, ffmpeg)
    if psnr < MINIMUM_PSNR:
        raise ReplayError(f"{encode.name} differs from {delivery.name} (PSNR {psnr:.1f} dB)")
    return {
        "version": 1, "intent": INTENT,
        "delivery": {"path": str(delivery), "bytes": delivery.stat().st_size,
                     "sha256": sha256(delivery)},
        "encode": {"path": str(encode), "bytes": encode.stat().st_size, "sha256": sha256(encode),
                   "setting": str(youtube_encode.SETTINGS)},
        "frames": made.frames, "size": list(made.size), "audioFrom": "delivery",
        "psnrMeanDb": round(psnr, 2),
        "psnrSampleFrames": SAMPLE_FRAMES, "minimumPsnrDb": MINIMUM_PSNR,
        "verifiedAt": datetime.now(UTC).isoformat(),
    }  # fmt: skip


def remux(encode: Path, delivery: Path, output: Path, ffmpeg: str) -> None:
    """The encode's video and the delivery's audio copied into an MP4 (no timecode track)."""
    partial = output.with_name(f"partial-{output.name}")
    partial.unlink(missing_ok=True)
    subprocess.run(
        [ffmpeg, "-v", "error", "-nostdin", "-i", str(encode), "-i", str(delivery),
         "-map", "0:v:0", "-map", "1:a:0",
         "-c", "copy", "-movflags", "+faststart", str(partial)],
        check=True,
    )  # fmt: skip
    partial.rename(output)


def carry(delivery: Path, master: Path) -> None:
    """The delivery's saved summaries and excitement levels, beside the master."""
    for suffix in SIDECARS:
        source = delivery.with_suffix(suffix)
        if source.is_file() and not master.with_suffix(suffix).exists():
            shutil.copy2(source, master.with_suffix(suffix))
    summaries = delivery.with_suffix(SUMMARIES)
    if summaries.is_dir() and not master.with_suffix(SUMMARIES).exists():
        shutil.copytree(summaries, master.with_suffix(SUMMARIES))
        os.chmod(master.with_suffix(SUMMARIES), 0o700)


def links_of(movie: Path, candidates: Sequence[Path]) -> list[Path]:
    """``movie`` and each candidate path that is the same file."""
    return [movie, *[path for path in candidates if path.exists() and path.samefile(movie)]]


def delete(paths: Sequence[Path], reason: str) -> None:
    log = checkout_root() / DELETIONS
    for path in paths:
        if path.exists():
            size = path.stat().st_size
            path.unlink()
            with log.open("a", encoding="utf-8") as stream:
                stream.write(f"- {datetime.now(UTC).date()} {path} ({size} bytes): {reason}\n")


def master(delivery: Path, *, ffmpeg: str, ffprobe: str) -> Path:
    """Make ``delivery``'s encode its master (see the module docstring); the master's path."""
    encode = youtube_encode.output_of(delivery)
    if not delivery.is_file() or not encode.is_file():
        raise ReplayError(f"{delivery.name} or its YouTube encode is missing")
    proof = verified(delivery, encode, ffmpeg=ffmpeg, ffprobe=ffprobe)
    staged = encode.with_name(delivery.name)
    if not staged.exists():
        remux(encode, delivery, staged, ffmpeg)
    revision = f"youtube-{sha256(staged)[:12]}"
    evidence = delivery.parents[1] / "metadata" / completion.INDEX_NAME
    links: list[Path] = []
    if evidence.is_file():
        published, links = republished(delivery, evidence, staged, revision)
    else:
        published = delivery.parents[1] / "revisions" / revision / "delivery" / delivery.name
        published.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        if not published.exists():
            os.link(staged, published)
        transcript = published.with_suffix(".srt")
        if not transcript.exists():
            shutil.copy2(delivery.with_suffix(".srt"), transcript)
    (published.parents[1] / "metadata").mkdir(mode=0o700, exist_ok=True)
    (published.parents[1] / "metadata" / RECORD).write_text(
        json.dumps({**proof, "master": {"path": str(published), "sha256": sha256(published)}},
                   indent=1, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )  # fmt: skip
    carry(delivery, published)
    reason = f"replaced by its verified YouTube encode {published} ({RECORD})"
    delete([*links_of(delivery, links), staged, encode], reason)
    if not evidence.is_file():  # an excerpt's own folder keeps nothing of the old delivery
        delete([delivery.with_suffix(suffix) for suffix in (".srt", *SIDECARS)], reason)
        summaries = delivery.with_suffix(SUMMARIES)
        if summaries.is_dir():
            delete(sorted(summaries.iterdir()), reason)
            summaries.rmdir()
    return published


def republished(
    delivery: Path, evidence: Path, staged: Path, revision: str
) -> tuple[Path, list[Path]]:
    """``staged`` published as the recording's newest revision with the same evidence, its older
    revisions superseded and its completion check passed; the delivery's pipeline hard links."""
    index = completion.read(evidence)
    root = checkout_root() / "edits" / str(index["recording"])
    finish = Path(str(record(record(index["steps"])["finish"])["path"]))
    published, new_index = completion.publish(
        root, movie=staged, steps=finish.parent,
        candidate=Path(str(record(index["candidate"])["path"])), revision=revision,
    )  # fmt: skip
    viewer.release_older(root, published)
    carry(delivery, published)  # the completion check reads the caption plan beside the master
    failures = completion.assess(new_index, delivery=published, root=root)
    if failures:
        raise ReplayError(f"The master fails its completion check: {failures}")
    candidate = record(record(completion.read(finish).get("outputs")).get("movie")).get("path")
    return published, [Path(str(candidate))] if candidate else []


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("deliveries", type=Path, nargs="+")
    parser.add_argument("--ffmpeg", default="ffmpeg")
    parser.add_argument("--ffprobe", default="ffprobe")
    args = parser.parse_args(argv)
    for delivery in args.deliveries:
        print(master(delivery, ffmpeg=args.ffmpeg, ffprobe=args.ffprobe))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
