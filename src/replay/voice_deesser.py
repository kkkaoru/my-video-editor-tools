"""A dynamic de-esser for the delivered voice (the user-confirmed sibilant fix of 2026-10-29).

The voice is split into a sibilant band (``BAND_HZ``, a linear-phase windowed-sinc FIR of
``TAPS`` taps) and its exact complement (the voice minus the band), so the two always sum back
to the voice. Only the band is turned down, and only while its 1 ms peak (the louder channel)
rises above the threshold: the whole recording's 90th-percentile band peak over its audible
milliseconds (within ``AUDIBLE_DB`` of the loudest) plus ``MARGIN_DB``, computed in Polars, and
never below the fixed ``MIN_THRESHOLD_DB``.
The gain has a soft knee (``KNEE_DB``, ``RATIO``), a ``ATTACK_SECONDS`` / ``RELEASE_SECONDS``
envelope at a 1 ms control rate (``voice_compressor``) and at most ``MAX_REDUCTION_DB``
(``--max-reduction-db``: the pipeline lowers it when the judge finds the voice re-coloured).

The recording is read twice in blocks (the band's peaks, then the output), so memory stays
bounded. Samples where the gain stays within ``PASS_DB`` of unity are copied unchanged, so the
output is bit-identical outside the reduced sibilants. The receipt (written last) names the
voice, the threshold and the reduced seconds.

``python -m replay.voice_deesser VOICE DIRECTORY`` writes ``DIRECTORY/vocals.wav`` and
``DIRECTORY/receipt.json``; an existing receipt is reused.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
from collections.abc import Sequence
from pathlib import Path

import numpy as np
import numpy.typing as npt
import polars as pl

from replay import voice_compressor as compressor
from replay import voice_repair as repair
from replay.core import ReplayError

Floats = npt.NDArray[np.float64]

VERSION = 1
BAND_HZ = (4000.0, 12000.0)
TAPS = 255
AUDIBLE_DB = 50.0
MARGIN_DB = 3.0
MIN_THRESHOLD_DB = -40.0  # fixed floor: a sibilant band quieter than this is never harsh
QUANTILE = 0.9
KNEE_DB = 6.0
RATIO = 4.0
ATTACK_SECONDS = 0.0015
RELEASE_SECONDS = 0.065
MAX_REDUCTION_DB = 8.0
PASS_DB = 0.01  # a gain this close to unity leaves the samples untouched
BLOCK_SECONDS = 30
FULL_SCALE = 32768


def band_filter(rate: int) -> Floats:
    """The sibilant band-pass: a Hann-windowed difference of two sincs, zero phase."""
    t = np.arange(TAPS) - TAPS // 2
    low, high = (2 * hz / rate for hz in BAND_HZ)
    return np.asarray((high * np.sinc(high * t) - low * np.sinc(low * t)) * np.hanning(TAPS))


def band(channels: Floats, taps: Floats) -> Floats:
    """``channels`` through the zero-phase band filter (FFT convolution, same length)."""
    size = 1 << (len(channels) + len(taps) - 2).bit_length()
    spectrum = np.fft.rfft(channels, n=size, axis=0) * np.fft.rfft(taps, n=size)[:, None]
    full = np.fft.irfft(spectrum, n=size, axis=0)
    return np.asarray(full[len(taps) // 2 : len(taps) // 2 + len(channels)])


def split(channels: Floats, rate: int) -> tuple[Floats, Floats]:
    """The complement and the band: they sum to ``channels``."""
    high = band(channels, band_filter(rate))
    return channels - high, high


def blocks_of(voice: repair.Pcm, step: int) -> list[tuple[int, int, int, int]]:
    """``(read_start, read_stop, keep_start, keep_stop)`` per block, with ``TAPS`` of overlap
    so the filter is exact at every block edge."""
    reach = TAPS
    return [
        (max(0, start - reach), min(voice.frames, start + step + reach), start,
         min(voice.frames, start + step))
        for start in range(0, voice.frames, step)
    ]  # fmt: skip


def peaks(voice: repair.Pcm) -> Floats:
    """The band's 1 ms peaks over the whole recording (louder channel)."""
    data, taps = voice.samples(), band_filter(voice.rate)
    block = voice.rate // compressor.CONTROL_RATE
    step = voice.rate * BLOCK_SECONDS // block * block
    parts = []
    for low, high, keep, stop in blocks_of(voice, step):
        signal = np.asarray(data[low:high], dtype=np.float64) / FULL_SCALE
        level = np.abs(band(signal, taps)[keep - low : stop - low]).max(axis=1)
        count = -(-len(level) // block)
        parts.append(np.pad(level, (0, count * block - len(level))).reshape(count, block).max(1))
    return np.concatenate(parts) if parts else np.zeros(0)


def threshold_db(band_peaks: Floats) -> float:
    """The whole recording's 90th-percentile audible band peak plus ``MARGIN_DB`` (Polars)."""
    table = pl.LazyFrame({"peak": band_peaks}).with_columns(
        (20 * pl.col("peak").clip(1e-9).log10()).alias("level")
    )
    audible = pl.col("level").filter(pl.col("level") > pl.col("level").max() - AUDIBLE_DB)
    value = table.select(audible.quantile(QUANTILE, "linear")).collect().item()
    if value is None:
        raise ReplayError("The voice has no audible sibilant band")
    return max(float(value) + MARGIN_DB, MIN_THRESHOLD_DB)


def control(band_peaks: Floats, threshold: float, depth: float = MAX_REDUCTION_DB) -> Floats:
    """The de-esser's gain (dB) per 1 ms, never below ``-depth``."""
    gain = compressor.control_gain(
        band_peaks, threshold=threshold, ratio=RATIO, knee=KNEE_DB,
        times=(ATTACK_SECONDS, RELEASE_SECONDS),
    )  # fmt: skip
    return np.maximum(gain, -depth)


def processed(signal: Floats, gain_db: Floats, rate: int) -> Floats:
    """``signal`` with its band scaled by ``gain_db`` (per sample)."""
    low, high = split(signal, rate)
    return np.asarray(low + high * (10 ** (gain_db / 20))[:, None])


def deessed(voice: repair.Pcm, output: Path, depth: float = MAX_REDUCTION_DB) -> dict[str, object]:
    """Write the de-essed voice to ``output``; the measurements for the receipt."""
    band_peaks = peaks(voice)
    threshold = threshold_db(band_peaks)
    gain = control(band_peaks, threshold, depth)
    block = voice.rate // compressor.CONTROL_RATE
    staged = output.with_name(f".{output.name}.partial")
    shutil.copyfile(voice.path, staged)
    target, data = repair.pcm(staged).samples("r+"), voice.samples()
    step = voice.rate * BLOCK_SECONDS // block * block
    changed = processed_samples = 0
    for low, high, keep, stop in blocks_of(voice, step):
        positions = np.arange(keep, stop)
        per_sample = np.interp(positions, np.arange(len(gain)) * block + block / 2, gain)
        reduced = per_sample < -PASS_DB
        if not reduced.any():
            continue
        signal = np.asarray(data[low:high], dtype=np.float64) / FULL_SCALE
        padded = np.zeros(high - low)
        padded[keep - low : stop - low] = per_sample
        result = processed(signal, padded, voice.rate)[keep - low : stop - low]
        values = np.clip(np.round(result * FULL_SCALE), -FULL_SCALE, FULL_SCALE - 1)
        block_out = np.asarray(data[keep:stop]).copy()
        block_out[reduced] = values[reduced].astype(np.int16)
        changed += int(np.count_nonzero((block_out != np.asarray(data[keep:stop])).any(axis=1)))
        processed_samples += int(np.count_nonzero(reduced))
        target[keep:stop] = block_out
    target.flush()
    del target
    written = repair.pcm(staged)
    if (written.frames, written.channels, written.rate) != (
        voice.frames,
        voice.channels,
        voice.rate,
    ):
        raise ReplayError("The de-essed voice changed its length, channels or rate")
    os.replace(staged, output)
    return {
        "thresholdDb": round(threshold, 2),
        "maxReductionDb": round(float(-gain.min(initial=0.0)), 2),
        "processedSeconds": round(processed_samples / voice.rate, 3),
        "changedSeconds": round(changed / voice.rate, 3),
        "changedShare": round(changed / voice.frames, 6) if voice.frames else 0.0,
    }


def run(voice_path: Path, directory: Path, depth: float = MAX_REDUCTION_DB) -> dict[str, object]:
    receipt = directory / "receipt.json"
    if receipt.exists():
        return dict(json.loads(receipt.read_text(encoding="utf-8")))
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    voice = repair.pcm(voice_path)
    output = directory / "vocals.wav"
    measured = deessed(voice, output, depth)
    result: dict[str, object] = {
        "version": VERSION,
        "input": {"path": str(voice_path), "sha256": repair.digest(voice_path)},
        "voice": {"path": str(output), "sha256": repair.digest(output)},
        **measured,
        "qualityApproved": False,
    }
    repair.write_json(receipt, result)
    return result


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("voice", type=Path, help="the voice (16-bit PCM WAV)")
    parser.add_argument("directory", type=Path, help="where the de-essed voice and receipt go")
    parser.add_argument("--max-reduction-db", type=float, default=MAX_REDUCTION_DB)
    args = parser.parse_args(argv)
    result = run(args.voice, args.directory, args.max_reduction_db)
    print(json.dumps({key: result[key] for key in ("voice", "thresholdDb", "changedSeconds")}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
