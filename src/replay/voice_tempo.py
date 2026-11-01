"""Cut-slot tempo by band-limited resampling: the delivered timing without overlap-add.

The paging tempo stage (``replay.clocked_tempo_audio``) fits each kept source interval to its
video slot with FFmpeg ``atempo``. Its ratios are clock corrections within about 1e-5 of unity
(34 cents at most on the recordings measured), yet WSOLA re-cuts the waveform of every
stretched slot; ``replay.voice_judge`` measures that as a waveform-fidelity and click failure.
This backend renders the same slots (the paging receipt's ``cuts``: exact source and output
sample spans) by windowed-sinc interpolation instead: output sample ``j`` of a slot is the
voice at ``source_start + j * ratio``, a uniform retiming whose pitch change is below hearing
at these ratios. Unity slots are byte-copied.

``python -m replay.voice_tempo VOICE PLAN OUTPUT`` writes ``OUTPUT/candidate.wav`` and
``OUTPUT/receipt.json`` (its ``cuts`` keep the plan's spans); an existing receipt is reused.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import wave
from collections.abc import Sequence
from pathlib import Path

import numpy as np
import numpy.typing as npt

from replay import voice_repair as repair
from replay.core import ReplayError
from replay.recordings import record
from replay.stages import file_digest

Floats = npt.NDArray[np.float64]

VERSION = 1
HALF_TAPS = 32
KAISER_BETA = 9.0
CHUNK = 44_100  # output samples interpolated at once (bounds the tap matrix)
FULL_SCALE = 32768
MAX_WAVE_BYTES = 4 * 1024**3 - 64


def interpolated(source: Floats, positions: Floats, cutoff: float) -> Floats:
    """``source`` (frames x channels, with ``HALF_TAPS`` frames of context on both sides of
    ``positions``) at fractional ``positions``, through a Kaiser-windowed sinc low-passed at
    ``cutoff`` of Nyquist."""
    base = np.floor(positions).astype(np.int64)
    offsets = np.arange(-HALF_TAPS + 1, HALF_TAPS + 1)
    distance = offsets[None, :] - (positions - base)[:, None]
    window = np.i0(KAISER_BETA * np.sqrt(np.clip(1 - (distance / HALF_TAPS) ** 2, 0, None)))
    kernel = cutoff * np.sinc(cutoff * distance) * window / np.i0(KAISER_BETA)
    result: Floats = np.einsum("nk,nkc->nc", kernel, source[base[:, None] + offsets[None, :]])
    return result


def resampled(
    samples: npt.NDArray[np.int16] | Floats, start: float, ratio: float, frames: int
) -> Floats:
    """``frames`` output frames at source positions ``start + j * ratio`` (frames outside the
    source read as silence), read and interpolated ``CHUNK`` frames at a time."""
    pad = HALF_TAPS + 2
    result = np.empty((frames, samples.shape[1]))
    for first in range(0, frames, CHUNK):
        at = start + np.arange(first, min(frames, first + CHUNK)) * ratio
        low, high = int(np.floor(at[0])) - pad, int(np.floor(at[-1])) + pad
        block = np.asarray(samples[max(0, low) : max(0, min(len(samples), high))], np.float64)
        block = np.pad(block, ((max(0, -low), high - low - max(0, -low) - len(block)), (0, 0)))
        positions = np.asarray(at - low, np.float64)
        result[first : first + len(at)] = interpolated(block, positions, min(1.0, 1 / ratio))
    return result


def cut_spans(plan: dict[str, object]) -> list[tuple[int, int, int, int]]:
    """The plan's ``(source_start, source_end, output_start, output_end)`` slots, validated as
    contiguous on the output clock."""
    cuts = plan.get("cuts")
    if not isinstance(cuts, list) or not cuts:
        raise ReplayError("The tempo plan has no cuts")
    spans = []
    for cut in (record(item) for item in cuts):
        values = tuple(int(str(cut[key])) for key in
                       ("source_start_sample", "source_end_sample", "output_start_sample",
                        "output_end_sample"))  # fmt: skip
        spans.append((values[0], values[1], values[2], values[3]))
    for (_, _, _, end), (_, _, start, _) in zip(spans, spans[1:], strict=False):
        if start != end:
            raise ReplayError("The tempo plan's output slots are not contiguous")
    if any(low >= high or start >= end for low, high, start, end in spans):
        raise ReplayError("The tempo plan has an empty slot")
    return spans


def run(voice: Path, plan_path: Path, directory: Path) -> Path:
    """Render ``voice`` on the plan's slots into ``directory`` (receipt last; reused)."""
    receipt = directory / "receipt.json"
    if receipt.exists():
        return receipt
    plan = record(json.loads(plan_path.read_text(encoding="utf-8")))
    spans = cut_spans(plan)
    source = repair.pcm(voice)
    if int(str(plan["sampleRate"])) != source.rate or spans[-1][1] > source.frames:
        raise ReplayError("The voice does not match the tempo plan")
    frames = spans[-1][3] - spans[0][2]
    if frames * source.channels * 2 >= MAX_WAVE_BYTES:
        # shortcut: no >4 GiB placeholder WAV header, needed for recordings over ~6.7 h stereo
        raise ReplayError("The resampled tempo output would exceed a WAV's 4 GiB")
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    output, staged = directory / "candidate.wav", directory / ".candidate.wav.partial"
    samples, clipped, cuts = source.samples(), 0, []
    with wave.open(str(staged), "wb") as writer:
        writer.setnchannels(source.channels)
        writer.setsampwidth(2)
        writer.setframerate(source.rate)
        for low, high, start, end in spans:
            stretched = high - low != end - start
            if stretched:
                values = resampled(samples, low, (high - low) / (end - start), end - start).round()
                clipped += int(np.sum(np.abs(values) > FULL_SCALE - 1))
                chunk = np.clip(values, -FULL_SCALE, FULL_SCALE - 1).astype("<i2")
            else:
                chunk = np.asarray(samples[low:high], "<i2")
            writer.writeframesraw(chunk.tobytes())
            cuts.append({"source_start_sample": low, "source_end_sample": high,
                         "output_start_sample": start, "output_end_sample": end,
                         "resampled": stretched})  # fmt: skip
    os.replace(staged, output)
    staged_receipt = directory / ".receipt.json.partial"
    staged_receipt.write_text(json.dumps({
        "version": VERSION,
        "kind": "sinc-resample-exact-cut-slot-candidate",
        "voice": {"path": str(voice), "sha256": file_digest(voice)},
        "plan": {"path": str(plan_path), "sha256": file_digest(plan_path)},
        "output": {"path": str(output), "sha256": file_digest(output)},
        "sampleRate": source.rate,
        "channels": source.channels,
        "outputStartSample": spans[0][2],
        "outputEndSample": spans[-1][3],
        "clippedSamples": clipped,
        "algorithm": f"Kaiser-windowed sinc ({2 * HALF_TAPS} taps, beta {KAISER_BETA})",
        "cuts": cuts,
    }, indent=2) + "\n", encoding="utf-8")  # fmt: skip
    os.replace(staged_receipt, receipt)
    return receipt


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("voice", type=Path)
    parser.add_argument("plan", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args(argv)
    if args.output.exists() and not (args.output / "receipt.json").exists():
        shutil.rmtree(args.output)  # a render cut short
    print(json.dumps({"receipt": str(run(args.voice, args.plan, args.output))}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
