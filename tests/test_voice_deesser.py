"""The de-esser on synthetic voice with sibilant bursts: band split, threshold, bit-identity."""

import json
import wave
from pathlib import Path

import numpy as np
import numpy.typing as npt
import pytest

from replay import voice_deesser as deesser
from replay import voice_repair as repair
from replay.core import ReplayError

RATE = 44_100


def voice_with_sibilants(seconds: float) -> npt.NDArray[np.float64]:
    """A voiced tone with loud 6 kHz noise bursts (sibilants) twice a second."""
    rng = np.random.default_rng(0)
    t = np.arange(int(seconds * RATE)) / RATE
    vowel = 0.2 * np.sin(2 * np.pi * 150 * t) * (np.sin(2 * np.pi * 0.5 * t) > -0.5)
    hiss = np.convolve(rng.standard_normal(len(t)), np.ones(3) / 3, "same")
    carrier = np.sin(2 * np.pi * 6000 * t)
    bursts = (np.sin(2 * np.pi * 2 * t) > 0.95) * 0.25 * hiss * carrier
    quiet = 0.01 * hiss * carrier
    return np.asarray(vowel + bursts + quiet)


def write(path: Path, mono: npt.NDArray[np.float64]) -> Path:
    stereo = np.clip(np.round(np.stack((mono, mono), axis=1) * 32768), -32768, 32767)
    with wave.open(str(path), "wb") as stream:
        stream.setnchannels(2)
        stream.setsampwidth(2)
        stream.setframerate(RATE)
        stream.writeframes(stereo.astype("<i2").tobytes())
    return path


def test_the_band_and_its_complement_sum_back_to_the_voice() -> None:
    signal = np.stack((voice_with_sibilants(0.5),) * 2, axis=1)
    low, high = deesser.split(signal, RATE)
    assert np.allclose(low + high, signal, atol=1e-15)
    tone = np.stack((np.sin(2 * np.pi * 300 * np.arange(RATE) / RATE),) * 2, axis=1)
    assert float(np.abs(deesser.split(tone, RATE)[1][1000:-1000]).max()) < 0.01


def test_the_threshold_is_the_audible_ninetieth_percentile_plus_the_margin() -> None:
    peaks = np.concatenate((np.full(90, 0.01), np.full(10, 0.1), np.full(50, 1e-9)))
    assert deesser.threshold_db(peaks) == pytest.approx(-35.0)  # p90 -38 dB, +3
    assert deesser.threshold_db(np.full(10, 1e-4)) == -40.0  # the fixed floor
    with pytest.raises(ReplayError, match="no audible sibilant band"):
        deesser.threshold_db(np.zeros(0))


def test_sibilant_bursts_are_turned_down_and_everything_else_is_bit_identical(
    tmp_path: Path,
) -> None:
    source = write(tmp_path / "voice.wav", voice_with_sibilants(3.0))
    result = deesser.run(source, tmp_path / "out")
    output = tmp_path / "out/vocals.wav"
    assert result["voice"] == {"path": str(output), "sha256": repair.digest(output)}
    assert 0 < float(str(result["maxReductionDb"])) <= deesser.MAX_REDUCTION_DB
    before = np.asarray(repair.pcm(source).samples(), np.float64) / 32768
    after = np.asarray(repair.pcm(output).samples(), np.float64) / 32768
    high_before = deesser.split(before, RATE)[1]
    high_after = deesser.split(after, RATE)[1]
    assert float(np.abs(high_after).max()) < 0.75 * float(np.abs(high_before).max())
    changed = (before != after).any(axis=1)
    assert 0 < changed.mean() < 0.9
    low_before, low_after = deesser.split(before, RATE)[0], deesser.split(after, RATE)[0]
    # Vowels keep their sound: only the filter's transition band leaks (-36 dB of the peak).
    assert float(np.abs(low_after - low_before).max()) < 0.01
    receipt = json.loads((tmp_path / "out/receipt.json").read_text(encoding="utf-8"))
    assert receipt["changedSeconds"] == round(int(changed.sum()) / RATE, 3)
    assert deesser.run(source, tmp_path / "out") == receipt  # reused


def test_a_voice_without_sibilants_is_copied_unchanged(tmp_path: Path) -> None:
    t = np.arange(RATE) / RATE
    source = write(tmp_path / "voice.wav", 0.2 * np.sin(2 * np.pi * 150 * t))
    result = deesser.run(source, tmp_path / "out")
    assert result["changedSeconds"] == 0.0
    assert repair.digest(tmp_path / "out/vocals.wav") == repair.digest(source)


def test_a_changed_layout_is_refused(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    source = write(tmp_path / "voice.wav", voice_with_sibilants(1.0))
    voice = repair.pcm(source)
    real = repair.pcm
    calls: list[Path] = []

    def changed(path: Path) -> repair.Pcm:
        calls.append(path)
        return real(path) if len(calls) == 1 else repair.Pcm(path, voice.offset, 1, 2, RATE)

    monkeypatch.setattr(repair, "pcm", changed)
    with pytest.raises(ReplayError, match="changed its length"):
        deesser.deessed(voice, tmp_path / "out.wav")


def test_the_cli_writes_the_receipt(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    source = write(tmp_path / "voice.wav", voice_with_sibilants(1.0))
    assert deesser.main([str(source), str(tmp_path / "out")]) == 0
    printed = json.loads(capsys.readouterr().out)
    assert set(printed) == {"voice", "thresholdDb", "changedSeconds"}
