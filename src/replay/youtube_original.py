"""Verified YouTube originals, with a provisional copy while a stream archive is processed.

Right after a stream ends YouTube serves the archive as ``post_live`` DASH while it is still being
processed: it can lack the last seconds and refuse fragments of its best audio format. Such a
download is kept as a *provisional* original, using the best audio format that downloads whole,
and its measured stream lengths are recorded. Once YouTube serves the processed archive, a later
run downloads it, verifies it against the metadata duration and replaces the provisional original
at the same path. Downloads stage in ``.<stem>.<live_status>.partial`` and abort on any
unavailable fragment, so a failed attempt is resumed instead of becoming an original.

A volume detaching mid-run (``replay.volume_resume`` reruns the command) leaves a state the next
run resumes: staging keeps its ``.part`` fragments and verified evidence, and a replacement cut
between its two renames is finished or rolled back before anything else.
"""

import fcntl
import json
import math
import os
import shutil
import subprocess
import sys
from pathlib import Path

from replay.core import ReplayError
from replay.recordings import record
from replay.volume_resume import require_mounted
from replay.youtube_delivery import _metadata, _yt_dlp, channel_record, source_stem, write_channel

FINISHED = frozenset({"not_live", "was_live", "post_live"})
PROCESSING = "post_live"
# Metadata duration is whole seconds (±1 s); muxed stream ends add under one AAC packet/frame.
TOLERANCE_SECONDS = 1.5
# post_live DASH serves ~1 s fragments; sequential fetching took ~4 h for a 1.5 h archive.
FRAGMENT_WORKERS = 8
STREAM_KINDS = ("video", "audio")
EVIDENCE = frozenset({"publication.json", "channel.json"})
REPLACED = ".provisional.replaced"


def expected_duration(metadata: object) -> tuple[str, float]:
    data = record(metadata)
    status = data.get("live_status")
    if status not in FINISHED:
        raise ReplayError(f"YouTube video is not a finished upload or archive: {status}")
    duration = data.get("duration")
    if (
        isinstance(duration, bool)
        or not isinstance(duration, (int, float))
        or not math.isfinite(duration)
        or duration <= 0
    ):
        raise ReplayError("YouTube metadata has no duration to verify the download against")
    return str(status), float(duration)


def audio_formats(metadata: object) -> list[str]:
    """Audio-only format IDs, best first (yt-dlp lists formats from worst to best)."""
    formats = record(metadata).get("formats")
    if not isinstance(formats, list):
        raise ReplayError("YouTube metadata lists no formats")
    choices = [record(item) for item in reversed(formats)]
    return [
        str(item["format_id"])
        for item in choices
        if item.get("vcodec") == "none"
        and item.get("acodec") not in (None, "none")
        and isinstance(item.get("format_id"), str)
    ]


def stream_durations(ffprobe: Path, media: Path) -> dict[str, float]:
    result = subprocess.run(
        [
            str(ffprobe),
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,duration",
            "-of",
            "json",
            str(media),
        ],
        capture_output=True,
        text=True,
        check=True,
        timeout=120,
    )
    streams = record(json.loads(result.stdout)).get("streams")
    if not isinstance(streams, list):
        raise ReplayError("ffprobe returned no streams")
    durations: dict[str, float] = {}
    for kind in STREAM_KINDS:
        found = [record(s) for s in streams if record(s).get("codec_type") == kind]
        if len(found) != 1:
            raise ReplayError(f"Expected one {kind} stream in the download")
        try:
            durations[kind] = float(str(found[0].get("duration")))
        except ValueError as error:
            raise ReplayError(f"Downloaded {kind} stream has no duration") from error
    return durations


def check_durations(durations: dict[str, float], duration: float, *, provisional: bool) -> None:
    """A processed archive matches the metadata; a provisional one may only fall short."""
    for kind, seconds in durations.items():
        too_short = seconds < duration - TOLERANCE_SECONDS
        if not 0 < seconds <= duration + TOLERANCE_SECONDS or (too_short and not provisional):
            raise ReplayError(
                f"Downloaded {kind} lasts {seconds:.3f}s, YouTube reports {duration:.0f}s;"
                " the archive is incomplete"
            )


def is_provisional(directory: Path) -> bool:
    try:
        evidence = json.loads((directory / "publication.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return False
    return isinstance(evidence, dict) and evidence.get("provisional") is True


def fetch(common: list[str], ffmpeg_directory: Path, target: str, url: str, fmt: str) -> None:
    subprocess.run(
        [
            *common,
            "--ffmpeg-location",
            str(ffmpeg_directory),
            "--no-overwrites",
            "--abort-on-unavailable-fragments",
            "--concurrent-fragments",
            str(FRAGMENT_WORKERS),
            "--match-filter",
            "!is_live",
            "-f",
            fmt,
            "--merge-output-format",
            "mp4",
            "--remux-video",
            "mp4",
            "-o",
            target,
            url,
        ],
        check=True,
    )


def fetch_provisional(
    common: list[str], ffmpeg_directory: Path, target: str, url: str, audio: list[str]
) -> str:
    """Use the best audio format the processing archive serves whole; return its ID.

    The archive refuses (HTTP 401) a growing run of fragments per attempt, so a format is resumed
    while each attempt adds bytes, and abandoned only when an attempt makes no progress.
    """
    staging = Path(target).parent
    device = staging.stat().st_dev
    for format_id in audio:
        while True:
            before = partial_bytes(staging, format_id)
            try:
                fetch(common, ffmpeg_directory, target, url, f"bv*+{format_id}")
            except subprocess.CalledProcessError as error:
                if not staging.is_dir() or staging.stat().st_dev != device:
                    # A detached volume is not a refused format: keep its fragments for a rerun.
                    raise ReplayError(f"{staging} went away during the download") from error
                if partial_bytes(staging, format_id) > before:
                    continue
                break
            return format_id
        # This format's leftovers must not reach the original.
        for leftover in staging.glob(f"*.f{format_id}.*"):
            leftover.unlink()
    raise ReplayError("No audio format of the processing archive downloads completely")


def partial_bytes(staging: Path, format_id: str) -> int:
    return sum(path.stat().st_size for path in staging.glob(f"*.f{format_id}.*"))


def write_evidence(staging: Path, evidence: dict[str, object]) -> None:
    """Write publication.json atomically; a rerun accepts only identical evidence."""
    path = staging / "publication.json"
    if path.exists():
        if json.loads(path.read_text(encoding="utf-8")) != evidence:
            raise ReplayError(f"{path} records a different download")
        return
    pending = staging.parent / f"{staging.name}.publication"
    pending.write_text(json.dumps(evidence), encoding="utf-8")
    os.replace(pending, path)


def settle_replacement(root: Path, video_id: str) -> None:
    """Finish or roll back a replacement interrupted between its renames."""
    for replaced in root.glob(f".*_JST_youtube-{video_id}{REPLACED}"):
        directory = root / replaced.name[1 : -len(REPLACED)]
        if directory.exists():
            shutil.rmtree(replaced)
        else:
            # The verified staging stays; the next attempt reuses it for the replacement.
            replaced.rename(directory)


def download_verified(
    *, url: str, root: Path, ffmpeg_directory: Path, cookies_from_browser: str | None = None
) -> Path:
    """Download, verify and publish the original; replace a provisional one once processed."""
    common = _yt_dlp(cookies_from_browser)
    video_id = url.rsplit("=", 1)[-1]
    if url not in (
        f"https://www.youtube.com/watch?v={video_id}",
        f"https://youtube.com/watch?v={video_id}",
    ):
        raise ReplayError("Expected a canonical YouTube watch URL")
    source_stem(video_id, {"id": video_id, "timestamp": 1})
    require_mounted(root)
    if not root.is_dir():
        raise ReplayError(f"Originals root {root} does not exist")
    with (root / f".youtube-{video_id}.lock").open("a", encoding="utf-8") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        settle_replacement(root, video_id)
        existing = tuple(root.glob(f"*_JST_youtube-{video_id}"))
        if any((root / f"{video_id}.{extension}").exists() for extension in ("mp4", "mkv")) or (
            existing and not all(is_provisional(path) for path in existing)
        ):
            raise ReplayError(
                "Original already exists; verify or migrate it, do not download again"
            )
        metadata = _metadata(common, url)
        stem = source_stem(video_id, metadata)
        channel = channel_record(metadata)
        status, duration = expected_duration(metadata)
        provisional = status == PROCESSING
        directory = root / stem
        if existing and (provisional or existing != (directory,)):
            raise ReplayError("A provisional original waits for YouTube's processed archive")
        # yt-dlp resumes a failed attempt's .part fragments; a post_live archive's streams are
        # never reused once YouTube serves the processed archive under another status.
        staging = root / f".{stem}.{status}.partial"
        staging.mkdir(mode=0o700, exist_ok=True)
        target = str(staging / f"{stem}.%(ext)s")
        recorded = staging / "publication.json"
        if recorded.exists():
            # Verified before an interruption: re-probe and publish it, fetch nothing again.
            prior = record(json.loads(recorded.read_text(encoding="utf-8"))).get("audio_format")
            audio = prior if isinstance(prior, str) else None
        elif provisional:
            audio = fetch_provisional(
                common, ffmpeg_directory, target, url, audio_formats(metadata)
            )
        else:
            fetch(common, ffmpeg_directory, target, url, "bv*+ba/b")
            audio = None
        media = staging / f"{stem}.mp4"
        if {p.name for p in staging.iterdir()} - EVIDENCE != {media.name}:
            raise ReplayError(f"Unexpected files beside the download in {staging}")
        durations = stream_durations(ffmpeg_directory / "ffprobe", media)
        check_durations(durations, duration, provisional=provisional)
        data = record(metadata)
        write_evidence(
            staging,
            {
                **{key: data.get(key) for key in ("id", "release_timestamp", "timestamp")},
                "live_status": status,
                "duration": duration,
                "provisional": provisional,
                "streams": durations,
                **({"audio_format": audio} if audio is not None else {}),
            },
        )
        write_channel(staging, channel)
        if existing:
            # The verified processed archive replaces the provisional copy at the same path.
            replaced = root / f".{stem}{REPLACED}"
            directory.rename(replaced)
            staging.rename(directory)
            shutil.rmtree(replaced)
        else:
            staging.rename(directory)
        return directory


def main() -> None:
    if len(sys.argv) not in (4, 5):
        raise SystemExit(
            "Usage: python -m replay.youtube_original URL ROOT FFMPEG_DIRECTORY [BROWSER]"
        )
    try:
        print(
            download_verified(
                url=sys.argv[1],
                root=Path(sys.argv[2]),
                ffmpeg_directory=Path(sys.argv[3]),
                cookies_from_browser=sys.argv[4] if len(sys.argv) == 5 else None,
            )
        )
    except (ReplayError, OSError, ValueError, subprocess.SubprocessError) as error:
        raise SystemExit(str(error)) from error


if __name__ == "__main__":
    main()
