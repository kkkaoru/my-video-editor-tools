"""The natural compressor on synthetic declipped speech: limiting, dynamics, loudness."""

import numpy as np
import numpy.typing as npt
import pytest

from replay import voice_compressor as compressor

RATE = 44_100


def burst(seconds: float, overshoot: float) -> npt.NDArray[np.float64]:
    """A voiced burst whose middle 100 ms crests ``overshoot`` times higher (a declipped peak)."""
    t = np.arange(int(seconds * RATE)) / RATE
    tone = sum(np.sin(2 * np.pi * 150 * k * t) / k for k in range(1, 12))
    envelope = 0.25 * np.clip(np.sin(np.pi * t / seconds), 0, None) ** 0.5
    middle = np.abs(t - seconds / 2) < 0.05
    mono = tone * envelope * np.where(middle, overshoot, 1.0)
    return np.stack((mono, mono), axis=1)


def crest(channels: npt.NDArray[np.float64]) -> float:
    mid = channels.mean(axis=1)
    return float(20 * np.log10(np.abs(mid).max() / np.sqrt(np.mean(mid**2))))


def test_the_gain_computer_has_a_soft_knee() -> None:
    levels = np.array([-30.0, -24.0, -20.0, -16.0, -10.0])
    gain = compressor.gain_computer(levels, -20.0, 2.5, 8.0)
    assert gain.round(4).tolist() == [0.0, 0.0, -0.6, -2.4, -6.0]


def test_attack_is_fast_and_release_is_slow() -> None:
    target = np.concatenate((np.zeros(5), np.full(30, -6.0), np.zeros(400)))
    gain = compressor.smoothed(target, 0.008, 0.18)
    assert gain[5 + 24] < -5.0  # three attack constants in
    assert gain[35 + 100] < -3.0  # a fraction of the release later, still reduced
    assert gain[-1] > -1.0  # 2.2 release constants later


def test_a_declipped_overshoot_is_limited_without_flattening_the_dynamics() -> None:
    reference = burst(1.0, 1.0)
    repaired = burst(1.0, 4.5)  # the restored crest reaches past full scale
    assert float(np.abs(repaired).max()) > 1.0
    result = compressor.compressed(repaired, reference, RATE, threshold=-12.0)
    assert float(np.abs(result).max()) <= 10 ** (compressor.CEILING_DBFS / 20) + 1e-9
    assert crest(result) >= crest(reference) - 1.5  # no more crest loss than the gate allows
    assert abs(compressor.loudness(result, RATE) - compressor.loudness(reference, RATE)) <= 0.5


def test_speech_under_the_threshold_passes_at_its_own_loudness() -> None:
    reference = burst(1.0, 1.0) * 0.2
    result = compressor.compressed(reference, reference, RATE, threshold=-6.0)
    assert np.allclose(result, reference, atol=1e-9)
    assert crest(result) == pytest.approx(crest(reference))


def test_a_loud_burst_is_compressed_and_made_up_to_the_reference() -> None:
    reference = burst(1.0, 1.0)
    result = compressor.compressed(reference, reference, RATE, threshold=-24.0)
    assert crest(result) < crest(reference)
    assert crest(result) > crest(reference) - 3.0
    assert abs(compressor.loudness(result, RATE) - compressor.loudness(reference, RATE)) <= 0.5


def test_loudness_follows_bs_1770_for_a_one_kilohertz_sine() -> None:
    t = np.arange(48_000) / 48_000
    sine = np.sin(2 * np.pi * 1000 * t) * 10 ** (-20 / 20)
    assert compressor.loudness(np.stack((sine, sine), axis=1), 48_000) == pytest.approx(
        -20.0, abs=0.05
    )
    one = np.stack((sine, np.zeros_like(sine)), axis=1)[:47_999]  # an odd length too
    assert compressor.loudness(one, 48_000) == pytest.approx(-23.0, abs=0.05)


def test_the_k_weighting_matches_the_published_48_khz_design() -> None:
    (shelf_b, shelf_a), (high_b, high_a) = compressor.biquads(48_000)
    assert shelf_b.round(8).tolist() == [1.53512486, -2.69169619, 1.19839281]
    assert shelf_a.round(8).tolist() == [1.0, -1.69065929, 0.73248077]
    assert high_b.tolist() == [1.0, -2.0, 1.0]
    assert high_a.round(8).tolist() == [1.0, -1.99004745, 0.99007225]


def test_a_limiter_overshoot_left_by_smoothing_is_trimmed(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(compressor, "LIMIT_RATIO", 1.0)  # a limiter that does nothing
    reference = burst(0.5, 1.0)
    result = compressor.compressed(burst(0.5, 6.0), reference, RATE, threshold=0.0)
    assert float(np.abs(result).max()) == pytest.approx(10 ** (compressor.CEILING_DBFS / 20))
