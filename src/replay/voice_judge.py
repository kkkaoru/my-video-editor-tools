"""A computed voice-quality judge: a processing stage's output against its input, no listening.

Each judged window pairs the stage's output with the input it was made from: the same samples
for a same-length stage, the input uniformly retimed at the window's exact source position for
a cut-slot tempo stage (``ratio`` input samples per output sample).
Measures, all computed from the PCM:

- ``snrP50Db`` / ``snrP10Db``: waveform fidelity, the median and 10th-percentile SNR of 10 ms
  output blocks against the best-lag, best-gain input block (the lag searched around the
  mapped position). Overlap-add time stretching (WSOLA, FFmpeg ``atempo``) re-cuts the
  waveform even at a unity-like ratio and drops it; a gain change, a de-esser and a resample
  keep it (Verhelst & Roelands 1993 on WSOLA splices).
- ``coherenceDrop``: loss of vertical phase coherence at spectral peaks, the phase agreement of
  each peak's neighbouring bins under a zero-phase Hann window (Laroche & Dolson 1999,
  "phasiness").
- ``fluxExcessPerSecond``: spectral-flux bursts in the output beyond those of the input.
- ``f0JumpExcessPerSecond``: pitch-track breaks (consecutive voiced 10 ms frames more than
  ``F0_JUMP_SEMITONES`` apart) beyond those of the input.
- ``newClicksPerSecond``: clicks the stage adds, from AR residual outliers (Vaseghi & Rayner
  1990; Godsill & Rayner 1998): blocks whitened by their own order-``AR_ORDER`` predictor,
  samples beyond ``CLICK_SIGMAS`` robust deviations, merged within 1 ms, kept when no input
  click lies within ``CLICK_TOLERANCE_SECONDS`` of the mapped time.
- ``clippedExcess``: the share of samples in flat runs at the window's peak, beyond the input's.
- ``timbreShiftDb``: the change of voice colour, the RMS difference of the level-matched long-term
  average spectra (dB, ``STFT_SIZE`` Hann frames above ``AUDIBLE_DB``, bins within
  ``TIMBRE_RANGE_DB`` of the reference's loudest). A resynthesis that keeps
  the speaker embedding can still re-colour the voice: the user heard Sidon's 10/28 and 10/29
  spans as extremely unnatural at 3.3–4.9 dB and MossFormer2's at 1.8 dB, while the approved
  de-esser measured 0.06 dB (``edits/work/voicefix/ng-spans.json``).

``verdict`` fails a window on any limit. The limits are fixed constants calibrated on synthetic
degradations (``tests/test_voice_judge.py``): legitimate processing (gain, de-esser, natural
compressor, resampling) must pass and injected clicks, clipping and FFmpeg ``atempo`` must
fail, each limit between the worst passing and the best failing value. They are never tuned
to one recording.

``python -m replay.voice_judge same INPUT OUTPUT REPORT`` judges a same-length stage and
``python -m replay.voice_judge tempo INPUT OUTPUT RECEIPT REPORT`` a cut-slot tempo render
(its receipt's ``cuts``). Both judge ``WINDOWS`` evenly spread audible windows, write the
report and exit 1 on a failing window.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Mapping, Sequence
from dataclasses import asdict, dataclass
from pathlib import Path

import numpy as np
import numpy.typing as npt

from replay import voice_repair as repair
from replay import voice_tempo as tempo
from replay.core import ReplayError
from replay.recordings import record

Floats = npt.NDArray[np.float64]

VERSION = 2
BLOCK_SECONDS = 0.01
LAG_SECONDS = 0.003
AUDIBLE_DB = -45.0  # 10 ms blocks quieter than this (dBFS) carry no judged speech
STFT_SIZE = 2048
STFT_HOP = 512
PEAK_FLOOR_DB = -40.0  # spectral peaks within this of the frame maximum
TIMBRE_RANGE_DB = 50.0  # long-term spectrum bins within this of the reference's loudest
FLUX_SIZE = 1024
FLUX_SIGMAS = 6.0
F0_RANGE_HZ = (70.0, 400.0)
F0_VOICED = 0.7
F0_JUMP_SEMITONES = 4.0
AR_ORDER = 24
AR_BLOCK_SECONDS = 0.02
CLICK_SIGMAS = 8.0
CLICK_MERGE_SECONDS = 0.001
CLICK_TOLERANCE_SECONDS = 0.001
CLICK_RATIO = 2.0
FIT_TAPS = 33
MAX_SNR_DB = 120.0  # an unchanged block; also a window with no audible block
CLIP_RUN = 3
CLIP_RELATIVE = 0.999
WINDOW_SECONDS = 8.0
WINDOWS = 24
FULL_SCALE = 32768
# Each limit lies between the worst must-pass and the best must-fail synthetic case
# (tests/test_voice_judge.py asserts both sides).
LIMITS: dict[str, tuple[str, float]] = {
    "snrP50Db": ("min", 30.0),
    "snrP10Db": ("min", 8.0),
    "coherenceDrop": ("max", 0.1),
    "fluxExcessPerSecond": ("max", 0.75),
    "f0JumpExcessPerSecond": ("max", 1.0),
    "newClicksPerSecond": ("max", 0.3),
    "clippedExcess": ("max", 0.005),
    "timbreShiftDb": ("max", 1.0),
}


@dataclass(frozen=True)
class Score:
    snrP50Db: float
    snrP10Db: float
    coherenceDrop: float
    fluxExcessPerSecond: float
    f0JumpExcessPerSecond: float
    newClicksPerSecond: float
    clippedExcess: float
    timbreShiftDb: float


def mono(samples: npt.ArrayLike) -> Floats:
    """Interleaved int16 frames (or float samples) as one float channel in full-scale units."""
    values = np.asarray(samples, np.float64)
    return (values.mean(axis=1) if values.ndim == 2 else values) / FULL_SCALE


def block_snr(reference: Floats, output: Floats, rate: int) -> Floats:
    """Each audible 10 ms output block's SNR (dB) against the reference block that explains it
    best: the best-correlating lag near the same position, refined by a least-squares
    ``FIT_TAPS``-tap filter (gain, fractional delay and gentle equalisation are legitimate)."""
    size, lag, half = round(BLOCK_SECONDS * rate), round(LAG_SECONDS * rate), FIT_TAPS // 2
    floor = size * 10 ** (AUDIBLE_DB / 10)
    result = []
    for start in range(0, len(output) - size + 1, size):
        block = output[start : start + size]
        energy = float(block @ block)
        low, high = start - lag - half, start + lag + size + half
        if energy < floor or low < 0 or high > len(reference):
            continue
        near = reference[low:high]
        inner = near[half:-half]
        products = np.correlate(inner, block, mode="valid")
        cumulative = np.concatenate(([0.0], np.cumsum(inner * inner)))
        energies = cumulative[size:] - cumulative[:-size] + 1e-12
        best = half + int(np.argmax(products * products / energies))
        taps = np.stack([near[best + k : best + k + size] for k in range(-half, half + 1)], axis=1)
        fit, *_ = np.linalg.lstsq(taps, block, rcond=None)
        error = block - taps @ fit
        result.append(
            10 * np.log10(energy / max(float(error @ error), energy * 10 ** (-MAX_SNR_DB / 10)))
        )
    return np.asarray(result, np.float64)


def long_term_spectrum(signal: Floats) -> Floats | None:
    """The mean dB power spectrum of the audible ``STFT_SIZE`` frames (None when none is)."""
    if len(signal) < STFT_SIZE:
        return None
    window = np.hanning(STFT_SIZE)
    frames = np.stack([signal[s : s + STFT_SIZE] * window
                       for s in range(0, len(signal) - STFT_SIZE + 1, STFT_HOP)])  # fmt: skip
    power = np.abs(np.fft.rfft(frames, axis=1)) ** 2 + 1e-20
    audible = 10 * np.log10(np.mean(frames**2, axis=1) + 1e-20) > AUDIBLE_DB
    return np.asarray(10 * np.log10(power[audible]).mean(axis=0)) if audible.any() else None


def timbre_shift(reference: Floats, output: Floats) -> float:
    """RMS dB difference of the two long-term spectra after matching their mean level."""
    first, second = long_term_spectrum(reference), long_term_spectrum(output)
    if first is None or second is None:
        return 0.0
    heard = first > first.max() - TIMBRE_RANGE_DB  # near-silent bins carry only noise-floor dB
    difference = (second - first)[heard]
    return float(np.sqrt(np.mean((difference - difference.mean()) ** 2)))


def coherence(signal: Floats) -> float:
    """Magnitude-weighted vertical phase coherence at spectral peaks (1: phase-locked)."""
    if len(signal) < STFT_SIZE:
        return 1.0
    window = np.hanning(STFT_SIZE)
    starts = range(0, len(signal) - STFT_SIZE + 1, STFT_HOP)
    frames = np.stack(
        [np.roll(signal[s : s + STFT_SIZE] * window, -STFT_SIZE // 2) for s in starts]
    )
    spectra = np.fft.rfft(frames, axis=1)
    magnitude, phase = np.abs(spectra), np.angle(spectra)
    inner = magnitude[:, 1:-1]
    peaks = (inner > magnitude[:, :-2]) & (inner >= magnitude[:, 2:])
    peaks &= inner > magnitude.max(axis=1, keepdims=True) * 10 ** (PEAK_FLOOR_DB / 20)
    agreement = (np.cos(phase[:, 2:] - phase[:, 1:-1]) + np.cos(phase[:, :-2] - phase[:, 1:-1])) / 2
    weight = inner * peaks
    total = float(weight.sum())
    return float((agreement * weight).sum() / total) if total > 0 else 1.0


def flux_events(signal: Floats, rate: int) -> int:
    """Audible frames whose positive log-spectral flux leaves the window's robust spread."""
    hop = round(BLOCK_SECONDS * rate)
    if len(signal) < FLUX_SIZE + 2 * hop:
        return 0
    window = np.hanning(FLUX_SIZE)
    starts = range(0, len(signal) - FLUX_SIZE + 1, hop)
    frames = np.stack([signal[s : s + FLUX_SIZE] for s in starts])
    level = 10 * np.log10(np.mean(frames * frames, axis=1) + 1e-12)
    spectra = np.log(np.abs(np.fft.rfft(frames * window, axis=1)) + 1e-6)
    flux = np.maximum(np.diff(spectra, axis=0), 0).mean(axis=1)
    median = np.median(flux)
    spread = 1.4826 * np.median(np.abs(flux - median)) + 1e-9
    return int(np.sum((flux > median + FLUX_SIGMAS * spread) & (level[1:] > AUDIBLE_DB)))


def f0_track(signal: Floats, rate: int) -> Floats:
    """A 10 ms normalised-autocorrelation pitch track (Hz; NaN where unvoiced)."""
    hop = round(BLOCK_SECONDS * rate)
    size = 4 * hop
    shortest, longest = round(rate / F0_RANGE_HZ[1]), round(rate / F0_RANGE_HZ[0])
    track = []
    for start in range(0, len(signal) - size - longest + 1, hop):
        frame = signal[start : start + size + longest]
        head = frame[:size]
        energy = float(head @ head)
        if energy < size * 10 ** (AUDIBLE_DB / 10):
            track.append(np.nan)
            continue
        products = np.correlate(frame, head, mode="valid")[shortest : longest + 1]
        cumulative = np.concatenate(([0.0], np.cumsum(frame * frame)))
        energies = cumulative[size:] - cumulative[:-size]
        normalised = products / np.sqrt(energy * energies[shortest : longest + 1] + 1e-12)
        best = int(np.argmax(normalised))
        track.append(rate / (shortest + best) if normalised[best] >= F0_VOICED else np.nan)
    return np.asarray(track, np.float64)


def f0_jumps(signal: Floats, rate: int) -> int:
    track = f0_track(signal, rate)
    steps = np.abs(12 * np.log2(track[1:] / track[:-1]))
    return int(np.sum(steps[np.isfinite(steps)] > F0_JUMP_SEMITONES))


def levinson(lags: Floats) -> Floats:
    """The inverse (whitening) filter ``[1, -a1, ..., -ap]`` of the order ``len(lags) - 1``
    autocorrelation predictor, by the Levinson-Durbin recursion."""
    inverse, error = np.array([1.0]), float(lags[0])
    for order in range(1, len(lags)):
        reflection = -float(inverse @ lags[order:0:-1]) / error
        padded = np.concatenate((inverse, [0.0]))
        inverse = padded + reflection * padded[::-1]
        error *= 1 - reflection * reflection
    return inverse


def residual_sigmas(signal: Floats, rate: int) -> Floats:
    """The signal whitened by each block's own order-``AR_ORDER`` predictor, in robust
    deviations of that block's residual (0 where a block is below ``AUDIBLE_DB``)."""
    size = round(AR_BLOCK_SECONDS * rate)
    window = np.hanning(size)
    result = np.zeros(len(signal))
    for start in range(AR_ORDER, len(signal) - size + 1, size):
        block = signal[start - AR_ORDER : start + size]
        shaped = block[AR_ORDER:] * window
        lags = np.correlate(shaped, shaped, mode="full")[size - 1 : size + AR_ORDER]
        if lags[0] < float(window @ window) * 10 ** (AUDIBLE_DB / 10):
            continue  # silence: quantisation, not a click
        lags[0] *= 1 + 1e-9  # white-noise conditioning keeps the recursion stable
        inverse = levinson(lags)
        residual = np.convolve(block, inverse)[AR_ORDER : AR_ORDER + size]
        sigma = 1.4826 * float(np.median(np.abs(residual))) + 1e-12
        result[start : start + size] = np.abs(residual) / sigma
    return result


def clicks(sigmas: Floats, rate: int) -> npt.NDArray[np.intp]:
    """Click positions (samples): residual outliers beyond ``CLICK_SIGMAS``, one per burst."""
    positions = np.flatnonzero(sigmas > CLICK_SIGMAS)
    merge = max(1, round(CLICK_MERGE_SECONDS * rate))
    return (
        positions[np.concatenate(([True], np.diff(positions) > merge))]
        if len(positions)
        else positions
    )


def new_clicks(reference: Floats, output: Floats, rate: int) -> int:
    """Output clicks whose residual outlier is ``CLICK_RATIO`` times the reference's strongest
    within ``CLICK_TOLERANCE_SECONDS``: an outlier the input already
    had (a glottal pulse, a source click) is not the stage's."""
    made, known = residual_sigmas(output, rate), residual_sigmas(reference, rate)
    reach = round(CLICK_TOLERANCE_SECONDS * rate)
    count = 0
    for at in clicks(made, rate):
        near = known[max(0, at - reach) : at + reach + 1]
        strongest = float(near.max()) if len(near) else 0.0
        count += int(made[max(0, at - reach) : at + reach + 1].max() > CLICK_RATIO * strongest)
    return count


def clipped_share(signal: Floats) -> float:
    """The share of samples in runs of at least ``CLIP_RUN`` at the window's peak magnitude."""
    peak = float(np.max(np.abs(signal))) if len(signal) else 0.0
    if peak <= 0:
        return 0.0
    at = np.concatenate(([0], (np.abs(signal) >= CLIP_RELATIVE * peak).astype(np.int8), [0]))
    edges = np.diff(at)
    lengths = np.flatnonzero(edges == -1) - np.flatnonzero(edges == 1)
    return float(lengths[lengths >= CLIP_RUN].sum()) / len(signal)


def scored(span: Floats, output: Floats, rate: int) -> Score:
    """The judge's measures for one output window against its same-length reference."""
    seconds = len(output) / rate
    snr = block_snr(span, output, rate)
    return Score(
        snrP50Db=float(np.percentile(snr, 50)) if len(snr) else MAX_SNR_DB,
        snrP10Db=float(np.percentile(snr, 10)) if len(snr) else MAX_SNR_DB,
        coherenceDrop=coherence(span) - coherence(output),
        fluxExcessPerSecond=max(0, flux_events(output, rate) - flux_events(span, rate)) / seconds,
        f0JumpExcessPerSecond=max(0, f0_jumps(output, rate) - f0_jumps(span, rate)) / seconds,
        newClicksPerSecond=new_clicks(span, output, rate) / seconds,
        clippedExcess=max(0.0, clipped_share(output) - clipped_share(span)),
        timbreShiftDb=timbre_shift(span, output),
    )


def failures(score: Score) -> list[str]:
    """The measures outside their fixed limits."""
    values = asdict(score)
    return [
        name
        for name, (kind, limit) in LIMITS.items()
        if (values[name] < limit if kind == "min" else values[name] > limit)
    ]


@dataclass(frozen=True)
class Window:
    output_start: int
    reference_start: float
    ratio: float


def same_windows(frames: int, rate: int, count: int = WINDOWS) -> list[Window]:
    """``count`` evenly spread windows of a same-length stage."""
    size = round(WINDOW_SECONDS * rate)
    starts = np.linspace(0, max(0, frames - size), count).round().astype(int)
    return [Window(int(at), int(at), 1.0) for at in np.unique(starts)]


def tempo_windows(
    cuts: Sequence[Mapping[str, object]], rate: int, count: int = WINDOWS
) -> list[Window]:
    """Up to ``count`` windows inside the stretched cut slots (the ones a tempo stage changes),
    longest slots first, each slot's mapping exact at the window start."""
    size = round(WINDOW_SECONDS * rate)
    origin = int(str(cuts[0]["output_start_sample"])) if cuts else 0
    slots = []
    for cut in cuts:
        source = int(str(cut["source_end_sample"])) - int(str(cut["source_start_sample"]))
        output = int(str(cut["output_end_sample"])) - int(str(cut["output_start_sample"]))
        if output >= size and source != output:
            slots.append((output, cut, source / output))
    slots.sort(key=lambda slot: -slot[0])
    windows = []
    for output, cut, ratio in slots[:count]:
        offset = (output - size) // 2
        windows.append(Window(
            int(str(cut["output_start_sample"])) - origin + offset,
            int(str(cut["source_start_sample"])) + offset * ratio,
            ratio,
        ))  # fmt: skip
    return windows


def reference_window(samples: repair.Memmap, window: Window, size: int) -> Floats:
    """The input under ``window``: a same-length stage's samples, or a retimed stage's input
    uniformly retimed at the window's exact source position (band-limited resampling), the
    ideal the stage approximates."""
    if window.ratio == 1 and float(window.reference_start).is_integer():
        start = int(window.reference_start)
        return mono(samples[start : start + size])
    return mono(tempo.resampled(samples, window.reference_start, window.ratio, size))


def judged(reference: Path, output: Path, windows: Sequence[Window]) -> dict[str, object]:
    """Every window's score and failures, and whether all of them passed."""
    source, made = repair.pcm(reference), repair.pcm(output)
    if source.rate != made.rate:
        raise ReplayError("The judged stage changed the sample rate")
    rate, size = made.rate, round(WINDOW_SECONDS * made.rate)
    rows = []
    for window in windows:
        span = reference_window(source.samples(), window, size)
        out = mono(made.samples()[window.output_start : window.output_start + size])
        if len(out) < size:
            continue
        score = scored(span, out, rate)
        rows.append({"outputSeconds": window.output_start / rate, **asdict(score),
                     "failures": failures(score)})  # fmt: skip
    if not rows:
        raise ReplayError("No window to judge")
    return {
        "version": VERSION,
        "input": str(reference),
        "output": str(output),
        "limits": {name: list(limit) for name, limit in LIMITS.items()},
        "windows": rows,
        "passed": all(not row["failures"] for row in rows),
    }


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    same = commands.add_parser("same", help="judge a same-length stage")
    retimed = commands.add_parser("tempo", help="judge a cut-slot tempo render")
    for command in (same, retimed):
        command.add_argument("input", type=Path)
        command.add_argument("output", type=Path)
    retimed.add_argument("receipt", type=Path)
    for command in (same, retimed):
        command.add_argument("report", type=Path)
    args = parser.parse_args(argv)
    made = repair.pcm(args.output)
    if args.command == "tempo":
        cuts = record(json.loads(args.receipt.read_text(encoding="utf-8")))["cuts"]
        if not isinstance(cuts, list):
            raise ReplayError("The tempo receipt has no cuts")
        windows = tempo_windows([record(cut) for cut in cuts], made.rate)
    else:
        windows = same_windows(made.frames, made.rate)
    report = judged(args.input, args.output, windows)
    args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    rows = report["windows"]
    failed = sum(1 for row in rows if record(row)["failures"]) if isinstance(rows, list) else 0
    if failed:
        print(f"voice judge anomaly: {failed} of {len(windows)} windows failed", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
