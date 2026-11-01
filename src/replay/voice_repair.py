"""Find and repair clipped or distorted ("robotic") speech in the separated voice.

The streamer's voice was sometimes clipped or codec-damaged at broadcast time. Separation does
not keep flat tops (its voice estimate is smooth), so each defect is measured where it is
visible. Every threshold pairs an absolute fixed value with a criterion relative to the
recording's own statistics; detection flags on either, the gate requires both.

Detection (``replay.voice_repair_features`` computes the frame features on the GPU and the
tables in Polars):

- clipping, on separation's own decode of the source (``delivery_audio.input_command``, sample
  aligned with the voice), decoded once: runs of at least ``CLIP_RUN`` samples at full scale
  (absolute), or at the recording's measured ceiling (relative: the lowest amplitude-histogram
  bin piling up ``PILE_Z`` robust deviations over a Theil-Sen fit of the tail, which can sit
  below full scale after the platform's processing), in frames where the voice is active;
- distortion, on the voice: a frozen spectrum or a noise-like collapse of a voiced frame, by an
  absolute floor or by robust z-scores against the recording's clean active frames.

Repair, computation only: every flagged segment gets AR interpolation of its clipped samples
(Janssen least squares, projected to stay consistent with the observation), then the natural
compressor (``replay.voice_compressor``: look-ahead soft knee, slow release, loudness make-up,
a safety limiter only when needed) over the declipped segment, with the threshold derived from
the recording's own short-term peaks. A segment is crossfaded in and kept only when its gate
passes both criteria:

- absolute: one metric improves by its fixed margin (flat tops by ``FLAT_TOP_MARGIN`` samples,
  harmonicity by ``HNR_MARGIN_DB``, flatness by ``FLATNESS_MARGIN``), harmonicity and flatness
  worsen at most by their tolerances, no new full-scale samples or peak above the AR repair's,
  crest loss and short-term loudness-range change within ``CREST_LOSS_DB`` and
  ``LOUDNESS_RANGE_DB``, and WavLM speaker similarity at least ``SIMILARITY_MIN``;
- relative: that improvement is also large for the segment itself or the recording (flat tops
  fall by ``FLAT_TOP_SHARE``; a harmonicity or flatness change beyond ``RELATIVE_T`` standard
  errors of the recording's frame spread), and neither metric worsens beyond the same.

Outside the kept segments the output is bit-identical to the input (verified after writing),
with the same sample count, rate and channels. When no segment is kept, no copy is written and
the receipt names the input as the voice. The receipt records every decision, a summary and
the wall time of each phase.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import time
import wave
from collections.abc import Callable, Iterator, Sequence
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Literal

import numpy as np
import numpy.typing as npt
import polars as pl

from replay import voice_compressor as compressor
from replay import voice_repair_features as analysis
from replay import voice_repair_models as models
from replay.core import ReplayError
from replay.delivery_audio import input_command
from replay.separation_window_pipeline import pcm_layout

Floats = npt.NDArray[np.float64]
Samples = npt.NDArray[np.float32]
Flags = npt.NDArray[np.bool_]
Runs = npt.NDArray[np.int64]
Memmap = np.memmap[tuple[int, ...], np.dtype[np.int16]]
Similarity = Callable[[Samples, Samples, int], float]

VERSION = 3
METHOD = "ar+compressor"
FULL_SCALE = 32767
HISTOGRAM_BINS = 32769
CANDIDATE_LEVEL = 16384  # samples at or above half scale are kept from the one decode pass
COARSE = 64  # levels per histogram bin of the ceiling search (0.2% of full scale)
TAIL_FROM = 0.25  # the fitted tail starts at this fraction of the loudest level
TAIL_MIN_BINS = 8
PILE_Z = 6.0
PILE_REGION = 0.9  # a pile-up counts only in the top 10% of the recording's own levels
CLIP_RUN = 3
FULL_SCALE_LEVEL = int(FULL_SCALE * 0.999)
MAD_SCALE = 1.4826
EPSILON = 1e-12
FADE_SECONDS = 0.01
CONTEXT_SECONDS = 0.5  # around a segment for its gate's similarity window and AR context
SIMILARITY_SECONDS = 1.0  # the shortest window an x-vector is compared over
AR_ORDER = 32
AR_CONTEXT = 3 * AR_ORDER
AR_WINDOW = 2048  # bounds each least-squares system
AR_ITERATIONS = 2
AR_HEADROOM = 2.0  # +6 dB over the observed peak: room for a restored crest, not for a blow-up
# The gate: absolute margins and tolerances, then the relative criteria.
FLAT_TOP_MARGIN = 8
HNR_MARGIN_DB = 0.3
FLATNESS_MARGIN = 0.05  # natural-log flatness
HNR_TOLERANCE_DB = 0.5
FLATNESS_TOLERANCE = 0.1
SIMILARITY_MIN = 0.86  # the speaker model card's same-speaker threshold
RANGE_WINDOW_SECONDS = 0.2  # short-term levels of the loudness range
CREST_LOSS_DB = 1.5  # a repair must not flatten the dynamics: crest and short-term range
LOUDNESS_RANGE_DB = 1.5
LOUDNESS_GATE_DB = 40.0
FLAT_TOP_SHARE = 0.25
RELATIVE_T = 2.0  # standard errors of the recording's own frame spread
BLOCK_SECONDS = 30
CHUNK_BYTES = 1 << 20


@dataclass(frozen=True, slots=True)
class Pcm:
    """A 16-bit PCM WAV's layout."""

    path: Path
    offset: int
    frames: int
    channels: int
    rate: int

    def samples(self, mode: Literal["r", "r+"] = "r") -> Memmap:
        return np.memmap(
            self.path,
            dtype=np.int16,  # little-endian on Apple silicon, as WAV is
            mode=mode,
            offset=self.offset,
            shape=(self.frames, self.channels),
        )


@dataclass(frozen=True, slots=True)
class Segment:
    """A flagged span of samples ``[start, end)``."""

    start: int
    end: int
    kinds: tuple[str, ...]
    clipped_samples: int
    longest_run: int
    severity: str


@dataclass(frozen=True, slots=True)
class Quality:
    """Reference-free measurements of a span's mid channel."""

    harmonicity_db: float
    flatness: float
    flat_tops: int
    full_scale: int
    peak: float
    crest_db: float
    loudness_range_db: float


@dataclass(frozen=True, slots=True)
class Measured:
    """A span's quality and the per-frame values its relative criteria compare."""

    quality: Quality
    harmonicity_db: Floats
    flatness: Floats


@dataclass(frozen=True, slots=True)
class Decision:
    segment: Segment
    method: str  # the candidate judged (``METHOD``)
    before: Quality
    after: Quality
    similarity: float
    kept: bool
    reason: str


@dataclass(frozen=True, slots=True)
class Detection:
    ceiling: int | None
    runs: Runs  # (start, length) of every clipped run
    statistics: dict[str, float]
    segments: tuple[Segment, ...]


@dataclass(frozen=True, slots=True)
class Gate:
    """The speaker check and the recording's frame spreads the relative criteria use."""

    similarity: Similarity
    spreads: tuple[float, float]


@dataclass(frozen=True, slots=True)
class Candidate:
    """A segment's context window, its original and its AR repair."""

    segment: Segment
    begin: int
    original: Floats
    repaired: Floats

    @property
    def span(self) -> slice:
        return slice(self.segment.start - self.begin, self.segment.end - self.begin)


def pcm(path: Path) -> Pcm:
    offset, frames = pcm_layout(path)
    with wave.open(str(path), "rb") as stream:
        if stream.getsampwidth() != 2:
            raise ReplayError("The voice repair reads 16-bit PCM only")
        channels, rate = stream.getnchannels(), stream.getframerate()
    # pcm_layout counts frames of two 16-bit channels; a mono file holds twice as many.
    return Pcm(path, offset, frames * 2 // channels, channels, rate)


def theil_sen(x: Floats, y: Floats) -> Floats:
    """The Theil-Sen line through the points (median pairwise slope), evaluated at ``x``."""
    first, second = np.triu_indices(len(x), k=1)
    slope = float(np.median((y[second] - y[first]) / (x[second] - x[first])))
    return slope * x + float(np.median(y - slope * x))


def ceiling(histogram: npt.NDArray[np.int64]) -> int | None:
    """The lowest level of an amplitude pile-up over the histogram's tail, if there is one."""
    levels = np.arange(len(histogram)) // COARSE
    counts = np.bincount(levels, weights=histogram.astype(np.float64))
    occupied = np.nonzero(counts)[0]
    if not len(occupied):
        return None
    top = int(occupied[-1])
    tail = np.arange(int(top * TAIL_FROM), top + 1)
    if np.count_nonzero(counts[tail]) < TAIL_MIN_BINS:
        return None
    logs = np.log1p(counts[tail])
    residual = logs - theil_sen(tail.astype(np.float64), logs)
    spread = MAD_SCALE * float(np.median(np.abs(residual - np.median(residual))))
    scores = residual / max(spread, EPSILON)
    piles = tail[(scores >= PILE_Z) & (tail >= top * PILE_REGION)]
    return int(piles[0]) * COARSE if len(piles) else None


def runs_of(flags: Flags) -> Runs:
    """(start, length) of every run of true values."""
    edges = np.diff(np.concatenate(([0], flags.astype(np.int8), [0])))
    starts, ends = np.nonzero(edges == 1)[0], np.nonzero(edges == -1)[0]
    return np.stack((starts, ends - starts), axis=1).astype(np.int64)


def sparse_runs(indices: npt.NDArray[np.int64], levels: npt.NDArray[np.int32], level: int) -> Runs:
    """Runs of at least ``CLIP_RUN`` consecutive samples at or above ``level``, from the sorted
    sample indices (and their levels) at or above ``CANDIDATE_LEVEL``."""
    chosen = indices[levels >= level]
    if not len(chosen):
        return np.zeros((0, 2), dtype=np.int64)
    breaks = np.nonzero(np.diff(chosen) != 1)[0] + 1
    starts = chosen[np.concatenate(([0], breaks))]
    lengths = np.diff(np.concatenate(([0], breaks, [len(chosen)])))
    found = np.stack((starts, lengths), axis=1).astype(np.int64)
    return found[found[:, 1] >= CLIP_RUN]


def decoded_blocks(
    command: Sequence[str], channels: int, frames: int
) -> Iterator[npt.NDArray[np.int16]]:
    """The decoded mix in blocks of ``frames`` frames (int16, ``channels`` wide)."""
    width = 2 * channels
    with subprocess.Popen(command, stdout=subprocess.PIPE) as process:
        stream = process.stdout
        if stream is None:
            raise ReplayError("The mix decoder has no output")
        while data := stream.read(frames * width):
            if len(data) % width:
                raise ReplayError("The decoded mix ended inside a frame")
            yield np.frombuffer(data, dtype="<i2").reshape(-1, channels)
    if process.returncode != 0:
        raise ReplayError(f"The mix decoder failed with status {process.returncode}")


@dataclass(frozen=True, slots=True)
class MixScan:
    """What one decode of the mix measured."""

    ceiling: int | None
    runs: Runs  # full-scale or ceiling runs
    plateaus: Runs  # near-equal runs at any amplitude
    frames: pl.DataFrame  # ``voice_repair_features.MIX_FEATURES`` per frame


def mix_clipping(command: Sequence[str], voice: Pcm) -> MixScan:
    """The mix's ceiling, clipped runs, plateau runs and frame features, from one decode: the
    histogram, every sample at or above half scale (a ceiling below that decodes the mix once
    more) and the per-frame waveform measurements."""
    block = voice.rate * BLOCK_SECONDS
    hop = voice.rate // analysis.FRAME_RATE
    histogram = np.zeros(HISTOGRAM_BINS, dtype=np.int64)
    indices, levels, mixes, plateaus, decoded = [], [], [], [], 0
    for samples in decoded_blocks(command, voice.channels, block):
        peaks = np.abs(samples.astype(np.int32)).max(axis=1)
        histogram += np.bincount(peaks, minlength=HISTOGRAM_BINS)
        loud = np.nonzero(peaks >= CANDIDATE_LEVEL)[0]
        indices.append(loud.astype(np.int64) + decoded)
        levels.append(peaks[loud])
        mid = mid_of(samples)
        mixes.append(analysis.mix_block(mid, hop))
        plateaus.append(analysis.plateau_runs(mid, hop, decoded))
        decoded += len(samples)
    if decoded != voice.frames:
        raise ReplayError("The decoded mix and the separated voice differ in length")
    level = ceiling(histogram)
    table = analysis.mix_table(mixes)
    found = np.concatenate(plateaus) if plateaus else np.zeros((0, 2), dtype=np.int64)
    if level is not None and level < CANDIDATE_LEVEL:
        return MixScan(level, low_ceiling_runs(command, voice, level), found, table)
    candidates = np.concatenate(indices) if indices else np.zeros(0, dtype=np.int64)
    loudness = np.concatenate(levels) if levels else np.zeros(0, dtype=np.int32)
    # Absolute (full scale) or relative (the measured ceiling): the lower level holds both.
    runs = sparse_runs(
        candidates, loudness.astype(np.int32), min(level or FULL_SCALE_LEVEL, FULL_SCALE_LEVEL)
    )
    return MixScan(level, runs, found, table)


def low_ceiling_runs(command: Sequence[str], voice: Pcm, level: int) -> Runs:
    """Clipped runs at a ceiling below ``CANDIDATE_LEVEL``, from a second decode."""
    parts, offset = [], 0
    for samples in decoded_blocks(command, voice.channels, voice.rate * BLOCK_SECONDS):
        found = runs_of(np.abs(samples.astype(np.int32)).max(axis=1) >= level)
        parts.append(found + np.array([offset, 0]))
        offset += len(samples)
    return joined_runs(parts)


def joined_runs(parts: list[Runs]) -> Runs:
    """Runs found block by block, with runs that continue across a block boundary joined, of
    at least ``CLIP_RUN`` samples."""
    joined: list[list[int]] = []
    for start, length in (row for part in parts for row in part.tolist()):
        if joined and joined[-1][0] + joined[-1][1] == start:
            joined[-1][1] += length
        else:
            joined.append([start, length])
    found = np.asarray(joined, dtype=np.int64).reshape(-1, 2)
    return found[found[:, 1] >= CLIP_RUN]


def mid_reader(data: Memmap) -> analysis.Reader:
    """The voice's mid channel over ``[start, stop)``, zero outside it."""

    def read(start: int, stop: int) -> Samples:
        output = np.zeros(stop - start, dtype=np.float32)
        low, high = max(0, start), min(len(data), stop)
        if low < high:
            output[low - start : high - start] = mid_of(np.asarray(data[low:high]))
        return output

    return read


def mid_of(samples: npt.NDArray[np.int16]) -> Samples:
    return np.asarray(samples.astype(np.float32).mean(axis=1) / (FULL_SCALE + 1), np.float32)


def segment_records(table: pl.DataFrame) -> tuple[Segment, ...]:
    return tuple(
        Segment(
            int(row["start"]), int(row["end"]),
            tuple(kind for kind in analysis.KINDS if row[kind.replace("-", "_")]),
            int(row["clipped_samples"]), int(row["longest_run"]), str(row["severity"]),
        )
        for row in table.iter_rows(named=True)
    )  # fmt: skip


def analysed(voice: Pcm, command: Sequence[str]) -> tuple[MixScan, pl.LazyFrame]:
    """The mix scan and the flagged frame table (every detector's columns)."""
    scan = mix_clipping(command, voice)
    hop = voice.rate // analysis.FRAME_RATE
    frames = analysis.features(mid_reader(voice.samples()), voice.frames, voice.rate)
    table = analysis.flagged(frames, scan.frames, analysis.runs_table(scan.runs), hop)
    return scan, table.collect().lazy()


def detect(voice: Pcm, command: Sequence[str]) -> Detection:
    scan, table = analysed(voice, command)
    hop = voice.rate // analysis.FRAME_RATE
    # AR interpolates full-scale and ceiling runs, and plateau runs where that detector fired.
    plateau_frames = table.filter(pl.col("plateau_runs")).select("frame").collect()["frame"]
    inside = np.isin(scan.plateaus[:, 0] // hop, plateau_frames.to_numpy())
    runs = merged_runs(np.concatenate((scan.runs, scan.plateaus[inside])))
    found = analysis.segments(
        table, analysis.runs_table(runs), hop=hop, length=voice.frames, rate=voice.rate
    )
    statistics = {
        **analysis.statistics(table),
        **{f"{name}Seconds": value for name, value in analysis.detector_seconds(table).items()},
    }
    return Detection(scan.ceiling, runs, statistics, segment_records(found))


def merged_runs(runs: Runs) -> Runs:
    """Overlapping or touching runs merged, sorted by start."""
    if not len(runs):
        return np.zeros((0, 2), dtype=np.int64)
    ordered = runs[np.argsort(runs[:, 0], kind="stable")]
    ends = ordered[:, 0] + ordered[:, 1]
    reach = np.maximum.accumulate(ends)
    breaks = np.concatenate(([True], ordered[1:, 0] > reach[:-1]))
    starts = ordered[breaks, 0]
    stops = np.maximum.reduceat(ends, np.nonzero(breaks)[0])
    return np.stack((starts, stops - starts), axis=1).astype(np.int64)


def ar_coefficients(signal: Floats) -> Floats:
    """The prediction-error filter ``[1, -a1, ..., -ap]`` of ``signal``'s autocorrelation."""
    correlation = np.correlate(signal, signal, "full")[len(signal) - 1 : len(signal) + AR_ORDER]
    if correlation[0] <= EPSILON:
        return np.concatenate(([1.0], np.zeros(AR_ORDER)))
    correlation[0] *= 1 + 1e-6  # a whisker of diagonal loading keeps the system definite
    lags = np.abs(np.subtract.outer(np.arange(AR_ORDER), np.arange(AR_ORDER)))
    return np.concatenate(([1.0], -np.linalg.solve(correlation[lags], correlation[1:])))


def interpolated(signal: Floats, missing: Flags) -> Floats:
    """Janssen AR interpolation of the ``missing`` samples, projected to stay consistent with
    what was observed: a sample keeps its sign and never shrinks below its observed magnitude.
    Long gaps make the predictor unstable: a sample beyond ``AR_HEADROOM`` times the window's
    peak is not trusted either, nor one that would reach full scale.

    The prediction error ``e[n] = sum_k a[k] x[n - k]`` is linear in the unknown samples: their
    columns are the filter placed at each unknown, and the known part is one convolution."""
    estimate = signal.copy()
    unknown = np.nonzero(missing)[0]
    if not len(unknown) or len(signal) <= 2 * AR_ORDER:
        return estimate
    bound = min(AR_HEADROOM * float(np.abs(signal).max()), FULL_SCALE_LEVEL / (FULL_SCALE + 1))
    rows = len(signal) - AR_ORDER
    lags = np.arange(AR_ORDER + 1)
    row_of = unknown[None, :] + lags[:, None] - AR_ORDER  # filter tap k reaches row u + k - p
    inside = (row_of >= 0) & (row_of < rows)
    columns = np.broadcast_to(np.arange(len(unknown))[None, :], row_of.shape)
    for _ in range(AR_ITERATIONS):
        error = ar_coefficients(estimate)
        matrix = np.zeros((rows, len(unknown)))
        matrix[row_of[inside], columns[inside]] = np.broadcast_to(error[:, None], row_of.shape)[
            inside
        ]
        known = np.where(missing, 0.0, estimate)
        solution = np.linalg.lstsq(matrix, -np.convolve(known, error, "valid"))[0]
        # Projected onto the observation: never smaller, same sign. A blow-up past the bound
        # keeps the observation (clipping it to the bound would draw a new flat top).
        observed = signal[unknown]
        projected = np.sign(observed) * np.maximum(np.sign(observed) * solution, np.abs(observed))
        estimate[unknown] = np.where(np.abs(solution) <= bound, projected, observed)
    return estimate


def ar_windows(missing: Flags) -> list[tuple[int, int]]:
    """Windows of ``AR_CONTEXT`` known samples around clusters of missing ones: runs closer than
    two contexts share a window (so no clipped sample is ever trusted as known), up to
    ``AR_WINDOW`` samples."""
    windows: list[list[int]] = []
    for start, size in runs_of(missing).tolist():
        low, high = max(0, start - AR_CONTEXT), min(len(missing), start + size + AR_CONTEXT)
        if windows and low <= windows[-1][1] and high - windows[-1][0] <= AR_WINDOW:
            windows[-1][1] = high
        else:
            windows.append([low, high])
    return [(low, high) for low, high in windows]


def ar_repaired(channels: Floats, runs: Runs, begin: int) -> Floats:
    """Each channel of a span with every clipped run (one sample wider each side) interpolated
    window by window (``ar_windows``)."""
    result = channels.copy()
    missing = np.zeros(len(channels), dtype=bool)
    for start, size in runs.tolist():
        missing[max(0, start - begin - 1) : max(0, start - begin + size + 1)] = True
    for low, high in ar_windows(missing):
        for channel in range(channels.shape[1]):
            result[low:high, channel] = interpolated(result[low:high, channel], missing[low:high])
    return result


def fade(length: int, rate: int) -> Floats:
    """1 inside the span, raised-cosine ramps of ``FADE_SECONDS`` at both ends."""
    ramp = min(int(FADE_SECONDS * rate), length // 2)
    weights = np.ones(length)
    if ramp:
        rise = 0.5 - 0.5 * np.cos(np.pi * (np.arange(ramp) + 0.5) / ramp)
        weights[:ramp], weights[length - ramp :] = rise, rise[::-1]
    return weights


def flat_tops(column: Floats) -> int:
    """Samples in runs of at least ``CLIP_RUN`` equal values in the top half of the peak."""
    loud = np.abs(column) >= np.abs(column).max(initial=0.0) / 2
    equal = np.concatenate(([False], np.diff(column) == 0)) & loud
    found = runs_of(equal)
    long = found[:, 1] >= CLIP_RUN - 1
    return int(found[long, 1].sum() + np.count_nonzero(long))


def measured(channels: Floats, rate: int, chosen: Flags | None = None) -> tuple[Measured, Flags]:
    """The span's measurements over ``chosen`` frames (by default its active ones, or all)."""
    mid = channels.mean(axis=1).astype(np.float32)
    frames = analysis.features(analysis.array_reader(mid), len(mid), rate)
    level = frames["level_db"].to_numpy()
    if chosen is None:
        floor = (
            np.percentile(level[level > analysis.SILENT_DB], 10)
            if (level > analysis.SILENT_DB).any()
            else 0.0
        )
        active = level > floor + analysis.VOICE_DB
        chosen = active if active.any() else np.ones(len(level), dtype=bool)
    harmonicity = np.clip(frames["harmonicity"].to_numpy()[chosen], 1e-4, 1 - 1e-4)
    hnr = 10 * np.log10(harmonicity / (1 - harmonicity))
    flatness = frames["flatness"].to_numpy()[chosen]
    scaled = np.round(channels * (FULL_SCALE + 1))
    peak = float(np.abs(mid).max(initial=0.0))
    rms = float(np.sqrt(np.mean(mid.astype(np.float64) ** 2))) if len(mid) else 0.0
    quality = Quality(
        harmonicity_db=float(np.mean(hnr)) if len(hnr) else 0.0,
        flatness=float(np.mean(flatness)) if len(flatness) else 0.0,
        flat_tops=sum(flat_tops(column) for column in scaled.T),
        full_scale=int(np.count_nonzero(np.abs(scaled) >= FULL_SCALE_LEVEL)),
        peak=peak,
        crest_db=float(20 * np.log10(max(peak, EPSILON) / max(rms, EPSILON))),
        loudness_range_db=loudness_range(mid.astype(np.float64), rate),
    )
    return Measured(quality, hnr, flatness), chosen


def loudness_range(mid: Floats, rate: int) -> float:
    """An LRA-style spread: the 95th minus the 10th percentile of the short-term level
    (``RANGE_WINDOW_SECONDS`` windows, hop half of that) over the audible part, in dB."""
    width, hop = (
        max(1, int(RANGE_WINDOW_SECONDS * rate)),
        max(1, int(RANGE_WINDOW_SECONDS * rate) // 2),
    )
    if len(mid) < width:
        return 0.0
    windows = np.lib.stride_tricks.sliding_window_view(mid, width)[::hop]
    level = 10 * np.log10(np.mean(windows**2, axis=1) + EPSILON)
    audible = level[level > level.max() - LOUDNESS_GATE_DB]
    return float(np.percentile(audible, 95) - np.percentile(audible, 10))


def standard_error(spread: float, frames: int) -> float:
    """The recording's frame spread of a metric as the standard error of a mean over
    ``frames`` frames."""
    return spread / float(np.sqrt(max(frames, 1)))


def verdict(
    before: Measured,
    after: Measured,
    similarity: float,
    spreads: tuple[float, float],
    ceiling: float | None = None,
) -> tuple[bool, str]:
    """Both criteria of the gate: absolute margins and tolerances, then the relative ones.

    ``spreads`` are the recording's robust frame spreads of harmonicity (dB) and flatness over
    its clean active frames: a change counts relatively only beyond ``RELATIVE_T`` standard
    errors of the segment's mean. ``ceiling`` is the highest peak a candidate may reach: the
    AR repair's own (which restores clipped crests within its headroom); by default the
    original's."""
    old, new = before.quality, after.quality
    frames = len(before.harmonicity_db)
    hnr_error = RELATIVE_T * standard_error(spreads[0], frames)
    flat_error = RELATIVE_T * standard_error(spreads[1], frames)
    hnr_gain = new.harmonicity_db - old.harmonicity_db
    flat_drop = old.flatness - new.flatness
    tops_drop = old.flat_tops - new.flat_tops
    improvements = (
        (tops_drop >= FLAT_TOP_MARGIN, tops_drop >= FLAT_TOP_SHARE * old.flat_tops),
        (hnr_gain >= HNR_MARGIN_DB, hnr_gain >= hnr_error),
        (flat_drop >= FLATNESS_MARGIN, flat_drop >= flat_error),
    )
    checks = (
        (any(absolute for absolute, _ in improvements),
         "no improvement above the absolute margins"),
        (any(absolute and relative for absolute, relative in improvements),
         "no improvement above the recording-relative criterion"),
        (hnr_gain >= -HNR_TOLERANCE_DB, "harmonicity fell"),
        (hnr_gain > -hnr_error, "harmonicity fell beyond the recording's spread"),
        (flat_drop >= -FLATNESS_TOLERANCE, "flatness rose"),
        (flat_drop > -flat_error, "flatness rose beyond the recording's spread"),
        (new.flat_tops <= old.flat_tops, "more flat tops"),
        (new.full_scale <= old.full_scale, "new full-scale samples"),
        (new.peak <= (old.peak if ceiling is None else ceiling) + 1 / (FULL_SCALE + 1),
         "a new peak above the ceiling"),
        (new.crest_db >= old.crest_db - CREST_LOSS_DB, "the crest factor collapsed"),
        (abs(new.loudness_range_db - old.loudness_range_db) <= LOUDNESS_RANGE_DB,
         "the short-term loudness range changed"),
        (similarity >= SIMILARITY_MIN, "speaker similarity below the threshold"),
    )  # fmt: skip
    failed = [reason for passed, reason in checks if not passed]
    return (not failed, failed[0] if failed else "improved")


def candidate(data: Memmap, found: Segment, runs: Runs, rate: int) -> Candidate:
    """The segment's context window and its AR repair."""
    reach = int(CONTEXT_SECONDS * rate)
    begin, end = max(0, found.start - reach), min(len(data), found.end + reach)
    original = np.asarray(data[begin:end], dtype=np.float64) / (FULL_SCALE + 1)
    inside = runs[(runs[:, 0] >= begin) & (runs[:, 0] + runs[:, 1] <= end)] if len(runs) else runs
    return Candidate(found, begin, original, ar_repaired(original, inside, begin))


def tried(
    item: Candidate, repaired: Floats, method: str, gate: Gate, rate: int
) -> tuple[Decision, npt.NDArray[np.int16]]:
    """One candidate's gate decision and its crossfaded int16 replacement of the span."""
    span = item.span
    weights = fade(span.stop - span.start, rate)[:, None]
    blended = item.original.copy()
    blended[span] = item.original[span] + weights * (repaired[span] - item.original[span])
    replacement = np.clip(np.round(blended * (FULL_SCALE + 1)), -FULL_SCALE - 1, FULL_SCALE)
    blended = replacement / (FULL_SCALE + 1)
    before, chosen = measured(item.original[span], rate)
    after, _ = measured(blended[span], rate, chosen)
    half = max(span.stop - span.start, int(SIMILARITY_SECONDS * rate)) // 2
    centre = (span.start + span.stop) // 2
    window = slice(max(0, centre - half), centre + half)
    score = gate.similarity(
        item.original[window].mean(axis=1).astype(np.float32),
        blended[window].mean(axis=1).astype(np.float32),
        rate,
    )
    ar_peak = float(np.abs(item.repaired[span].mean(axis=1)).max(initial=0.0))
    kept, reason = verdict(before, after, score, gate.spreads, max(before.quality.peak, ar_peak))
    decision = Decision(item.segment, method, before.quality, after.quality, score, kept, reason)
    return decision, replacement[span].astype(np.int16)


@dataclass(frozen=True, slots=True)
class Repair:
    decisions: list[Decision]
    replacements: list[npt.NDArray[np.int16]]
    timings: dict[str, float]


def spreads(statistics: dict[str, float]) -> tuple[float, float]:
    return statistics.get("spreadHarmonicityDb", 0.0), statistics.get("spreadFlatness", 0.0)


def judged(data: Memmap, found: Detection, similarity: Similarity, rate: int) -> Repair:
    """Every segment: AR, then the natural compressor over the declipped span, then the gate."""
    gate = Gate(similarity, spreads(found.statistics))
    threshold = found.statistics.get("compressorThresholdDb", 0.0)
    started = time.monotonic()
    items = [candidate(data, item, found.runs, rate) for item in found.segments]
    after_ar = time.monotonic()
    results = [tried(item, levelled_repair(item, threshold, rate), METHOD, gate, rate)
               for item in items]  # fmt: skip
    ended = time.monotonic()
    return Repair(
        decisions=[decision for decision, _ in results],
        replacements=[replacement for _, replacement in results],
        timings={
            "arSeconds": round(after_ar - started, 2),
            "compressorAndGateSeconds": round(ended - after_ar, 2),
        },
    )


def levelled_repair(item: Candidate, threshold: float, rate: int) -> Floats:
    """The AR repair with its segment compressed back toward the original's loudness."""
    span = item.span
    result = item.repaired.copy()
    result[span] = compressor.compressed(item.repaired[span], item.original[span], rate, threshold)
    return result


def differing_outside(voice: Pcm, output: Pcm, kept: Sequence[Segment]) -> int:
    """Samples that differ between the files outside the kept segments."""
    inside = np.zeros(voice.frames, dtype=bool)
    for found in kept:
        inside[found.start : found.end] = True
    first, second = voice.samples(), output.samples()
    block, differing = voice.rate * BLOCK_SECONDS, 0
    for start in range(0, voice.frames, block):
        changed = np.any(first[start : start + block] != second[start : start + block], axis=1)
        differing += int(np.count_nonzero(changed & ~inside[start : start + block]))
    return differing


def digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(CHUNK_BYTES):
            hasher.update(chunk)
    return hasher.hexdigest()


def write_json(path: Path, value: object) -> None:
    staged = path.with_name(f".{path.name}.partial")
    staged.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n", encoding="utf-8")
    os.replace(staged, path)


def written_voice(voice: Pcm, repair: Repair, output: Path) -> dict[str, int]:
    """Write the kept segments into a copy of the voice at ``output`` and verify it."""
    kept = [
        (decision, replacement)
        for decision, replacement in zip(repair.decisions, repair.replacements, strict=True)
        if decision.kept
    ]
    if not kept:
        return {}
    staged = output.with_name(f".{output.name}.partial")
    shutil.copyfile(voice.path, staged)
    target = pcm(staged).samples("r+")
    for decision, replacement in kept:
        target[decision.segment.start : decision.segment.end] = replacement
    target.flush()
    del target
    written = pcm(staged)
    if (written.frames, written.channels, written.rate) != (
        voice.frames,
        voice.channels,
        voice.rate,
    ):
        raise ReplayError("The repaired voice changed its length, channels or rate")
    differing = differing_outside(voice, written, [decision.segment for decision, _ in kept])
    if differing:
        raise ReplayError(f"The repaired voice differs outside its segments ({differing} samples)")
    os.replace(staged, output)
    return {"frames": voice.frames, "differingOutsideSegments": differing}


def decision_record(decision: Decision) -> dict[str, object]:
    return {
        **asdict(decision.segment),
        "kinds": list(decision.segment.kinds),
        "tried": decision.method,
        "before": asdict(decision.before),
        "after": asdict(decision.after),
        "similarity": round(decision.similarity, 6),
        "kept": decision.kept,
        "reason": decision.reason,
    }


def summary(
    records: list[dict[str, object]], rate: int, frames: int, repair: Repair
) -> dict[str, object]:
    """The receipt's summary: decisions by stage and reason, and the kept seconds."""
    table = pl.DataFrame(
        {
            "tried": [str(item["tried"]) for item in records],
            "kept": [bool(item["kept"]) for item in records],
            "reason": [str(item["reason"]) for item in records],
            "samples": [int(str(item["end"])) - int(str(item["start"])) for item in records],
        },
        schema={"tried": pl.String, "kept": pl.Boolean, "reason": pl.String, "samples": pl.Int64},
    ).lazy()
    by_stage = (
        table.group_by("tried", "kept", "reason")
        .agg(pl.len().alias("segments"), (pl.col("samples").sum() / rate).round(2).alias("seconds"))
        .sort("tried", "kept", "reason")
        .collect()
    )
    kept_seconds = (
        table.select(pl.col("samples").filter(pl.col("kept")).sum() / rate).collect().item()
    )
    return {
        "byStage": by_stage.to_dicts(),
        "keptSeconds": round(float(kept_seconds or 0.0), 2),
        "keptShare": round(float(kept_seconds or 0.0) / (frames / rate), 6) if frames else 0.0,
    }


def run(
    voice_path: Path,
    directory: Path,
    command: Sequence[str],
    similarity: Similarity,
    pins: dict[str, object],
) -> dict[str, object]:
    """Detect, repair and gate; the receipt (written last) names the voice consumers read."""
    receipt = directory / "receipt.json"
    if receipt.exists():
        return dict(json.loads(receipt.read_text(encoding="utf-8")))
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    started = time.monotonic()
    voice = pcm(voice_path)
    found = detect(voice, command)
    detected = time.monotonic()
    repair = judged(voice.samples(), found, similarity, voice.rate)
    output = directory / "vocals.wav"
    verification = written_voice(voice, repair, output)
    records = [decision_record(decision) for decision in repair.decisions]
    given = {"path": str(voice_path), "sha256": digest(voice_path)}
    result: dict[str, object] = {
        "version": VERSION,
        "input": given,
        "voice": {"path": str(output), "sha256": digest(output)} if verification else given,
        "repaired": bool(verification),
        "ceiling": found.ceiling,
        "clippedRuns": len(found.runs),
        "statistics": found.statistics,
        "summary": summary(records, voice.rate, voice.frames, repair),
        "segments": records,
        "kept": sum(decision.kept for decision in repair.decisions),
        "verification": verification,
        "timings": {
            "detectSeconds": round(detected - started, 2),
            **repair.timings,
            "totalSeconds": round(time.monotonic() - started, 2),
            "audioSeconds": round(voice.frames / voice.rate, 2),
        },
        "models": pins,
        "qualityApproved": False,
    }
    write_json(receipt, result)
    return result


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("voice", type=Path, help="the separated voice (16-bit PCM WAV)")
    parser.add_argument("directory", type=Path, help="where the repaired voice and receipt go")
    parser.add_argument("--source", type=Path, required=True, help="the source separation decoded")
    parser.add_argument("--gain-db", type=float, required=True, help="separation's input gain")
    parser.add_argument("--ffmpeg", required=True)
    parser.add_argument("--speaker", type=Path, required=True, help="the WavLM-SV directory")
    args = parser.parse_args(argv)
    decode = [
        *input_command(args.source, Path("pipe:1"), gain=args.gain_db, ffmpeg=args.ffmpeg)[:-1],
        "-f", "s16le", "pipe:1",
    ]  # fmt: skip
    pins: dict[str, object] = {
        "speaker": {"path": str(args.speaker), "files": models.verified_speaker(args.speaker)},
    }
    similarity = models.load_similarity(args.speaker)
    result = run(args.voice, args.directory, decode, similarity, pins)
    print(json.dumps({key: result[key] for key in ("repaired", "kept", "voice", "summary")}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
