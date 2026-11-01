"""Noise-suppression flicker: the "robotic" voice of an over-tuned noise canceller, by computation.

A broadcast whose noise suppression is set too hard gates each 2-4 kHz band frame by frame, so
its log magnitude jumps between neighbouring 5 ms frames. The measure is the time-only nonlocal
Laplacian of the log-magnitude STFT, computed as a sliced FFT (Li, Meng, Guo & Zhang,
arXiv:2610.28372, Algorithm 1): planes are frames, each transformed over frequency (zero-padded to
a power of two, the circulant embedding), coupled across +-``HORIZON`` frames by
``Y_t = sum_s lambda_s U_{t+s}`` with the peridynamic ``1/d`` kernel. It equals the direct
neighbourhood sum at interior frames (``tests/test_voice_flicker.py``).

The verdict uses the suppression depth instead: the 10th percentile, over active frames and
4-7 kHz bins, of each bin's level below its own maximum within +-``DEPTH_FRAMES`` frames (25 ms).
An over-tuned canceller drives bins down to its gain floor between neighbouring frames, so the
dips are deeper. On the user's ear labels (2026-10-31) it separated every broadcast, also leaving
each one out (60 sampled windows; window AUC 0.89); over every audible window: less robotic 10/29
-21.5 and 10/21 -21.5 dB; robotic 10/28 -23.8, 10/27 (YouTube) -24.1, 10/17 -25.9 dB. The
flicker kept the order with a 0.11 margin and failed leave-one-out once: reported, not judged.

Runs on Metal (MLX): the framing, FFTs, sliced-FFT operator and moving maximum stay on the GPU;
only each window's few selected values come back. The audio is streamed from FFmpeg one window at
a time, so memory stays constant whatever the recording's length.

``python -m replay.voice_flicker AUDIO`` prints the medians over audible 6 s windows.
"""

from __future__ import annotations

import argparse
import functools
import importlib
import json
import subprocess
from collections.abc import Iterable, Iterator, Sequence

import numpy as np
import numpy.typing as npt

from replay import voice_judge
from replay.core import ReplayError

Floats = npt.NDArray[np.float64]

RATE = 16_000
FFT_SIZE = 512
HOP = 80  # 5 ms planes
HORIZON = 3  # frames each side
BAND_HZ = (2000.0, 4000.0)
DEPTH_BAND_HZ = (4000.0, 7000.0)
DEPTH_FRAMES = 5  # +-25 ms
DEPTH_PERCENTILE = 10.0
ACTIVE_DB = 40.0  # frames within this of the loudest are judged
SILENT_DB = -80.0  # a window whose loudest frame bin is below this has no voice to judge
WINDOW_SECONDS = 6.0
QUIET_RMS = 0.01
ROBOTIC_DEPTH_DB = -22.5
# A correction must land inside the less-robotic broadcasts' range (+-1 dB of -21.5): shallower
# means the dips were filled with made-up energy (mixed excitation, -17.8 dB, heard as the worst).
# Necessary, not sufficient: a gain-floor lift reached -21.7 dB, passed, and still sounded robotic
# (2026-10-31). The depth detects over-suppression; it does not certify a repair, which still
# needs an ear-approved sample.
NATURAL_DEPTH_DB = (-22.5, -20.5)
TIMBRE_LIMIT_DB = 1.0
CLICK_LIMIT_PER_SECOND = 0.3

mx = importlib.import_module("mlx.core")


def time_kernel(horizon: int = HORIZON) -> Floats:
    """Weights ``1/|s|`` for ``0 < |s| <= horizon`` and the self term making each row sum to 0."""
    offsets = np.arange(-horizon, horizon + 1)
    weights = np.where(offsets == 0, 0.0, 1.0 / np.maximum(np.abs(offsets), 1))
    weights[horizon] = -weights.sum()
    return np.asarray(weights, np.float64)


def _sliced(planes: object, weights: Floats, frames: int, bins: int) -> object:
    """The time-only nonlocal operator on GPU planes (frames x bins), by the sliced FFT."""
    horizon = len(weights) // 2
    size = 1 << int(np.ceil(np.log2(bins)))
    spectra = mx.fft.fft(planes, n=size, axis=1)
    result = mx.zeros_like(spectra)
    for offset in range(-horizon, horizon + 1):
        # lambda_s is the weight itself: a time-only kernel has no frequency coupling.
        lower, upper = max(0, -offset), frames - max(0, offset)
        shifted = spectra[lower + offset : upper + offset] * float(weights[offset + horizon])
        result[lower:upper] = result[lower:upper] + shifted
    return mx.real(mx.fft.ifft(result, axis=1))[:, :bins]


def sliced_apply(planes: Floats, weights: Floats) -> Floats:
    """``_sliced`` for host planes (the direct-sum check in the tests)."""
    frames, bins = planes.shape
    return np.asarray(
        _sliced(mx.array(planes.astype(np.float32)), weights, frames, bins), np.float64
    )


def band(lower_upper: tuple[float, float]) -> npt.NDArray[np.bool_]:
    freqs = np.fft.rfftfreq(FFT_SIZE, 1 / RATE)
    return np.asarray((freqs >= lower_upper[0]) & (freqs < lower_upper[1]))


@functools.cache
def frame_index(count: int) -> object:
    """The GPU index matrix of ``count`` overlapping frames (built once per window length)."""
    return mx.array(np.arange(FFT_SIZE)[None, :] + HOP * np.arange(count)[:, None])


def measures(signal: Floats) -> tuple[float, float] | None:
    """(suppression depth dB, flicker dB) of one window; None when silent or short."""
    count = (len(signal) - FFT_SIZE) // HOP + 1
    if count < 2 * HORIZON + 2:
        return None
    window = mx.array(np.hanning(FFT_SIZE).astype(np.float32))
    frames = mx.take(mx.array(signal.astype(np.float32)), frame_index(count)) * window
    level = 20 * mx.log10(mx.abs(mx.fft.rfft(frames, axis=1)) + 1e-6)
    padded = mx.concatenate([mx.repeat(level[:1], DEPTH_FRAMES, axis=0), level,
                             mx.repeat(level[-1:], DEPTH_FRAMES, axis=0)])  # fmt: skip
    peak = padded[:count]
    for shift in range(1, 2 * DEPTH_FRAMES + 1):
        peak = mx.maximum(peak, padded[shift : shift + count])
    laplacian = mx.abs(_sliced(level, time_kernel(), count, FFT_SIZE // 2 + 1))
    host = np.asarray(level)
    if host.max() < SILENT_DB:
        return None
    active = host.max(axis=1) > host.max() - ACTIVE_DB
    below = np.asarray(level - peak)[active][:, band(DEPTH_BAND_HZ)]
    inner = active[HORIZON:-HORIZON]
    lap = np.asarray(laplacian)[HORIZON:-HORIZON][inner][:, band(BAND_HZ)]
    return float(np.percentile(below, DEPTH_PERCENTILE)), float(np.median(lap))


def windows(path: str, ffmpeg: str = "ffmpeg") -> Iterator[Floats]:
    """Mono 16 kHz ``WINDOW_SECONDS`` windows of an audio or movie file, streamed from FFmpeg."""
    size = round(WINDOW_SECONDS * RATE) * 2
    with subprocess.Popen(
        [ffmpeg, "-nostdin", "-v", "error", "-i", path, "-vn", "-ac", "1", "-ar", str(RATE),
         "-f", "s16le", "-"],
        stdout=subprocess.PIPE,
    ) as process:  # fmt: skip
        stream = process.stdout
        if stream is None:
            raise ReplayError("FFmpeg opened no output pipe")
        while len(chunk := stream.read(size)) == size:
            yield np.frombuffer(chunk, "<i2").astype(np.float64) / 32768
    if process.returncode != 0:
        raise subprocess.CalledProcessError(process.returncode, ffmpeg)


def recording_report(parts: Iterable[Floats]) -> dict[str, object]:
    """Medians over audible windows and the robotic verdict (by the depth)."""
    values = [
        found
        for part in parts
        if np.sqrt(np.mean(part**2)) >= QUIET_RMS and (found := measures(part)) is not None
    ]
    depth = float(np.median([value[0] for value in values])) if values else None
    return {
        "windows": len(values),
        "suppressionDepthDb": depth,
        "flicker": float(np.median([value[1] for value in values])) if values else None,
        "robotic": depth is not None and depth < ROBOTIC_DEPTH_DB,
    }


def correction_failures(original: Floats, corrected: Floats) -> list[str]:
    """Why ``corrected`` (16 kHz, same length) is not an acceptable fix of ``original``: its
    suppression depth outside ``NATURAL_DEPTH_DB``, a timbre shift or new clicks."""
    found = measures(corrected)
    depth = found[0] if found is not None else None
    seconds = len(corrected) / RATE
    checks = (
        ("depth", depth is None or not NATURAL_DEPTH_DB[0] <= depth <= NATURAL_DEPTH_DB[1]),
        ("timbre", voice_judge.timbre_shift(original, corrected) > TIMBRE_LIMIT_DB),
        ("clicks", voice_judge.new_clicks(original, corrected, RATE) / seconds
         > CLICK_LIMIT_PER_SECOND),
    )  # fmt: skip
    return [name for name, failed in checks if failed]


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("audio")
    args = parser.parse_args(argv)
    print(json.dumps(recording_report(windows(args.audio))))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
