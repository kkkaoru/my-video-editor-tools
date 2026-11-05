"""Take requested YouTube videos from their URLs to the recording queue, unattended.

``python -m replay.youtube_intake --queue Q.json --root ORIGINALS --ffmpeg-directory DIR URL...``

What runs side by side and what runs in turn (the queue and the pipeline own everything after the
download, ``docs/operations.md``):

- each video has its own thread, so one waiting for YouTube's processed archive holds up no other;
- at most ``DOWNLOADS`` downloads run at once: they wait on the network, not on this machine;
- a video is bound (``edit_completion init-youtube``) and queued only once its final original is
  on disk: a pipeline started on a provisional copy is stranded when the archive replaces it.
  ``--accept-provisional`` queues the copy YouTube serves now instead, and then nothing replaces
  it: that recording's edits stay bound to that copy.

A video without a ``video-config.json`` gets one from its measured frame: a portrait source keeps
its frame, a landscape one is cropped to 9:16 around the character. Write the file first to choose
otherwise: prepare copies it once. Every step is safe to repeat, so ``replay.volume_resume`` can
rerun the whole command.
"""

import argparse
import json
import subprocess
import sys
import threading
import time
from collections.abc import Callable, Sequence
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from replay import edit_completion, recording_queue, video_config, youtube_original
from replay.core import ReplayError
from replay.recordings import record

DOWNLOADS = threading.BoundedSemaphore(3)
RETRY_SECONDS = 900.0
# 15 minutes apart: a 1.6 h archive took about 16 h to leave post_live (2026-11-04).
ATTEMPTS = 96
FAILURES = (ReplayError, OSError, ValueError, subprocess.SubprocessError)
LAYOUT = {
    "canvas": [1080, 1920],
    "align": "centre",
    "originX": 0,
    "regionTop": 1300,
    "regionBottom": 1800,
    "regionRight": 1080,
}


def video_id_of(url: str) -> str:
    return url.rsplit("=", 1)[-1]


def final_original(root: Path, video_id: str, *, accept_provisional: bool = False) -> Path | None:
    """The video's verified original, unless it is still the provisional copy (when refused)."""
    for directory in root.glob(f"*_JST_youtube-{video_id}"):
        if accept_provisional or not youtube_original.is_provisional(directory):
            found = directory / f"{directory.name}.mp4"
            return found if found.is_file() else None
    return None


def wait_for_original(
    url: str,
    *,
    root: Path,
    ffmpeg_directory: Path,
    accept_provisional: bool = False,
    sleep: Callable[[float], None] = time.sleep,
) -> Path:
    """Download until the wanted original is there; a failed attempt resumes at the next one."""
    video_id = video_id_of(url)
    for attempt in range(ATTEMPTS):
        found = final_original(root, video_id, accept_provisional=accept_provisional)
        if found is not None:
            return found
        if attempt > 0:
            sleep(RETRY_SECONDS)
        try:
            with DOWNLOADS:
                youtube_original.download_verified(
                    url=url, root=root, ffmpeg_directory=ffmpeg_directory
                )
        except FAILURES as error:
            print(f"{video_id}: attempt {attempt + 1}: {error}", flush=True)
    found = final_original(root, video_id, accept_provisional=accept_provisional)
    if found is None:
        raise ReplayError(f"No final original of {video_id} after {ATTEMPTS} attempts")
    return found


def measured_config(original: Path, *, ffprobe: str = "ffprobe") -> dict[str, object]:
    """The default per-video settings for this original's frame and channel."""
    probe = subprocess.run(
        [ffprobe, "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height",
         "-of", "json", str(original)],
        capture_output=True, text=True, check=True,
    )  # fmt: skip
    stream = record(json.loads(probe.stdout)["streams"][0])
    portrait = int(str(stream["height"])) > int(str(stream["width"]))
    channel = json.loads((original.parent / "channel.json").read_text(encoding="utf-8"))
    return {
        "version": 1,
        "orientation": "source" if portrait else video_config.REFRAME,
        "captions": {"layout": LAYOUT},
        "cut": {"nonSpeech": True},
        "cleanup": {"sourceCaptions": True, "commentBoxes": False},
        "channelLabel": str(record(channel)["channel"]),
    }


def intake(
    url: str,
    *,
    root: Path,
    ffmpeg_directory: Path,
    queue: Path,
    checkout: Path,
    accept_provisional: bool = False,
    sleep: Callable[[float], None] = time.sleep,
) -> str:
    """One video: its original, its settings and edit configuration, its place in the queue."""
    original = wait_for_original(
        url,
        root=root,
        ffmpeg_directory=ffmpeg_directory,
        accept_provisional=accept_provisional,
        sleep=sleep,
    )
    recording = f"youtube-{video_id_of(url)}"
    edit_root = checkout / "edits" / recording
    edit_root.mkdir(mode=0o700, parents=True, exist_ok=True)
    settings = edit_root / "video-config.json"
    if not settings.exists():
        text = json.dumps(measured_config(original), ensure_ascii=False, indent=1)
        settings.write_text(text + "\n", encoding="utf-8")
    if not (edit_root / "edit-config.json").exists():
        edit_completion.create_youtube_config(
            edit_root,
            source=original,
            publication=original.parent / "publication.json",
            engine=checkout / "src/replay",
            executor_scope=checkout,
        )
    recording_queue.add(queue, recording)
    print(f"{recording}: queued with {original}", flush=True)
    return recording


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--queue", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--ffmpeg-directory", type=Path, required=True)
    parser.add_argument("--accept-provisional", action="store_true")
    parser.add_argument("urls", nargs="+")
    args = parser.parse_args(argv)
    checkout = Path.cwd().resolve()
    with ThreadPoolExecutor(max_workers=len(args.urls)) as pool:
        jobs = {
            url: pool.submit(
                intake,
                url,
                root=args.root,
                ffmpeg_directory=args.ffmpeg_directory,
                queue=args.queue,
                checkout=checkout,
                accept_provisional=args.accept_provisional,
            )
            for url in args.urls
        }
    failed = {url: job.exception() for url, job in jobs.items() if job.exception() is not None}
    for url, error in failed.items():
        print(f"{url}: {error}", file=sys.stderr)
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
