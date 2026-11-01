"""Spectral-hole filling on synthetic speech with a zeroed band: detection, fill, mixing."""

import numpy as np
import numpy.typing as npt

from replay import voice_hole_fill as fill

RATE = 16_000


def harmonic(seconds: float) -> npt.NDArray[np.float64]:
    t = np.arange(int(seconds * RATE)) / RATE
    phase = 2 * np.pi * np.cumsum(140 + 6 * np.sin(2 * np.pi * 2 * t)) / RATE
    tone = sum(np.sin(k * phase) / k**0.7 for k in range(1, 40))
    noise = 0.002 * np.random.default_rng(0).standard_normal(len(t))
    return np.asarray(0.1 * tone + noise)


def holed(clean: npt.NDArray[np.float64]) -> tuple[npt.NDArray[np.float64], npt.NDArray[np.bool_]]:
    """``clean`` with bins 60-75 (1.9-2.3 kHz) zeroed in frames 40-55, and that truth mask."""
    spectrum = fill.stft(clean, RATE)
    truth = np.zeros(spectrum.shape, dtype=bool)
    truth[40:56, 60:76] = True
    spectrum[truth] = 0
    return fill.istft(spectrum, RATE, len(clean)), truth


def test_the_short_time_transform_reconstructs_exactly() -> None:
    clean = harmonic(0.5)
    assert np.allclose(fill.istft(fill.stft(clean, RATE), RATE, len(clean)), clean, atol=1e-12)
    assert fill.size_of(RATE) == 512


def test_a_zeroed_band_is_found_with_high_precision() -> None:
    signal, truth = holed(harmonic(1.0))
    found = fill.holes(fill.stft(signal, RATE), RATE)
    hits = found.flags & truth
    assert int(hits.sum()) / max(int(found.flags.sum()), 1) > 0.9  # precision
    assert int(hits.sum()) / int(truth.sum()) > 0.5  # recall (zeroed edges are smeared)
    assert found.fraction[40:56].max() > 0.05
    assert found.fraction[:30].max() == 0.0


def test_filling_the_band_brings_it_back_toward_the_clean_spectrum() -> None:
    clean = harmonic(1.0)
    signal, truth = holed(clean)
    channels = np.stack((signal, signal), axis=1)
    segment = slice(30 * 128, 66 * 128)
    result, before, after, stretched = fill.filled(channels, segment, RATE)
    reference = 20 * np.log10(np.abs(fill.stft(clean, RATE)) + 1e-10)
    damaged = 20 * np.log10(np.abs(fill.stft(signal, RATE)) + 1e-10)
    repaired = 20 * np.log10(np.abs(fill.stft(result[:, 0], RATE)) + 1e-10)
    lsd_before = np.sqrt(np.mean((damaged - reference)[truth] ** 2))
    lsd_after = np.sqrt(np.mean((repaired - reference)[truth] ** 2))
    assert lsd_after < 0.5 * lsd_before
    assert after.flags.sum() < 0.5 * before.flags.sum()
    assert stretched.start <= segment.start and stretched.stop >= segment.stop
    assert np.array_equal(result[: stretched.start], channels[: stretched.start])
    assert np.array_equal(result[stretched.stop :], channels[stretched.stop :])
    assert np.array_equal(result[:, 0], result[:, 1])


def test_a_signal_without_holes_is_left_alone() -> None:
    clean = harmonic(0.5)
    channels = np.stack((clean, clean), axis=1)
    result, before, _, stretched = fill.filled(channels, slice(1000, 4000), RATE)
    assert stretched == slice(1000, 4000)
    assert before.flags.sum() == 0
    assert np.allclose(result, channels, atol=1e-12)


def test_holes_without_unholed_neighbours_in_time_fill_across_frequency() -> None:
    level = np.full((3, 6), -20.0)
    level[:, 2] = -90.0
    flags = np.zeros((3, 6), dtype=bool)
    flags[:, 2] = True
    filled = fill.interpolated_level(level, flags)
    assert filled[:, 2].tolist() == [-20.0, -20.0, -20.0]


def test_relative_frames_need_a_standing_hole_fraction() -> None:
    fraction = np.array([0.0, 0.01, 0.02, 0.01, 0.3, 0.0])
    active = np.array([True, True, True, True, True, False])
    assert fill.relative_frames(fraction, active).tolist() == [
        False,
        False,
        False,
        False,
        True,
        False,
    ]


def test_only_wide_runs_of_hole_bins_count() -> None:
    flags = np.array([[1, 1, 0, 1, 1, 1, 1], [1, 0, 0, 0, 0, 0, 0]], dtype=bool)
    assert fill.wide_runs(flags, 3).astype(int).tolist() == [
        [0, 0, 0, 1, 1, 1, 1],
        [0, 0, 0, 0, 0, 0, 0],
    ]


def test_the_crossfade_ramps_both_ends() -> None:
    weights = fill.fade(400, RATE)
    assert weights[200] == 1.0
    assert weights[0] < 0.01
    assert fill.fade(1, RATE).tolist() == [1.0]


def test_a_frame_gated_out_between_speech_is_a_dropout() -> None:
    loudness = np.array([0.0, 0.0, 0.0, -80.0, 0.0, 0.0, 0.0, -80.0])
    active = loudness > -40
    assert fill.dropout_frames(loudness, active).tolist() == [
        False, False, False, True, False, False, False, False,
    ]  # fmt: skip


def test_a_dropped_frame_is_filled_from_its_neighbours() -> None:
    gated = harmonic(0.6)
    gated[4000:4960] = 0.0  # 60 ms gated out between speech
    found = fill.holes(fill.stft(gated, RATE), RATE)
    holed = np.nonzero(found.flags.sum(axis=1) > 100)[0]
    assert holed.tolist() == [37]  # the frame whose window lies inside the gap


def test_noisy_frames_are_told_from_harmonic_ones() -> None:
    level = np.array([[0.0] * 8, [0.0, -60.0] * 4])
    assert fill.noisy_frames(level, np.zeros((2, 8), dtype=bool)).tolist() == [True, False]


def test_the_band_envelope_smooths_neighbouring_bins() -> None:
    level = np.array([[0.0, 0.0, 10.0, 0.0, 0.0, 0.0]])
    assert fill.smoothed_bins(level).round(6).tolist() == [[2.0, 2.0, 2.0, 2.0, 2.0, 0.0]]
