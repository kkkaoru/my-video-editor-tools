"""A natural compressor for repaired voice segments (computation only).

AR declipping restores crests the clipping flattened, and those crests can rise above the
original's level. This compressor brings them back gently:

- a look-ahead peak detector (``LOOK_AHEAD_SECONDS``) over the louder channel;
- a soft-knee gain computer (``KNEE_DB`` knee, ``RATIO``) around a threshold derived from the
  recording itself (``voice_repair_features``: its 99th-percentile 60 ms peak minus 6 dB);
- one-pole gain smoothing with ``ATTACK_SECONDS`` and ``RELEASE_SECONDS``, run at a 1 ms control
  rate (the only recursion; the gain is interpolated back to the samples);
- make-up gain so the segment's K-weighted loudness (ITU-R BS.1770, ungated: a segment is
  shorter than the gating blocks) matches the original within ``LOUDNESS_TOLERANCE_LU``;
- a safety limiter at ``CEILING_DBFS``, used only when a peak would pass it.
"""

from __future__ import annotations

import numpy as np
import numpy.typing as npt

Floats = npt.NDArray[np.float64]

LOOK_AHEAD_SECONDS = 0.005
KNEE_DB = 8.0
RATIO = 2.5
ATTACK_SECONDS = 0.008
RELEASE_SECONDS = 0.18
LIMIT_RATIO = 20.0
LIMIT_KNEE_DB = 2.0
LIMIT_ATTACK_SECONDS = 0.001
LIMIT_RELEASE_SECONDS = 0.08
CEILING_DBFS = -1.0
LOUDNESS_TOLERANCE_LU = 0.5
MAKEUP_ROUNDS = 4
CONTROL_RATE = 1000  # gain updates per second
EPSILON = 1e-12


def gain_computer(level_db: Floats, threshold: float, ratio: float, knee: float) -> Floats:
    """Soft-knee static gain (dB, at most 0) for each level."""
    over = level_db - threshold
    slope = 1 - 1 / ratio
    inside = -((over + knee / 2) ** 2) / (2 * knee) * slope
    return np.where(over <= -knee / 2, 0.0, np.where(over >= knee / 2, -over * slope, inside))


def smoothed(target: Floats, attack: float, release: float) -> Floats:
    """One-pole smoothing at the control rate: attack while the gain falls, release while it
    recovers."""
    rise = np.exp(-1 / (attack * CONTROL_RATE))
    fall = np.exp(-1 / (release * CONTROL_RATE))
    output = np.empty_like(target)
    value = 0.0
    for index, wanted in enumerate(target.tolist()):
        weight = rise if wanted < value else fall
        value = weight * value + (1 - weight) * wanted
        output[index] = value
    return output


def gain_curve(
    channels: Floats,
    rate: int,
    *,
    threshold: float,
    ratio: float,
    knee: float,
    times: tuple[float, float],
) -> Floats:
    """The smoothed gain (dB) per sample, computed at the control rate."""
    block = max(1, rate // CONTROL_RATE)
    level = np.abs(channels).max(axis=1)
    count = -(-len(level) // block)
    padded = np.pad(level, (0, count * block - len(level)))
    control = control_gain(
        padded.reshape(count, block).max(axis=1),
        threshold=threshold, ratio=ratio, knee=knee, times=times,
    )  # fmt: skip
    return np.interp(np.arange(len(level)), np.arange(count) * block + block / 2, control)


def control_gain(
    blocks: Floats, *, threshold: float, ratio: float, knee: float, times: tuple[float, float]
) -> Floats:
    """The smoothed gain (dB) of each control block (1 ms) from its peak, with look-ahead."""
    reach = max(1, int(LOOK_AHEAD_SECONDS * CONTROL_RATE))
    ahead = np.lib.stride_tricks.sliding_window_view(
        np.pad(blocks, (0, reach), mode="edge"), reach + 1
    ).max(axis=1)
    target = gain_computer(20 * np.log10(np.maximum(ahead, EPSILON)), threshold, ratio, knee)
    return smoothed(target, *times)


def k_weighting(rate: int, length: int) -> Floats:
    """The BS.1770 K-weighting power response on the real-FFT bins of ``length`` samples (shelf
    and high-pass biquads designed for ``rate``)."""
    frequency = np.exp(-2j * np.pi * np.fft.rfftfreq(length, 1 / rate) / rate)
    response = np.ones(len(frequency), dtype=np.complex128)
    for b, a in biquads(rate):
        response *= np.polyval(b[::-1], frequency) / np.polyval(a[::-1], frequency)
    return np.abs(response) ** 2


def biquads(rate: int) -> list[tuple[Floats, Floats]]:
    """BS.1770's two pre-filter stages for any sample rate (the published 48 kHz design)."""
    gain, quality, centre = 3.999843853973347, 0.7071752369554196, 1681.974450955533
    k = np.tan(np.pi * centre / rate)
    vh = 10 ** (gain / 20)
    vb = vh**0.4996667741545416
    a0 = 1 + k / quality + k * k
    shelf = (
        np.array([vh + vb * k / quality + k * k, 2 * (k * k - vh), vh - vb * k / quality + k * k])
        / a0,
        np.array([1.0, 2 * (k * k - 1) / a0, (1 - k / quality + k * k) / a0]),
    )
    quality, centre = 0.5003270373238773, 38.13547087602444
    k = np.tan(np.pi * centre / rate)
    a0 = 1 + k / quality + k * k
    highpass = (
        np.array([1.0, -2.0, 1.0]),
        np.array([1.0, 2 * (k * k - 1) / a0, (1 - k / quality + k * k) / a0]),
    )
    return [shelf, highpass]


def loudness(channels: Floats, rate: int) -> float:
    """Ungated BS.1770 loudness (LUFS) of the channels, by Parseval over the K-weighted power."""
    length = len(channels)
    spectrum = np.fft.rfft(channels, axis=0)
    weight = k_weighting(rate, length)
    # Parseval over the real spectrum: every bin but DC (and Nyquist, for even lengths) twice.
    twice = np.full(len(weight), 2.0)
    twice[0] = 1.0
    if length % 2 == 0:
        twice[-1] = 1.0
    power = (np.abs(spectrum) ** 2 * (weight * twice)[:, None]).sum(axis=0) / length**2
    mean_square = float(power.sum())  # both channels weigh 1.0 in BS.1770
    return -0.691 + 10 * float(np.log10(max(mean_square, EPSILON)))


def limited(channels: Floats, rate: int) -> Floats:
    """The safety limiter at ``CEILING_DBFS``, only where a peak would pass it, then one
    static trim for whatever its smoothing let through."""
    ceiling = 10 ** (CEILING_DBFS / 20)
    if np.abs(channels).max() <= ceiling:
        return channels
    limit = gain_curve(
        channels,
        rate,
        threshold=CEILING_DBFS - LIMIT_KNEE_DB,
        ratio=LIMIT_RATIO,
        knee=LIMIT_KNEE_DB,
        times=(LIMIT_ATTACK_SECONDS, LIMIT_RELEASE_SECONDS),
    )
    result = channels * (10 ** (limit / 20))[:, None]
    peak = float(np.abs(result).max())
    return result * (ceiling / peak) if peak > ceiling else result


def compressed(channels: Floats, reference: Floats, rate: int, threshold: float) -> Floats:
    """``channels`` compressed, made up to ``reference``'s loudness, and limited under the
    ceiling only where a peak would pass it."""
    gain = gain_curve(channels, rate, threshold=threshold, ratio=RATIO, knee=KNEE_DB,
                      times=(ATTACK_SECONDS, RELEASE_SECONDS))  # fmt: skip
    result = channels * (10 ** (gain / 20))[:, None]
    target = loudness(reference, rate)
    for _ in range(MAKEUP_ROUNDS):  # make up, limit, and make up again what the limiter took
        result = result * 10 ** ((target - loudness(result, rate)) / 20)
        result = limited(result, rate)
        if abs(loudness(result, rate) - target) <= LOUDNESS_TOLERANCE_LU / 2:
            break
    return result
