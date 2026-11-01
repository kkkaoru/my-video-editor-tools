"""Fill spectral holes (codec band drop-outs) in the voice by computation alone.

A short-time spectrum (Hann window of ``WINDOW_SECONDS``, 75% overlap) marks a hole where a
time-frequency bin lies ``HOLE_DB`` below the median of its time neighbours (``TIME_REACH``
frames each side, itself excluded) and of its frequency neighbours (``FREQUENCY_REACH`` bins each
side), or ``CARVED_DB`` below its frame's band median (a region a separation mask or a codec
zeroed), or ``DROP_DB`` below the same bin's median over the active frames (a band that drops
out for longer than the neighbours reach), in an active frame, inside ``BAND_HZ``, in a run of
at least ``MIN_BAND_HZ`` of adjacent bins (fixed criterion). A frame's hole fraction must also
stand out against the excerpt's own frames (relative criterion: a robust z of at least
``ROBUST_Z``, or no frame flags).

A hole's log magnitude is interpolated across time from the nearest unholed frames of the same
bin (within ``FILL_REACH``), else across frequency from the frame's unholed bins. Its phase
advances from the previous frame by the bin's expected phase step (a phase vocoder), and
``CONSISTENCY_ITERATIONS`` Griffin-Lim iterations refine the phase of the hole bins only; every
other bin keeps its magnitude and phase. The excerpt is resynthesized by weighted overlap-add,
and ``filled`` returns it crossfaded into the original only over the segment, so the output is
bit-identical elsewhere.

Version 2 (2026-10-29) targets separation-mask gaps: whole frames gated out between speech
frames are holes too, a fill is levelled to the neighbouring frames' band envelope (smoothed over
``ENVELOPE_BINS``), noisy frames get noise phase (PNS-like) instead of the vocoder's, and the
fill covers the whole holed stretch around the segment.

Not implemented: LPC or cepstral envelope shaping and the spectral-folding extension of a
missing top band.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np
import numpy.typing as npt
import polars as pl

Floats = npt.NDArray[np.float64]
Flags = npt.NDArray[np.bool_]
Complex = npt.NDArray[np.complex128]

WINDOW_SECONDS = 0.021
OVERLAP = 4  # 75% overlap
BAND_HZ = (100.0, 8000.0)
HOLE_DB = 25.0
DROPOUT_DB = 40.0  # a whole frame this far below its speech neighbours on both sides
DROP_DB = 30.0  # below the bin's own median over the excerpt's active frames
CARVED_DB = 40.0  # the voice detectors' band-hole depth below the frame's band median
TIME_REACH = 3
FREQUENCY_REACH = 4
MIN_BAND_HZ = 100.0
ACTIVE_DB = 40.0  # an active frame: within this of the excerpt's loudest frame
ROBUST_Z = 4.0
MAD_SCALE = 1.4826
FILL_REACH = 8
CONSISTENCY_ITERATIONS = 3
FADE_SECONDS = 0.01
EPSILON = 1e-10
ENVELOPE_BINS = 5
NOISY_FLATNESS = -3.0  # natural-log flatness of a frame's unholed bins
NOISE_SEED = 20261029
STRETCH_GAP = 4


@dataclass(frozen=True, slots=True)
class Holes:
    """Hole flags per (frame, bin) and each frame's hole fraction in the band."""

    flags: Flags
    fraction: Floats


def size_of(rate: int) -> int:
    return 1 << (int(WINDOW_SECONDS * rate) - 1).bit_length()


def stft(signal: Floats, rate: int) -> Complex:
    size = size_of(rate)
    hop = size // OVERLAP
    padded = np.pad(signal, (size, size + hop))
    frames = np.lib.stride_tricks.sliding_window_view(padded, size)[::hop]
    return np.fft.rfft(frames * np.hanning(size), axis=1)


def istft(spectrum: Complex, rate: int, length: int) -> Floats:
    """Weighted overlap-add (Hann analysis and synthesis, normalized by the window power)."""
    size = size_of(rate)
    hop = size // OVERLAP
    window = np.hanning(size)
    frames = np.fft.irfft(spectrum, n=size, axis=1) * window
    total = (len(frames) - 1) * hop + size
    output, weight = np.zeros(total), np.zeros(total)
    starts = np.arange(len(frames)) * hop
    index = starts[:, None] + np.arange(size)[None, :]
    np.add.at(output, index, frames)
    np.add.at(weight, index, np.broadcast_to(window**2, frames.shape))
    return (output / np.maximum(weight, EPSILON))[size : size + length]


def neighbour_median(level: Floats, axis: int, reach: int) -> Floats:
    """The median of each value's neighbours along ``axis`` (itself excluded, edges reflected)."""
    pad = [(0, 0), (0, 0)]
    pad[axis] = (reach, reach)
    padded = np.pad(level, pad, mode="reflect")
    windows = np.lib.stride_tricks.sliding_window_view(padded, 2 * reach + 1, axis=axis)
    others = np.delete(windows, reach, axis=-1)
    return np.median(others, axis=-1)


def holes(spectrum: Complex, rate: int) -> Holes:
    """The holes of a spectrum (fixed criterion) in frames that also pass the relative one."""
    size = size_of(rate)
    level = 20 * np.log10(np.abs(spectrum) + EPSILON)
    frequencies = np.fft.rfftfreq(size, 1 / rate)
    band = (frequencies >= BAND_HZ[0]) & (frequencies < BAND_HZ[1])
    loudness = level.max(axis=1)
    active = loudness >= loudness.max() - ACTIVE_DB
    isolated = (level < neighbour_median(level, 0, TIME_REACH) - HOLE_DB) & (
        level < neighbour_median(level, 1, FREQUENCY_REACH) - HOLE_DB
    )
    median = np.median(np.where(band[None, :], level, np.nan)[:, band], axis=1)
    carved = level < median[:, None] - CARVED_DB  # a mask or codec zeroed the region
    typical = np.median(np.where(active[:, None], level, np.nan)[active], axis=0)
    dropped = level < typical[None, :] - DROP_DB  # a band dropping out for a while
    deep = isolated | carved | dropped
    gap = dropout_frames(loudness, active)
    candidate = (deep & band[None, :] & active[:, None]) | (gap[:, None] & band[None, :])
    active = active | gap
    wide = wide_runs(candidate, max(1, int(np.ceil(MIN_BAND_HZ * size / rate))))
    fraction = wide[:, band].mean(axis=1)
    standing = relative_frames(fraction, active)
    return Holes(wide & standing[:, None], np.where(standing, fraction, 0.0))


def dropout_frames(loudness: Floats, active: Flags) -> Flags:
    """Frames gated out between speech: ``DROPOUT_DB`` below the loudest active frame within
    ``TIME_REACH`` frames on each side (a separation mask closing for a moment)."""
    padded = np.pad(np.where(active, loudness, -np.inf), TIME_REACH, constant_values=-np.inf)
    windows = np.lib.stride_tricks.sliding_window_view(padded, TIME_REACH)
    before = windows[: len(loudness)].max(axis=1)
    after = windows[TIME_REACH + 1 : TIME_REACH + 1 + len(loudness)].max(axis=1)
    return (loudness < before - DROPOUT_DB) & (loudness < after - DROPOUT_DB)


def wide_runs(flags: Flags, width: int) -> Flags:
    """Only runs of at least ``width`` adjacent flagged bins in each frame."""
    table = pl.DataFrame({"flag": flags.reshape(-1)}).with_row_index("i")
    table = table.with_columns((pl.col("i") // flags.shape[1]).alias("frame"))
    run = pl.struct(pl.col("frame"), pl.col("flag").rle_id())
    kept = table.select(pl.col("flag") & (pl.len().over(run) >= width))["flag"]
    return kept.to_numpy().reshape(flags.shape)


def relative_frames(fraction: Floats, active: Flags) -> Flags:
    """Frames whose hole fraction stands out (robust z) against the active frames."""
    table = pl.DataFrame({"fraction": fraction, "active": active})
    values = pl.col("fraction").filter(pl.col("active"))
    spread = (values - values.median()).abs().median() * MAD_SCALE
    score = (pl.col("fraction") - values.median()) / pl.max_horizontal(spread, pl.lit(EPSILON))
    flagged = table.select(((score >= ROBUST_Z) | (spread <= EPSILON)) & (pl.col("fraction") > 0))
    return flagged.to_series().to_numpy()


def interpolated_level(level: Floats, flags: Flags) -> Floats:
    """Hole log magnitudes from the nearest unholed frames of the same bin (linear in dB), else
    from the frame's unholed bins."""
    filled = level.copy()
    frames = np.arange(level.shape[0])
    for column in np.nonzero(flags.any(axis=0))[0]:
        good = ~flags[:, column]
        holes_here = np.nonzero(flags[:, column])[0]
        if good.any():
            values = np.interp(holes_here, frames[good], level[good, column])
            near = np.min(np.abs(frames[good][None, :] - holes_here[:, None]), axis=1)
            usable = near <= FILL_REACH
            filled[holes_here[usable], column] = values[usable]
            flags_left = holes_here[~usable]
        else:
            flags_left = holes_here
        for frame in flags_left:
            bins = np.arange(level.shape[1])
            known = ~flags[frame]
            filled[frame, column] = np.interp(column, bins[known], level[frame, known])
    return filled


def filled_spectrum(spectrum: Complex, found: Holes, rate: int, length: int) -> Complex:
    """The spectrum with its holes' magnitudes interpolated and their phases propagated, then
    made consistent over the hole bins only."""
    size = size_of(rate)
    hop = size // OVERLAP
    level = 20 * np.log10(np.abs(spectrum) + EPSILON)
    envelope = smoothed_bins(interpolated_level(level, found.flags))
    magnitude = 10 ** (np.where(found.flags, envelope, level) / 20)
    phase = np.angle(spectrum)
    advance = 2 * np.pi * np.arange(spectrum.shape[1]) * hop / size
    noisy = noisy_frames(level, found.flags)
    random = np.random.default_rng(NOISE_SEED).uniform(-np.pi, np.pi, phase.shape)
    for frame in range(1, len(phase)):
        holed = found.flags[frame]
        # PNS-like noise phase in noisy frames, phase-vocoder advance in harmonic ones.
        step = random[frame, holed] if noisy[frame] else phase[frame - 1, holed] + advance[holed]
        phase[frame, holed] = step
    result = np.where(found.flags, magnitude * np.exp(1j * phase), spectrum)
    for _ in range(CONSISTENCY_ITERATIONS):
        projected = stft(istft(result, rate, length), rate)[: len(result)]
        consistent = magnitude * np.exp(1j * np.angle(projected))
        result = np.where(found.flags, consistent, spectrum)
    return result


def smoothed_bins(level: Floats) -> Floats:
    """The log magnitude smoothed over ``ENVELOPE_BINS`` bins: a band envelope, so a fill does
    not carry one neighbour's harmonic ripple."""
    kernel = np.ones(ENVELOPE_BINS) / ENVELOPE_BINS
    padded = np.pad(level, ((0, 0), (ENVELOPE_BINS // 2, ENVELOPE_BINS // 2)), mode="edge")
    return np.apply_along_axis(np.convolve, 1, padded, kernel, mode="valid")[:, : level.shape[1]]


def noisy_frames(level: Floats, flags: Flags) -> Flags:
    """Frames whose unholed bins are noise-like (log flatness above ``NOISY_FLATNESS``)."""
    power = np.where(flags, np.nan, 10 ** (level / 10))
    flatness = np.nanmean(np.log(power + EPSILON), axis=1) - np.log(
        np.nanmean(power, axis=1) + EPSILON
    )
    return np.nan_to_num(flatness, nan=0.0) > NOISY_FLATNESS


def stretch(found: Holes, segment: slice, hop: int, length: int) -> slice:
    """``segment`` widened to the whole run of holed frames it touches (gaps of up to
    ``STRETCH_GAP`` frames joined); the transform's frames start one window early."""
    holed = found.flags.any(axis=1)
    first, last = segment.start // hop + OVERLAP, (segment.stop - 1) // hop + OVERLAP
    while first > 0 and holed[max(0, first - STRETCH_GAP) : first].any():
        first -= 1
    while last < len(holed) - 1 and holed[last + 1 : last + 1 + STRETCH_GAP].any():
        last += 1
    begin, end = segment.start // hop + OVERLAP, (segment.stop - 1) // hop + OVERLAP
    start = max(0, (first - OVERLAP) * hop) if first < begin else segment.start
    stop = min(length, (last - OVERLAP + 1) * hop) if last > end else segment.stop
    return slice(min(start, segment.start), max(stop, segment.stop))


def fade(length: int, rate: int) -> Floats:
    ramp = min(int(FADE_SECONDS * rate), length // 2)
    weights = np.ones(length)
    if ramp:
        rise = 0.5 - 0.5 * np.cos(np.pi * (np.arange(ramp) + 0.5) / ramp)
        weights[:ramp], weights[length - ramp :] = rise, rise[::-1]
    return weights


def filled(channels: Floats, segment: slice, rate: int) -> tuple[Floats, Holes, Holes, slice]:
    """The excerpt with the holes of the whole holed stretch around ``segment`` filled (each
    channel the same correction as the mid, so the stereo image is kept), crossfaded in,
    bit-identical elsewhere; with the holes found before and after, and the stretch."""
    mid = channels.mean(axis=1)
    spectrum = stft(mid, rate)
    found = holes(spectrum, rate)
    repaired = istft(filled_spectrum(spectrum, found, rate, len(mid)), rate, len(mid))
    segment = stretch(found, segment, size_of(rate) // OVERLAP, len(mid))
    correction = np.zeros_like(mid)
    weights = fade(segment.stop - segment.start, rate)
    correction[segment] = weights * (repaired[segment] - mid[segment])
    result = channels.copy()
    result[segment] = channels[segment] + correction[segment, None]
    after = holes(stft(result.mean(axis=1), rate), rate)
    return result, found, after, segment
