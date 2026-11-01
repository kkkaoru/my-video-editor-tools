"""The voice repair's analysis: per-frame features (numpy) and every detector as lazy Polars.

Frames are 20 ms (hop ``rate // 50``) with an FFT of the next power of two at least two hops
long and a Hann window. Features are computed ``BLOCK_FRAMES`` frames at a time, each block
starting one frame early so every frame's flux has its predecessor. On 10/06 the CPU took 3.9 s
per 1.5 h against MLX's 1.9 s (2026-10-29): not worth a GPU lease, so the analysis runs on the
CPU.

Voice (mid of the separated voice) columns: ``level_db``, ``flatness`` (log geometric over
arithmetic mean power in ``BAND_HZ``), ``flux`` (RMS change of the log band spectrum),
``harmonicity`` (normalized autocorrelation peak over ``PITCH_HZ`` lags) and its ``lag``,
``odd_even_db`` (harmonics 3, 5, 7 against 2, 4, 6 at the frame's pitch), ``band_holes`` (share
of band bins 40 dB below the band's median), ``crest_db`` and ``kurtosis``.

Mix (separation's decode of the source) columns, for clipping that no longer sits at full
scale after a gain change or a re-encode: ``plateau`` (samples in runs of ``PLATEAU_RUN``
near-equal samples, within ``PLATEAU_TOLERANCE`` of the frame's peak, in its top half),
``near_peak`` (share of samples within 1% of the frame's peak: a segment-local pile-up) and
``ringing`` (local maxima of the magnitude between 90% and 99% of the frame's peak: the
overshoot a lossy codec draws around a clipped edge).

Every detector flags active frames on an absolute fixed criterion or a criterion relative to the
recording's clean reference frames (robust z-scores for continuous features; a multiple of the
reference's 99th percentile for count features, which are mostly zero), held for
``MIN_FRAMES`` frames (``PITCH_FROZEN_FRAMES`` for a frozen pitch).
"""

from __future__ import annotations

from collections.abc import Callable

import numpy as np
import numpy.typing as npt
import polars as pl

Samples = npt.NDArray[np.float32]
Floats = npt.NDArray[np.float64]
Reader = Callable[[int, int], Samples]  # samples [start, stop), zero outside the signal

FRAME_RATE = 50
BLOCK_FRAMES = 6000  # two minutes of frames per block: bounded memory
BAND_HZ = (100.0, 8000.0)
PITCH_HZ = (60.0, 400.0)
EPSILON = 1e-12
SILENT_DB = 10 * np.log10(EPSILON) + 1
VOICE_DB = 20.0  # active voice: this far above the recording's 10th-percentile frame level
VOICED = 0.6  # harmonicity above which a frame's pitch and harmonics are measured
MAD_SCALE = 1.4826
ROBUST_Z = 4.0
# A count feature beyond 3x the reference's 99th percentile: relative only while under 1% of
# the clean frames show it (above that the fixed floor carries the detection).
COUNT_MULTIPLE = 3.0
COUNT_QUANTILE = 0.99
MIN_FRAMES = 3
CONTEXT_FRAMES = 4  # a collapse must lie between voiced frames this close on both sides
PITCH_FROZEN_FRAMES = 10  # 200 ms of exactly the same pitch lag while voiced
HOLE_DB = 40.0
PLATEAU_RUN = 5
# A smooth 140 Hz peak at 44.1 kHz stays within 0.2% for ~10 samples: only near-equal (int16
# steps of a held value) counts.
PLATEAU_TOLERANCE = 2e-4
NEAR_PEAK = 0.99
RING_BAND = (0.9, 0.99)
HARMONICS_ODD, HARMONICS_EVEN = (3, 5, 7), (2, 4, 6)
MERGE_FRAMES = 5
SHORT_TERM_FRAMES = 3  # a 60 ms short-term peak for the compressor's threshold
COMPRESSOR_HEADROOM_DB = 6.0  # its threshold: the 99th-percentile short-term peak minus this
PAD_SECONDS = 0.04
MILD_RUN_SECONDS = 0.0005
MILD_FRACTION = 0.005
VOICE_FEATURES = (
    "level_db", "flux", "flatness", "harmonicity", "lag", "odd_even_db", "band_holes",
    "crest_db", "kurtosis", "peak",
)  # fmt: skip
MIX_FEATURES = ("plateau", "near_peak", "ringing")


# Absolute fixed criteria by detector (see ``DETECTORS`` for each detector's relative one). Each
# sits beyond the clean 10-14b recording's own extremes (its 99.99th or 0.1st percentile over
# active frames, measured 2026-10-29): a separated vowel is nearly sinusoidal (crest 4-11 dB,
# kurtosis 1.5-5), and a mix with music rings near its peaks (up to 27 maxima a frame).
FIXED = {
    "frozen": pl.col("flux") < 0.05,
    "collapse": (pl.col("harmonicity") < 0.2) & (pl.col("flatness") > -1.0) & pl.col("in_voice"),
    "spectral_holes": pl.col("band_holes") >= 0.5,
    "crest": pl.col("crest_db") < 3.3,  # a sine is 3 dB
    "odd_harmonics": pl.col("voiced") & (pl.col("odd_even_db") > 15.0),
    "low_kurtosis": pl.col("kurtosis") < 1.0,  # a sine is 1.5, a square 1
    "plateau_runs": pl.col("plateau") >= 30,  # clean: 26 at the 99.99th percentile
    "pile_up": pl.col("near_peak") >= 0.15,  # a pure sine holds 9% within 1% of its peak
    "ringing": pl.col("ringing") >= 40,  # clean: 27 at the 99.99th percentile
}
# Each detector: its kind, and its relative criterion as (feature, direction, statistic).
DETECTORS = {
    "frozen": ("robotic", "log_flux", -1, "z"),
    "collapse": ("robotic", "harmonicity", -1, "z"),
    "spectral_holes": ("robotic", "band_holes", 1, "count"),
    "pitch_frozen": ("robotic", "", 0, ""),
    "crest": ("soft-clipping", "crest_db", -1, "z"),
    "odd_harmonics": ("soft-clipping", "odd_even_db", 1, "z"),
    "low_kurtosis": ("soft-clipping", "kurtosis", -1, "z"),
    "plateau_runs": ("clipping", "plateau", 1, "count"),
    "pile_up": ("clipping", "near_peak", 1, "count"),
    "ringing": ("clipping", "ringing", 1, "count"),
}
KINDS = ("clipping", "soft-clipping", "robotic")


def fft_size(hop: int) -> int:
    return 1 << (2 * hop - 1).bit_length()


def framed(samples: Samples, hop: int, size: int, count: int) -> Floats:
    """``count + 1`` overlapping frames of ``size`` samples at ``hop`` (a strided view)."""
    return np.lib.stride_tricks.sliding_window_view(samples.astype(np.float64), size)[::hop][
        : count + 1
    ]


def harmonic_ratio(power: Floats, lag: npt.NDArray[np.int64], size: int) -> Floats:
    """Odd over even harmonic power (dB) at each frame's pitch (``size / lag`` bins)."""
    fundamental = size / np.maximum(lag, 1)
    columns = np.arange(power.shape[1])

    def harmonics(orders: tuple[int, ...]) -> Floats:
        bins = np.rint(np.outer(fundamental, orders)).astype(np.int64)
        inside = np.minimum(bins, columns[-1])
        return np.take_along_axis(power, inside, axis=1).sum(axis=1)

    return 10 * np.log10(harmonics(HARMONICS_ODD) / harmonics(HARMONICS_EVEN))


def voice_block(samples: Samples, rate: int, count: int) -> dict[str, Floats]:
    """Voice features of ``count`` frames; ``samples`` starts one hop before the first frame and
    holds ``(count + 1) * hop + size`` samples."""
    hop, size = rate // FRAME_RATE, fft_size(rate // FRAME_RATE)
    frames = framed(samples, hop, size, count)
    own = frames[:, :hop]
    level = 10 * np.log10(np.mean(own**2, axis=1) + EPSILON)
    centred = own - own.mean(axis=1, keepdims=True)
    variance = np.mean(centred**2, axis=1)
    kurtosis = np.mean(centred**4, axis=1) / np.maximum(variance**2, EPSILON)
    crest = 20 * np.log10(
        np.abs(own).max(axis=1) / np.sqrt(np.mean(own**2, axis=1) + EPSILON) + EPSILON
    )
    power = np.abs(np.fft.rfft(frames * np.hanning(size), axis=1)) ** 2 + EPSILON
    low, high = (int(hz * size / rate) for hz in BAND_HZ)
    band = np.log(power[:, low:high])
    flatness = band.mean(axis=1) - np.log(power[:, low:high].mean(axis=1))
    flux = np.sqrt(np.mean((band[1:] - band[:-1]) ** 2, axis=1))
    holes = np.mean(
        band < np.median(band, axis=1, keepdims=True) - HOLE_DB * np.log(10) / 10, axis=1
    )
    correlation = np.fft.irfft(power, axis=1)
    shortest, longest = (int(rate / hz) for hz in reversed(PITCH_HZ))
    window = correlation[:, shortest:longest]
    lag = np.argmax(window, axis=1).astype(np.int64) + shortest
    peak = window.max(axis=1) / correlation[:, 0]
    columns = (level, flux, flatness, np.clip(peak, 0.0, 1.0), lag.astype(np.float64),
               harmonic_ratio(power, lag, size), holes, crest, kurtosis,
               np.abs(own).max(axis=1))  # fmt: skip
    return {
        name: column[1:] if name != "flux" else column
        for name, column in zip(VOICE_FEATURES, columns, strict=True)
    }


def mix_block(samples: Samples, hop: int) -> dict[str, Floats]:
    """Mix features of each whole frame of ``samples``."""
    count = len(samples) // hop
    frames = samples[: count * hop].astype(np.float64).reshape(count, hop)
    magnitude = np.abs(frames)
    peak = magnitude.max(axis=1, keepdims=True)
    loud = magnitude >= peak / 2
    flat = np.abs(np.diff(frames, axis=1, prepend=frames[:, :1] + 1)) <= PLATEAU_TOLERANCE * peak
    plateau = plateau_samples((flat & loud & (peak > EPSILON)).reshape(-1)).reshape(count, hop)
    rising = np.diff(magnitude, axis=1, prepend=magnitude[:, :1])
    falling = np.diff(magnitude, axis=1, append=magnitude[:, -1:])
    maxima = (rising > 0) & (falling < 0)
    band = (magnitude >= RING_BAND[0] * peak) & (magnitude < RING_BAND[1] * peak)
    return {
        "plateau": plateau.sum(axis=1).astype(np.float64),
        "near_peak": np.mean(magnitude >= NEAR_PEAK * peak, axis=1),
        "ringing": (maxima & band).sum(axis=1).astype(np.float64),
    }


def plateau_samples(flags: npt.NDArray[np.bool_]) -> npt.NDArray[np.bool_]:
    """The samples of runs of at least ``PLATEAU_RUN`` flags (each run's first sample included,
    as a near-equal step joins two samples)."""
    edges = np.diff(np.concatenate(([0], flags.astype(np.int8), [0])))
    starts, ends = np.nonzero(edges == 1)[0], np.nonzero(edges == -1)[0]
    keep = (ends - starts) >= PLATEAU_RUN - 1
    marks = np.zeros(len(flags) + 1, dtype=np.int64)
    np.add.at(marks, np.maximum(starts[keep] - 1, 0), 1)
    np.add.at(marks, ends[keep], -1)
    return np.cumsum(marks)[:-1] > 0


def plateau_runs(samples: Samples, hop: int, offset: int) -> npt.NDArray[np.int64]:
    """(start, length) of the plateau runs in ``samples`` (positions shifted by ``offset``)."""
    count = len(samples) // hop
    frames = samples[: count * hop].astype(np.float64).reshape(count, hop)
    peak = np.abs(frames).max(axis=1, keepdims=True)
    flat = np.abs(np.diff(frames, axis=1, prepend=frames[:, :1] + 1)) <= PLATEAU_TOLERANCE * peak
    flags = plateau_samples((flat & (np.abs(frames) >= peak / 2) & (peak > EPSILON)).reshape(-1))
    edges = np.diff(np.concatenate(([0], flags.astype(np.int8), [0])))
    starts, ends = np.nonzero(edges == 1)[0], np.nonzero(edges == -1)[0]
    return np.stack((starts + offset, ends - starts), axis=1).astype(np.int64)


def features(read: Reader, length: int, rate: int) -> pl.DataFrame:
    """The voice frame table (``frame`` and ``VOICE_FEATURES``) of every whole frame."""
    hop, size = rate // FRAME_RATE, fft_size(rate // FRAME_RATE)
    total = length // hop
    parts = []
    for first in range(0, total, BLOCK_FRAMES):
        count = min(BLOCK_FRAMES, total - first)
        start = (first - 1) * hop
        block = voice_block(read(start, start + (count + 1) * hop + size), rate, count)
        parts.append(pl.DataFrame(block))
    schema = dict.fromkeys(VOICE_FEATURES, pl.Float64)
    table = pl.concat(parts) if parts else pl.DataFrame(schema=schema)
    return table.with_row_index("frame").with_columns(pl.col("frame").cast(pl.Int64))


def mix_table(blocks: list[dict[str, Floats]]) -> pl.DataFrame:
    """The mix frame table from per-block ``mix_block`` columns."""
    schema = dict.fromkeys(MIX_FEATURES, pl.Float64)
    table = (
        pl.concat([pl.DataFrame(part) for part in blocks])
        if blocks
        else pl.DataFrame(schema=schema)
    )
    return table.with_row_index("frame").with_columns(pl.col("frame").cast(pl.Int64))


def array_reader(samples: Samples) -> Reader:
    """A reader over an in-memory signal, zero outside it."""

    def read(start: int, stop: int) -> Samples:
        output = np.zeros(stop - start, dtype=np.float32)
        low, high = max(0, start), min(len(samples), stop)
        if low < high:
            output[low - start : high - start] = samples[low:high]
        return output

    return read


def runs_table(runs: npt.NDArray[np.int64]) -> pl.DataFrame:
    return pl.DataFrame(
        {"start": runs[:, 0], "length": runs[:, 1]}, schema={"start": pl.Int64, "length": pl.Int64}
    )


def robust_z(name: str, reference: pl.Expr) -> pl.Expr:
    """``name`` as a robust z-score against the ``reference`` frames."""
    values = pl.col(name).filter(reference)
    middle = values.median()
    spread = (values - middle).abs().median() * MAD_SCALE
    return (pl.col(name) - middle) / pl.max_horizontal(spread, pl.lit(EPSILON))


def relative(name: str, reference: pl.Expr) -> pl.Expr:
    """``name``'s relative criterion against the recording's ``reference`` frames (a collapse
    only inside voiced context, like its fixed criterion)."""
    _, feature, direction, statistic = DETECTORS[name]
    if name == "collapse":
        harmonicity = robust_z("harmonicity", reference) <= -ROBUST_Z
        return harmonicity & (robust_z("flatness", reference) >= ROBUST_Z) & pl.col("in_voice")
    if statistic == "count":
        limit = pl.col(feature).filter(reference).quantile(COUNT_QUANTILE, "linear")
        return (pl.col(feature) > 0) & (pl.col(feature) > COUNT_MULTIPLE * limit)
    return robust_z(feature, reference) * direction >= ROBUST_Z


def held(flag: str, frames: int = MIN_FRAMES) -> pl.Expr:
    """``flag`` only in runs of at least ``frames`` frames."""
    return pl.col(flag) & (pl.len().over(pl.col(flag).rle_id()) >= frames)


def in_voice() -> pl.Expr:
    """Frames with a voiced frame within ``CONTEXT_FRAMES`` before and after them: a noise
    burst inside speech, not a fricative or a pause between syllables."""
    voiced = pl.col("voiced").cast(pl.Int8)
    before = voiced.rolling_max(CONTEXT_FRAMES).shift(1)
    after = voiced.rolling_max(CONTEXT_FRAMES).shift(-CONTEXT_FRAMES)
    return ((before == 1) & (after == 1)).fill_null(False)


def frozen_pitch() -> pl.Expr:
    """Voiced frames whose pitch lag stays exactly the same for ``PITCH_FROZEN_FRAMES``."""
    same = (pl.col("lag").diff().fill_null(1) == 0) & pl.col("voiced")
    return same & (pl.len().over(same.rle_id()) >= PITCH_FROZEN_FRAMES)


def flagged(frames: pl.DataFrame, mix: pl.DataFrame, runs: pl.DataFrame, hop: int) -> pl.LazyFrame:
    """The frame table with ``active``, ``clipped`` (full-scale or ceiling runs), one
    ``<detector>_fixed`` / ``<detector>_relative`` / ``<detector>`` triple per detector, and
    one flag per kind; ``reference`` is the clean frames the relative criteria compare with."""
    touched = (
        runs.lazy()
        .select(
            pl.int_ranges(
                pl.col("start") // hop, (pl.col("start") + pl.col("length") - 1) // hop + 1
            ).alias("frame")
        )
        .explode("frame")
        .unique()
        .with_columns(pl.lit(True).alias("touched"))
    )
    audible = pl.col("level_db") > SILENT_DB
    floor = pl.col("level_db").filter(audible).quantile(0.1, "linear")
    fixed_names = [f"{name}_fixed" for name in FIXED]
    table = (
        frames.lazy()
        .join(mix.lazy(), on="frame", how="left", maintain_order="left")
        .join(touched, on="frame", how="left", maintain_order="left")
        .with_columns(pl.col(*MIX_FEATURES).fill_null(0.0))
        .with_columns(
            (pl.col("level_db") > floor + VOICE_DB).fill_null(False).alias("active"),
            pl.col("flux").clip(EPSILON).log().alias("log_flux"),
            (pl.col("harmonicity") >= VOICED).alias("voiced"),
        )
        .with_columns(in_voice().alias("in_voice"))
        .with_columns(
            (pl.col("touched").fill_null(False) & pl.col("active")).alias("clipped"),
            *(expression.alias(f"{name}_fixed") for name, expression in FIXED.items()),
        )
        .with_columns(
            (pl.col("active") & ~pl.col("clipped") & ~pl.any_horizontal(*fixed_names)).alias(
                "reference"
            )
        )
    )
    reference = pl.col("reference")
    enough = reference.sum() >= MIN_FRAMES
    raw = [
        (pl.col("active") & enough & (pl.col(f"{name}_fixed") | pl.col(f"{name}_relative"))).alias(
            f"{name}_raw"
        )
        for name in FIXED
    ]
    table = table.with_columns(
        *(relative(name, reference).fill_null(False).alias(f"{name}_relative") for name in FIXED),
        (pl.col("active") & frozen_pitch()).alias("pitch_frozen"),
    ).with_columns(*raw)
    table = table.with_columns(*(held(f"{name}_raw").alias(name) for name in FIXED))
    by_kind = {
        kind: pl.any_horizontal(
            *(pl.col(name) for name, entry in DETECTORS.items() if entry[0] == kind)
        )
        for kind in KINDS
    }
    return table.with_columns(
        (by_kind["clipping"] | pl.col("clipped")).alias("clipping"),
        by_kind["soft-clipping"].alias("soft_clipping"),
        by_kind["robotic"].alias("robotic"),
    ).drop("touched", *(f"{name}_raw" for name in FIXED))


def detector_seconds(table: pl.LazyFrame) -> dict[str, float]:
    """Seconds each detector (and the full-scale or ceiling clipping) flagged."""
    names = ["clipped", *DETECTORS]
    row = table.select(*((pl.col(name).sum() / FRAME_RATE).alias(name) for name in names)).collect()
    return {name: float(value or 0.0) for name, value in row.row(0, named=True).items()}


def hnr_db() -> pl.Expr:
    """Harmonicity as a harmonics-to-noise ratio in dB."""
    clipped = pl.col("harmonicity").clip(1e-4, 1 - 1e-4)
    return 10 * (clipped / (1 - clipped)).log10()


def spread(values: pl.Expr) -> pl.Expr:
    """The robust spread (scaled MAD) of ``values``."""
    return (values - values.median()).abs().median() * MAD_SCALE


def statistics(table: pl.LazyFrame) -> dict[str, float]:
    """The recording's own reference statistics and flag counts."""
    reference = pl.col("reference")
    row = table.select(
        reference.sum().alias("referenceFrames"),
        pl.col("log_flux").filter(reference).median().alias("medianLogFlux"),
        pl.col("harmonicity").filter(reference).median().alias("medianHarmonicity"),
        pl.col("flatness").filter(reference).median().alias("medianFlatness"),
        pl.col("crest_db").filter(reference).median().alias("medianCrestDb"),
        (
            20
            * pl.col("peak")
            .rolling_max(SHORT_TERM_FRAMES)
            .filter(pl.col("active"))
            .quantile(0.99, "linear")
            .clip(EPSILON)
            .log10()
            - COMPRESSOR_HEADROOM_DB
        ).alias("compressorThresholdDb"),
        spread(hnr_db().filter(reference)).alias("spreadHarmonicityDb"),
        spread(pl.col("flatness").filter(reference)).alias("spreadFlatness"),
        pl.col("active").sum().alias("activeFrames"),
        pl.col("clipping").sum().alias("clippingFrames"),
        pl.col("soft_clipping").sum().alias("softClippingFrames"),
        pl.col("robotic").sum().alias("roboticFrames"),
    ).collect()
    return {name: float(value or 0.0) for name, value in row.row(0, named=True).items()}


def segments(
    table: pl.LazyFrame, runs: pl.DataFrame, *, hop: int, length: int, rate: int
) -> pl.DataFrame:
    """Padded, merged segments ``[start, end)`` over flagged frames, with their kinds, clipped
    sample counts, longest run and severity."""
    pad = int(PAD_SECONDS * rate)
    flag = pl.col("clipping") | pl.col("soft_clipping") | pl.col("robotic")
    spans = (
        table.filter(flag)
        .with_columns((pl.col("frame").diff().fill_null(1) != 1).cum_sum().alias("run"))
        .group_by("run")
        .agg(pl.col("frame").min().alias("first"), pl.col("frame").max().alias("last"))
        .select(
            (pl.col("first") * hop - pad).clip(0).alias("start"),
            ((pl.col("last") + 1) * hop + pad).clip(upper_bound=length).alias("end"),
        )
        .sort("start")
        .with_columns(
            (pl.col("start") > pl.col("end").cum_max().shift(1) + MERGE_FRAMES * hop)
            .fill_null(True)
            .cum_sum()
            .alias("segment")
        )
        .group_by("segment")
        .agg(pl.col("start").min(), pl.col("end").max())
        .sort("start")
    )
    clip_stats = (
        runs.lazy()
        .sort("start")
        .join_asof(
            spans.select("start", "end", "segment").rename({"start": "begin"}),
            left_on="start",
            right_on="begin",
            strategy="backward",
        )  # fmt: skip
        .filter(pl.col("start") < pl.col("end"))
        .group_by("segment")
        .agg(
            pl.col("length").sum().alias("clipped_samples"),
            pl.col("length").max().alias("longest_run"),
        )  # fmt: skip
    )
    kinds = (
        table.filter(flag)
        .with_columns((pl.col("frame") * hop).alias("sample"))
        .sort("sample")
        .join_asof(
            spans.select("start", "segment"),
            left_on="sample",
            right_on="start",
            strategy="backward",
        )  # fmt: skip
        .group_by("segment")
        .agg(pl.col("clipping", "soft_clipping", "robotic").any())
    )
    mild = (
        pl.col("clipping")
        & ~pl.col("soft_clipping")
        & ~pl.col("robotic")
        & (pl.col("longest_run") <= MILD_RUN_SECONDS * rate)
        & (pl.col("clipped_samples") <= MILD_FRACTION * (pl.col("end") - pl.col("start")))
    )
    return (
        spans.join(kinds, on="segment", how="left")
        .join(clip_stats, on="segment", how="left")
        .with_columns(pl.col("clipped_samples", "longest_run").fill_null(0))
        .with_columns(
            pl.when(mild).then(pl.lit("mild")).otherwise(pl.lit("severe")).alias("severity")
        )
        .sort("start")
        .select(
            "start",
            "end",
            "clipping",
            "soft_clipping",
            "robotic",
            "clipped_samples",
            "longest_run",
            "severity",
        )  # fmt: skip
        .collect()
    )
