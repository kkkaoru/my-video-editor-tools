"""Audio-only redo of a finished delivery: restore the voice, re-mux the movie, keep the video.

A recording's ``voice-remux.json`` (beside ``pipeline.json``; a separate file, so no receipt goes
stale) names the restore components to apply to the delivered voice::

    {"version": 1, "components": ["deesser"], "tempo": "resample"}

``tempo`` (optional, default ``atempo``) picks the tempo render: ``atempo`` re-runs the paging
backend (``clocked_tempo_audio``), ``resample`` renders the paging slots by band-limited
resampling (``replay.voice_tempo``). ``components`` may be empty when ``tempo`` is given.

The ``voice-remux`` step (``replay.recording_caption_steps``) then restores the voice
(``python -m replay.voice_remux restore``), re-renders the tempo audio from the paging tempo
selection rebound to the restored voice, rejects it unless ``replay.voice_judge`` passes it
against the restored voice, encodes it under the
recording's loudness policy (``caption_compose.encode_audio``) and stream-copies the finished
movie's video with the new audio into a new movie (``remuxed``): same frames, never overwriting.
Without the file the step passes the finished movie through. Forcing ``voice-remux`` redoes it
and the publication steps after it, never the caption, cleanup or compose steps.

Components (each bit-identical where it changes nothing):

- ``deesser``: ``replay.voice_deesser``.

ML resynthesis (Sidon) is not a component, and its model was deleted: on 2026-10-31 the user
heard the spans it had replaced in a delivery (10/28, 461 windows) as unnaturally processed.
NISQA rose and speaker similarity was 0.96; ``replay.voice_judge``'s timbre shift now fails it.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
from collections.abc import Callable, Sequence
from pathlib import Path
from tempfile import TemporaryDirectory

from replay import voice_deesser
from replay import voice_repair as repair
from replay.caption_compose import MOOV_BYTES_PER_FRAME, MOOV_MARGIN
from replay.core import ReplayError
from replay.media import quicktime_video_tags
from replay.native_join import probe_video
from replay.recordings import record

SETTINGS = "voice-remux.json"
VERSION = 1


def deessed(source: Path, directory: Path) -> Path:
    return Path(str(record(voice_deesser.run(source, directory)["voice"])["path"]))


COMPONENTS: dict[str, Callable[[Path, Path], Path]] = {
    "deesser": deessed,
}
TEMPO = ("atempo", "resample")
KEYS = frozenset({"version", "components", "tempo"})


def settings(path: Path) -> tuple[list[str], str]:
    """The validated component list and tempo render of a ``voice-remux.json``."""
    data = record(json.loads(path.read_text(encoding="utf-8")))
    components, tempo = data.get("components"), data.get("tempo", "atempo")
    if data.get("version") != VERSION or not {"version", "components"} <= set(data) <= KEYS:
        raise ReplayError(f"{SETTINGS} takes version 1, components and an optional tempo")
    if tempo not in TEMPO:
        raise ReplayError(f"{SETTINGS}: tempo is one of {', '.join(TEMPO)}")
    if not isinstance(components, list) or not (components or "tempo" in data):
        raise ReplayError(f"{SETTINGS}: components is a list, non-empty without a tempo")
    unknown = [str(name) for name in components if name not in COMPONENTS]
    if unknown:
        raise ReplayError(f"{SETTINGS}: unknown components {', '.join(unknown)}")
    return [str(name) for name in components], str(tempo)


def restored(voice: Path, directory: Path, components: Sequence[str]) -> Path:
    """The voice after each component in order (each in its own subdirectory, reused)."""
    current = voice
    for index, name in enumerate(components):
        current = COMPONENTS[name](current, directory / f"{index:02d}-{name}")
    output = directory / "vocals.wav"
    if not output.exists():
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        staged = output.with_name(f".{output.name}.partial")
        shutil.copyfile(current, staged)  # a fixed name the tempo selection can pin
        os.replace(staged, output)
    if repair.pcm(output).frames != repair.pcm(voice).frames:
        raise ReplayError("The restored voice changed its length")
    return output


def remuxed(movie: Path, audio: Path, output: Path, *, ffmpeg: str) -> int:
    """Stream-copy ``movie``'s video with ``audio`` into a new ``output`` (same frames)."""
    if output.exists():
        raise ReplayError("Refusing to overwrite the remuxed movie")
    output.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    stream = probe_video(movie)
    with TemporaryDirectory(dir=output.parent, prefix=".remux-") as work:
        staged = Path(work) / output.name
        subprocess.run(
            [ffmpeg, "-nostdin", "-v", "error", "-n", "-i", str(movie), "-i", str(audio),
             "-map", "0:v:0", "-map", "1:a:0", "-c", "copy",
             "-video_track_timescale", str(stream.time_base.denominator),
             *quicktime_video_tags(movie),
             "-moov_size", str(MOOV_MARGIN + MOOV_BYTES_PER_FRAME * stream.packets),
             "-write_btrt", "0", str(staged)],
            check=True,
        )  # fmt: skip
        if probe_video(staged).packets != stream.packets:
            raise ReplayError("The remuxed movie does not hold the finished movie's frames")
        os.link(staged, output)
    return stream.packets


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    restore = commands.add_parser("restore", help="restore a voice with the settings' components")
    restore.add_argument("voice", type=Path)
    restore.add_argument("directory", type=Path)
    restore.add_argument("settings", type=Path)
    args = parser.parse_args(argv)
    output = restored(args.voice, args.directory, settings(args.settings)[0])
    print(json.dumps({"voice": str(output)}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
