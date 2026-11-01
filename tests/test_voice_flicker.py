"""Noise-suppression flicker: the sliced FFT equals the direct sum; gated bands score high."""

from __future__ import annotations

import subprocess
import wave
from pathlib import Path

import numpy as np
import numpy.typing as npt
import pytest

from replay import voice_flicker as flicker

Floats = npt.NDArray[np.float64]


def voice(seconds: float, gated: bool) -> Floats:
    """A 200 Hz harmonic voice with breath noise; ``gated`` chops 2-7 kHz every 7.5 ms."""
    rng = np.random.default_rng(0)
    t = np.arange(int(seconds * flicker.RATE)) / flicker.RATE
    harmonics = sum(np.sin(2 * np.pi * 200 * k * t) / k for k in range(1, 38))
    noise = np.convolve(rng.standard_normal(len(t)), np.ones(4) / 4, mode="same")
    signal = 0.1 * harmonics + 0.02 * noise
    if gated:
        spectrum = np.fft.rfft(signal)
        freqs = np.fft.rfftfreq(len(signal), 1 / flicker.RATE)
        band = np.fft.irfft(np.where((freqs > 2000) & (freqs < 7000), spectrum, 0), len(signal))
        gate = np.repeat(rng.choice([0.05, 1.0], len(t) // 120 + 1), 120)[: len(t)]
        signal = signal - band + band * gate
    return np.asarray(signal)


def test_the_sliced_fft_equals_the_direct_neighbourhood_sum() -> None:
    planes = np.random.default_rng(1).standard_normal((30, 50))
    weights = flicker.time_kernel()
    assert weights.sum() == pytest.approx(0.0)
    direct = np.array([
        [weights @ planes[t - 3 : t + 4, f] for f in range(50)] for t in range(3, 27)
    ])  # fmt: skip
    assert np.allclose(flicker.sliced_apply(planes, weights)[3:27], direct, atol=1e-3)


def test_gated_bands_dip_deeper_and_flicker_more() -> None:
    steady = flicker.measures(voice(2.0, gated=False))
    chopped = flicker.measures(voice(2.0, gated=True))
    assert steady is not None and chopped is not None
    assert chopped[0] < steady[0] - 3.0
    assert chopped[1] > steady[1] + 2.0


def test_silence_has_no_measure_and_no_verdict() -> None:
    assert flicker.measures(np.zeros(4000)) is None
    assert flicker.measures(np.zeros(100)) is None
    assert flicker.recording_report([np.zeros(flicker.RATE * 6)]) == {
        "windows": 0, "suppressionDepthDb": None, "flicker": None, "robotic": False,
    }  # fmt: skip


def test_the_recording_verdict_follows_the_threshold(monkeypatch: pytest.MonkeyPatch) -> None:
    parts = [voice(6.0, gated=False), voice(6.0, gated=False), np.zeros(flicker.RATE * 6)]
    monkeypatch.setattr(flicker, "measures", lambda _window: (-24.0, 11.0))
    assert flicker.recording_report(parts) == {
        "windows": 2, "suppressionDepthDb": -24.0, "flicker": 11.0, "robotic": True,
    }  # fmt: skip
    monkeypatch.setattr(flicker, "measures", lambda _window: (-21.0, 11.0))
    assert flicker.recording_report(parts)["robotic"] is False


def test_the_cli_decodes_and_prints_the_verdict(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    path = tmp_path / "voice.wav"
    with wave.open(str(path), "wb") as writer:
        writer.setnchannels(1)
        writer.setsampwidth(2)
        writer.setframerate(flicker.RATE)
        writer.writeframes((voice(7.0, gated=True) * 32767).astype("<i2").tobytes())
    assert flicker.main([str(path)]) == 0
    assert '"windows": 1' in capsys.readouterr().out
    with pytest.raises(subprocess.CalledProcessError):
        list(flicker.windows(str(tmp_path / "missing.wav")))


def test_a_correction_must_land_in_the_natural_depth_range(monkeypatch: pytest.MonkeyPatch) -> None:
    signal = voice(2.0, gated=True)
    monkeypatch.setattr(flicker, "measures", lambda _window: (-21.5, 10.0))
    assert flicker.correction_failures(signal, signal) == []
    monkeypatch.setattr(flicker, "measures", lambda _window: (-17.8, 10.0))
    assert flicker.correction_failures(signal, signal) == ["depth"]
    monkeypatch.setattr(flicker, "measures", lambda _window: None)
    assert flicker.correction_failures(signal, signal) == ["depth"]


def test_a_recoloured_or_clicking_correction_fails(monkeypatch: pytest.MonkeyPatch) -> None:
    signal = voice(2.0, gated=False)
    monkeypatch.setattr(flicker, "measures", lambda _window: (-21.5, 10.0))
    freqs = np.fft.rfftfreq(len(signal), 1 / flicker.RATE)
    tilted = np.fft.irfft(np.fft.rfft(signal) * np.where(freqs > 3000, 2.0, 1.0), len(signal))
    assert flicker.correction_failures(signal, tilted) == ["timbre"]
    clicked = signal.copy()
    clicked[[4000, 12000, 20000]] += 0.8
    assert flicker.correction_failures(signal, clicked) == ["clicks"]
