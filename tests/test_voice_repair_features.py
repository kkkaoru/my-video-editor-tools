"""Frame features against a naive per-frame reference, and every Polars detector, flag and
segment table on synthetic inputs."""

import numpy as np
import numpy.typing as npt
import polars as pl

from replay import voice_repair_features as analysis

RATE = 16_000
HOP = RATE // analysis.FRAME_RATE
NO_RUNS = np.zeros((0, 2), np.int64)


def voiced(seconds: float) -> npt.NDArray[np.float32]:
    t = np.arange(int(seconds * RATE)) / RATE
    phase = 2 * np.pi * np.cumsum(150 + 10 * np.sin(2 * np.pi * 3 * t)) / RATE
    tone = sum(np.sin(k * phase) / k for k in range(1, 8))
    noise = 0.01 * np.random.default_rng(0).standard_normal(len(t))
    return np.asarray(0.2 * tone + noise, np.float32)


def test_features_match_a_naive_frame_by_frame_reference_across_blocks() -> None:
    mid = voiced(1.0)
    saved, analysis.BLOCK_FRAMES = analysis.BLOCK_FRAMES, 7  # several blocks and a remainder
    try:
        table = analysis.features(analysis.array_reader(mid), len(mid), RATE)
    finally:
        analysis.BLOCK_FRAMES = saved
    assert table.columns == ["frame", *analysis.VOICE_FEATURES]
    assert table.height == 50
    size = analysis.fft_size(HOP)
    frame = 20  # one frame, by hand
    window = np.zeros(size)
    window[: min(size, len(mid) - frame * HOP)] = mid[frame * HOP : frame * HOP + size]
    own = window[:HOP]
    power = np.abs(np.fft.rfft(window * np.hanning(size))) ** 2 + analysis.EPSILON
    low, high = (int(hz * size / RATE) for hz in analysis.BAND_HZ)
    flatness = np.mean(np.log(power[low:high])) - np.log(np.mean(power[low:high]))
    centred = own - own.mean()
    row = table.row(frame, named=True)
    assert row["level_db"] == np.float64(10 * np.log10(np.mean(own**2) + analysis.EPSILON))
    assert abs(row["flatness"] - flatness) < 1e-9
    assert abs(row["kurtosis"] - np.mean(centred**4) / np.mean(centred**2) ** 2) < 1e-9
    assert abs(row["lag"] - 100) < 2  # the pitch at 0.4 s, 159.5 Hz
    assert row["harmonicity"] > 0.9


def test_an_empty_signal_has_an_empty_frame_table() -> None:
    table = analysis.features(analysis.array_reader(np.zeros(10, np.float32)), 10, RATE)
    assert table.height == 0
    assert analysis.mix_table([]).height == 0


def test_the_mix_measures_plateaus_pile_ups_and_ringing_per_frame() -> None:
    t = np.arange(4 * HOP) / RATE
    wave = np.sin(2 * np.pi * 200 * t)
    clipped = np.clip(wave, -0.5, 0.5).astype(np.float32)
    ripple = 0.02 * np.sin(2 * np.pi * 3000 * t) + 0.02 * np.sin(2 * np.pi * 2300 * t)
    ringing = (np.sign(wave) * (0.94 + ripple)).astype(np.float32)
    signal = np.concatenate((wave.astype(np.float32), clipped, ringing))
    measured = analysis.mix_block(signal, HOP)
    assert measured["plateau"][:4].tolist() == [0.0, 0.0, 0.0, 0.0]
    assert min(measured["plateau"][4:8]) > 100
    assert max(measured["near_peak"][:4]) < 0.15 < min(measured["near_peak"][4:8])
    assert min(measured["ringing"][8:]) > 30
    runs = analysis.plateau_runs(clipped, HOP, 1000)
    assert runs[0].tolist() == [1007, 27]
    assert len(runs) == 32  # both half-cycles of 16 periods


def test_plateau_samples_keep_runs_of_five_equal_samples() -> None:
    steps = np.array([0, 1, 1, 1, 1, 0, 1, 1, 0], dtype=bool)  # a step flag joins two samples
    assert analysis.plateau_samples(steps).astype(int).tolist() == [1, 1, 1, 1, 1, 0, 0, 0, 0]


def test_the_harmonic_ratio_weighs_odd_against_even_harmonics() -> None:
    power = np.full((1, 64), 1e-12)
    power[0, [6, 10, 14]] = 10.0  # the odd harmonics of a 2-bin fundamental
    power[0, [4, 8, 12]] = 1.0
    ratio = analysis.harmonic_ratio(power, np.array([32], dtype=np.int64), 64)
    assert ratio.round(6).tolist() == [10.0]


def frame_table(count: int = 40, **columns: list[float]) -> tuple[pl.DataFrame, pl.DataFrame]:
    """Ordinary speech frames (quiet ones first set the floor, the last is silent), overridden."""
    quiet = max(5, count // 10)
    defaults: dict[str, list[float]] = {
        "level_db": [-60.0] * quiet + [-20.0] * (count - quiet - 1) + [-110.0],
        "flux": [2.0 + 0.1 * (i % 5) for i in range(count)],
        "flatness": [-6.0 + 0.1 * (i % 3) for i in range(count)],
        "harmonicity": [0.8 + 0.01 * (i % 4) for i in range(count)],
        "lag": [300.0 + (i % 7) for i in range(count)],
        "odd_even_db": [0.1 * (i % 5) for i in range(count)],
        "band_holes": [0.0] * count,
        "crest_db": [8.0 + 0.2 * (i % 5) for i in range(count)],
        "kurtosis": [3.0 + 0.1 * (i % 5) for i in range(count)],
        "peak": [0.25 + 0.01 * (i % 5) for i in range(count)],
    }
    mix = {"plateau": [0.0] * count, "near_peak": [0.01] * count, "ringing": [2.0] * count}
    voice = {name: columns.get(name, values) for name, values in defaults.items()}
    mixed = {name: columns.get(name, values) for name, values in mix.items()}
    frames = pl.DataFrame({"frame": list(range(count)), **voice})
    return frames, pl.DataFrame({"frame": list(range(count)), **mixed})


def flags(
    frames: tuple[pl.DataFrame, pl.DataFrame], runs: npt.NDArray[np.int64] = NO_RUNS
) -> pl.DataFrame:
    return analysis.flagged(*frames, analysis.runs_table(runs), HOP).collect()


def test_ordinary_speech_flags_nothing() -> None:
    found = flags(frame_table())
    assert found.select("clipping", "soft_clipping", "robotic").sum().row(0) == (0, 0, 0)
    assert analysis.detector_seconds(found.lazy())["frozen"] == 0.0


def test_a_relative_frozen_spectrum_is_held_and_a_single_frame_is_not() -> None:
    flux = [2.0 + 0.1 * (i % 5) for i in range(40)]
    flux[10:13] = [0.3, 0.3, 0.3]  # far below this recording's own flux, above the fixed floor
    flux[30] = 0.3
    found = flags(frame_table(flux=flux))
    assert found.filter(pl.col("frozen"))["frame"].to_list() == [10, 11, 12]
    assert found["frozen_relative"][30] is True
    assert found["frozen_fixed"].any() is False
    assert found.filter(pl.col("robotic"))["frame"].to_list() == [10, 11, 12]


def test_a_noise_collapse_counts_only_between_voiced_frames() -> None:
    harmonicity = [0.8] * 40
    flatness = [-6.0 + 0.1 * (i % 3) for i in range(40)]
    harmonicity[20:23], flatness[20:23] = [0.1] * 3, [-0.5] * 3  # a burst inside speech
    harmonicity[34:39], flatness[34:39] = [0.1] * 5, [-0.5] * 5  # a fricative before the end
    found = flags(frame_table(harmonicity=harmonicity, flatness=flatness))
    assert found.filter(pl.col("collapse"))["frame"].to_list() == [20, 21, 22]


def test_a_pitch_held_exactly_for_ten_voiced_frames_is_frozen() -> None:
    lag = [300.0 + (i % 7) for i in range(40)]
    lag[10:22] = [250.0] * 12
    found = flags(frame_table(lag=lag))
    assert found.filter(pl.col("pitch_frozen"))["frame"].to_list() == list(range(11, 22))


def test_soft_clipping_flags_a_collapsed_crest_and_kurtosis() -> None:
    crest = [8.0 + 0.2 * (i % 5) for i in range(40)]
    kurtosis = [3.0 + 0.1 * (i % 5) for i in range(40)]
    crest[8:11], kurtosis[25:29] = [3.0] * 3, [0.9] * 4
    found = flags(frame_table(crest_db=crest, kurtosis=kurtosis))
    assert found.filter(pl.col("crest"))["frame"].to_list() == [8, 9, 10]
    assert found.filter(pl.col("low_kurtosis"))["frame"].to_list() == [25, 26, 27, 28]
    assert found.filter(pl.col("soft_clipping"))["frame"].to_list() == [8, 9, 10, 25, 26, 27, 28]


def test_odd_harmonics_flag_only_voiced_frames() -> None:
    odd = [0.1 * (i % 5) for i in range(40)]
    harmonicity = [0.8 + 0.01 * (i % 4) for i in range(40)]
    odd[12:15] = [20.0] * 3
    odd[30:33], harmonicity[30:33] = [20.0] * 3, [0.5] * 3
    found = flags(frame_table(odd_even_db=odd, harmonicity=harmonicity))
    assert found.filter(pl.col("odd_harmonics_fixed"))["frame"].to_list() == [12, 13, 14]
    assert found.filter(pl.col("odd_harmonics"))["frame"].to_list() == [12, 13, 14, 30, 31, 32]


def test_count_detectors_use_the_fixed_floor_or_a_multiple_of_the_reference() -> None:
    plateau, ringing, holes = [0.0] * 400, [2.0] * 400, [0.0] * 400
    plateau[106:109] = [40.0] * 3  # fixed: at least 30 plateau samples
    ringing[115:118] = [10.0] * 3  # relative: over 3x this recording's 99th percentile (2)
    holes[125:128] = [0.6] * 3
    found = flags(frame_table(400, plateau=plateau, ringing=ringing, band_holes=holes))
    assert found.filter(pl.col("plateau_runs"))["frame"].to_list() == [106, 107, 108]
    assert found.filter(pl.col("ringing_relative"))["frame"].to_list() == [115, 116, 117]
    assert found.filter(pl.col("ringing_fixed")).height == 0
    assert found.filter(pl.col("spectral_holes"))["frame"].to_list() == [125, 126, 127]
    assert found.filter(pl.col("clipping"))["frame"].to_list() == [106, 107, 108, 115, 116, 117]


def test_clipped_runs_mark_active_frames_only() -> None:
    found = flags(frame_table(20), np.array([[7 * HOP - 1, 2], [19 * HOP, 4]]))
    assert found.filter(pl.col("clipped"))["frame"].to_list() == [6, 7]
    assert found.filter(pl.col("clipping"))["frame"].to_list() == [6, 7]
    assert analysis.detector_seconds(found.lazy())["clipped"] == 0.04


def test_too_few_clean_frames_flag_no_relative_detections() -> None:
    found = flags(frame_table(9, flux=[0.0] * 9), np.array([[5 * HOP, 3 * HOP]]))
    assert found["frozen_fixed"].all() is True
    assert found["frozen"].any() is False


def test_statistics_report_the_reference_and_its_spreads() -> None:
    flatness = [-1.0] * 5 + [-6.0, -6.2, -5.8, -6.0, -6.0, -6.0] + [-1.0]
    harmonicity = [0.1] * 5 + [0.8] * 6 + [0.1]
    found = analysis.statistics(
        analysis.flagged(
            *frame_table(12, flatness=flatness, harmonicity=harmonicity),
            analysis.runs_table(NO_RUNS),
            HOP,
        )  # fmt: skip
    )
    assert found["referenceFrames"] == 6.0
    assert found["medianFlatness"] == -6.0
    assert found["spreadFlatness"] == 0.0
    assert round(found["medianHarmonicity"], 6) == 0.8
    assert found["spreadHarmonicityDb"] == 0.0
    assert found["roboticFrames"] == 0.0
    assert round(found["compressorThresholdDb"], 3) == -16.752  # 20 log10(0.29) - 6


def test_flagged_frames_become_padded_merged_segments_with_their_kinds() -> None:
    clipping = [i in (10, 12) for i in range(100)]
    robotic = [i in (60, 61, 62) for i in range(100)]
    table = pl.DataFrame(
        {
            "frame": list(range(100)),
            "clipping": clipping,
            "soft_clipping": [False] * 100,
            "robotic": robotic,
        }  # fmt: skip
    )
    runs = analysis.runs_table(np.array([[10 * HOP, 3], [12 * HOP + 5, 4]]))
    found = analysis.segments(table.lazy(), runs, hop=HOP, length=100 * HOP, rate=RATE)
    assert found.to_dicts() == [
        {"start": 2560, "end": 4800, "clipping": True, "soft_clipping": False, "robotic": False,
         "clipped_samples": 7, "longest_run": 4, "severity": "mild"},
        {"start": 18560, "end": 20800, "clipping": False, "soft_clipping": False, "robotic": True,
         "clipped_samples": 0, "longest_run": 0, "severity": "severe"},
    ]  # fmt: skip


def test_long_clipped_runs_are_severe_and_segments_stop_at_the_edges() -> None:
    table = pl.DataFrame(
        {
            "frame": [0, 1, 2],
            "clipping": [True, False, True],
            "soft_clipping": [False] * 3,
            "robotic": [False] * 3,
        }  # fmt: skip
    )
    runs = analysis.runs_table(np.array([[0, 40], [2 * HOP, 3]]))
    found = analysis.segments(table.lazy(), runs, hop=HOP, length=3 * HOP, rate=RATE)
    assert found.to_dicts() == [
        {"start": 0, "end": 960, "clipping": True, "soft_clipping": False, "robotic": False,
         "clipped_samples": 43, "longest_run": 40, "severity": "severe"},
    ]  # fmt: skip
