"""Characters timed into silence start with the voice: Silero VAD on the delivered voice track.

``replay.speech_onsets`` times each character by CTC where its acoustics are strong and by the
recognizer elsewhere, and a recognizer's time can come before the sound. Whatever Silero hears as
silence with a voice starting at most ``REACH_SECONDS`` later moves up to that voice. Only later,
never earlier: a character inside a voice may be mid-sentence, where the voice's own start says
nothing about it. ``anchor`` stays, so a character keeps belonging to the caption paging built, and
a title's frames stay on the placement every gate compares; only when its text appears changes.

    python -m replay.caption_voice_onset ONSETS AUDIO FFMPEG BINARY BINARY-SHA256 MODEL
        MODEL-SHA256 WORK OUTPUT
"""

import argparse
import bisect
import json
import re
import subprocess
from collections.abc import Sequence
from pathlib import Path

from replay.core import ReplayError
from replay.recordings import record
from replay.stages import file_digest

VERSION = 1
REACH_SECONDS = 0.6  # further from the character's own time, a voice belongs to other words
CENTISECONDS_PER_SECOND = 100.0
SPAN = re.compile(r"^Speech segment \d+: start = ([\d.]+), end = ([\d.]+)$", re.MULTILINE)
RATE = "16000"  # the rate Silero reads


def parse_spans(output: str) -> list[tuple[float, float]]:
    """The voice spans ``whisper-vad-speech-segments`` prints, in seconds, in order."""
    return [
        (float(start) / CENTISECONDS_PER_SECOND, float(end) / CENTISECONDS_PER_SECOND)
        for start, end in SPAN.findall(output)
    ]


def voiced_from(at: float, spans: Sequence[tuple[float, float]]) -> float:
    """When a voice is next heard at or after ``at``: ``at`` itself inside a voice, else an onset
    no further than ``REACH_SECONDS`` away, else ``at`` when none is that near."""
    index = bisect.bisect_left([end for _, end in spans], at)
    if index == len(spans):
        return at
    onset = spans[index][0]
    return onset if at < onset <= at + REACH_SECONDS else at


def moved(
    tokens: Sequence[dict[str, object]], spans: Sequence[tuple[float, float]]
) -> list[dict[str, object]]:
    """Tokens starting with the voice, in order; a token's end never falls before its start."""
    result: list[dict[str, object]] = []
    for token in tokens:
        start = float(str(token["start"]))
        onset = voiced_from(start, spans)
        if onset == start:
            result.append(dict(token))
            continue
        result.append({**token, "start": onset, "end": max(onset, float(str(token["end"])))})
    return result


def write(onsets: Path, output_text: str, model_sha256: str, output: Path) -> dict[str, object]:
    """The onset document with its tokens moved, and a summary of what moved."""
    document = record(json.loads(onsets.read_text(encoding="utf-8")))
    tokens = document.get("tokens")
    if not isinstance(tokens, list):
        raise ReplayError(f"{onsets} has no tokens")
    spans = parse_spans(output_text)
    before = [record(token) for token in tokens]
    after = moved(before, spans)
    summary: dict[str, object] = {
        "version": VERSION, "method": "silero-vad-v6.2.0", "modelSha256": model_sha256,
        "reachSeconds": REACH_SECONDS, "voiceSpans": len(spans),
        "movedTokens": sum(
            old["start"] != new["start"] for old, new in zip(before, after, strict=True)
        ),
    }  # fmt: skip
    timed = {
        **document,
        "tokens": after,
        "parentOnsetsSha256": file_digest(onsets),
        "voiceOnset": summary,
    }
    staged = output.with_name(f".{output.name}.partial")
    staged.write_text(json.dumps(timed, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    staged.replace(output)
    return summary


def detect(audio: Path, ffmpeg: str, binary: Path, model: Path, work: Path) -> str:
    """What Silero hears on ``audio``, read from a 16 kHz mono copy removed afterwards."""
    work.mkdir(mode=0o700, parents=True, exist_ok=True)
    pcm = work / "voice-onset-mono.wav"
    try:
        subprocess.run(
            [ffmpeg, "-y", "-v", "error", "-i", str(audio), "-vn", "-ac", "1", "-ar", RATE,
             "-c:a", "pcm_s16le", str(pcm)],
            check=True,
        )  # fmt: skip
        done = subprocess.run(
            [str(binary), "-f", str(pcm), "-vm", str(model), "-np"],
            check=True, capture_output=True, text=True,
        )  # fmt: skip
    finally:
        pcm.unlink(missing_ok=True)
    return done.stdout


def verified(path: Path, sha256: str) -> Path:
    if file_digest(path) != sha256:
        raise ReplayError(f"{path} is not the pinned file")
    return path


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("onsets", "audio", "ffmpeg", "binary", "binary_sha256", "model", "model_sha256",
                 "work", "output"):  # fmt: skip
        parser.add_argument(name)
    args = parser.parse_args(argv)
    binary = verified(Path(args.binary), args.binary_sha256)
    model = verified(Path(args.model), args.model_sha256)
    heard = detect(Path(args.audio), args.ffmpeg, binary, model, Path(args.work))
    print(json.dumps(write(Path(args.onsets), heard, args.model_sha256, Path(args.output))))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
