"""The computed voice judge: two-sided calibration on synthetic degradations, and its CLI."""

from __future__ import annotations

import json
import subprocess
import wave
from pathlib import Path

import numpy as np
import numpy.typing as npt
import pytest

from replay import voice_compressor as compressor
from replay import voice_deesser as deesser
from replay import voice_judge as judge
from replay import voice_repair as repair
from replay import voice_tempo as tempo
from replay.core import ReplayError

Floats = npt.NDArray[np.float64]
RATE = 44_100
FFMPEG = "/opt/homebrew/bin/ffmpeg"


def speech(seconds: float = 4.0) -> Floats:
    """Jittered glottal pulses through three formants, in syllables, with fricative hiss."""
    rng = np.random.default_rng(0)
    count = int(seconds * RATE)
    t = np.arange(count) / RATE
    f0 = 150 + 50 * np.sin(2 * np.pi * 0.3 * t) + rng.standard_normal(count).cumsum() * 0.02
    pulses = np.diff(np.floor(np.cumsum(f0 / RATE)), prepend=0) * (
        1 + 0.05 * rng.standard_normal(count)
    )
    source = pulses
    z = np.exp(-2j * np.pi * np.fft.rfftfreq(count))
    poles = np.exp(-np.pi * np.array([90, 110, 160]) / RATE)
    angles = 2 * np.pi * np.array([700, 1200, 2600]) / RATE
    resonators = (1 - poles) / (
        1 - 2 * poles * np.cos(angles) * z[:, None] + poles**2 * z[:, None] ** 2
    )
    source = np.fft.irfft(np.fft.rfft(source) * resonators.prod(axis=1) / (1 - 0.97 * z), count)
    envelope = np.clip(np.sin(2 * np.pi * 1.7 * t) * 1.5, 0, 1) ** 0.5
    hiss = np.diff(rng.standard_normal(count + 1)) * (np.sin(2 * np.pi * 0.9 * t + 1) > 0.9)
    voice = source / np.max(np.abs(source)) * 0.3 * envelope + 0.05 * hiss
    return np.asarray(voice + 0.0005 * rng.standard_normal(count))


def stereo(mono: Floats) -> Floats:
    return np.stack([mono, mono], axis=1)


def write(path: Path, mono: Floats) -> Path:
    with wave.open(str(path), "wb") as writer:
        writer.setnchannels(2)
        writer.setsampwidth(2)
        writer.setframerate(RATE)
        pcm = np.clip(stereo(mono) * 32768, -32768, 32767).astype("<i2")
        writer.writeframes(pcm.tobytes())
    return path


def read(path: Path) -> Floats:
    return judge.mono(repair.pcm(path).samples())


def atempo(tmp_path: Path, voice: Floats, ratio: float) -> Floats:
    source, output = write(tmp_path / "in.wav", voice), tmp_path / "atempo.wav"
    subprocess.run(
        [FFMPEG, "-nostdin", "-v", "error", "-i", str(source), "-af", f"atempo={ratio:.17g}",
         "-c:a", "pcm_s16le", str(output)],
        check=True,
    )  # fmt: skip
    return read(output)


def retimed_reference(voice: Floats, ratio: float, frames: int) -> Floats:
    return judge.mono(tempo.resampled(stereo(voice) * 32768, 0.0, ratio, frames))


def test_a_gain_change_passes() -> None:
    voice = speech()
    assert judge.failures(judge.scored(voice, 0.5 * voice, RATE)) == []


def test_the_natural_compressor_passes() -> None:
    voice = speech()
    made = compressor.compressed(stereo(voice), stereo(voice), RATE, -20.0)[:, 0]
    assert judge.failures(judge.scored(voice, made, RATE)) == []


def test_the_deesser_passes(tmp_path: Path) -> None:
    voice = speech()
    t = np.arange(len(voice)) / RATE
    hiss = np.random.default_rng(1).standard_normal(len(voice))
    voice = voice + (np.sin(2 * np.pi * 2 * t) > 0.95) * 0.25 * np.sin(2 * np.pi * 6000 * t) * hiss
    deesser.run(write(tmp_path / "voice.wav", voice), tmp_path / "deessed")
    reference = read(tmp_path / "voice.wav")
    made = read(tmp_path / "deessed" / "vocals.wav")
    assert judge.failures(judge.scored(reference, made, RATE)) == []


def test_resampling_by_a_clock_correction_passes() -> None:
    voice = speech()
    made = tempo.resampled(stereo(voice), 0.0, 1.00001, 176_398)[:, 0]
    reference = retimed_reference(voice, 1.00001, 176_398)
    assert judge.failures(judge.scored(reference, made, RATE)) == []


def test_resampling_by_two_percent_passes() -> None:
    voice = speech()
    made = tempo.resampled(stereo(voice), 0.0, 1.02, 172_941)[:, 0]
    reference = retimed_reference(voice, 1.02, 172_941)
    assert judge.failures(judge.scored(reference, made, RATE)) == []


def test_injected_clicks_fail() -> None:
    voice = speech()
    rng = np.random.default_rng(2)
    at = rng.integers(1000, len(voice) - 1000, 8)
    signs = rng.choice([-1.0, 1.0], 8)
    clicked = voice.copy()
    np.add.at(clicked, (at[:, None] + np.arange(3)).ravel(), np.repeat(0.3 * signs, 3))
    score = judge.scored(voice, clicked, RATE)
    assert judge.failures(score) == ["fluxExcessPerSecond", "newClicksPerSecond"]
    assert score.snrP50Db == 120.0


def test_clipping_fails() -> None:
    voice = speech()
    assert judge.failures(judge.scored(voice, np.clip(voice, -0.12, 0.12), RATE)) == [
        "snrP50Db",
        "newClicksPerSecond",
        "clippedExcess",
        "timbreShiftDb",
    ]


def test_atempo_at_a_clock_correction_fails(tmp_path: Path) -> None:
    voice = speech()
    made = atempo(tmp_path, voice, 1.00001)
    reference = retimed_reference(voice, 1.00001, len(made) - 200)
    score = judge.scored(reference, made[: len(made) - 200], RATE)
    assert judge.failures(score) == ["snrP50Db", "snrP10Db"]
    assert score.snrP10Db < 1.0


def test_atempo_by_two_percent_fails(tmp_path: Path) -> None:
    voice = speech()
    made = atempo(tmp_path, voice, 1.02)
    reference = retimed_reference(voice, 1.02, len(made) - 200)
    score = judge.scored(reference, made[: len(made) - 200], RATE)
    assert judge.failures(score) == [
        "snrP50Db", "snrP10Db", "newClicksPerSecond", "timbreShiftDb",
    ]  # fmt: skip


def test_a_recoloured_voice_fails() -> None:
    voice = speech()
    freqs = np.fft.rfftfreq(len(voice), 1 / RATE)
    gain = np.where(freqs > 8000, 10 ** (3 / 20), np.where(freqs > 4000, 10 ** (-3 / 20), 1.0))
    recoloured = np.fft.irfft(np.fft.rfft(voice) * gain, len(voice))
    score = judge.scored(voice, recoloured, RATE)
    assert "timbreShiftDb" in judge.failures(score)
    assert judge.timbre_shift(voice[:10], voice[:10]) == 0.0


def test_short_or_silent_windows_measure_as_unchanged() -> None:
    silence = np.zeros(RATE // 100)
    assert judge.coherence(silence) == 1.0
    assert judge.flux_events(silence, RATE) == 0
    assert judge.clipped_share(silence) == 0.0
    assert judge.clicks(judge.residual_sigmas(np.zeros(RATE), RATE), RATE).tolist() == []
    assert judge.coherence(np.zeros(4096)) == 1.0
    assert judge.scored(silence, silence, RATE).snrP10Db == 120.0


def test_new_clicks_count_against_an_empty_reference_reach() -> None:
    voice = speech(0.5)
    clicked = voice.copy()
    clicked[10_000:10_003] += 0.3
    assert judge.new_clicks(voice[:10_000], clicked, RATE) == 1


def test_tempo_windows_take_the_longest_stretched_slots() -> None:
    cuts = [
        {"source_start_sample": 0, "source_end_sample": 400_000,
         "output_start_sample": 1_000, "output_end_sample": 401_000},
        {"source_start_sample": 400_000, "source_end_sample": 800_010,
         "output_start_sample": 401_000, "output_end_sample": 801_000},
        {"source_start_sample": 800_010, "source_end_sample": 800_110,
         "output_start_sample": 801_000, "output_end_sample": 801_090},
    ]  # fmt: skip
    assert judge.tempo_windows(cuts, RATE) == [
        judge.Window(output_start=423_600, reference_start=423_600.59, ratio=1.000025)
    ]
    assert judge.tempo_windows([], RATE) == []


def test_same_windows_spread_evenly() -> None:
    assert judge.same_windows(1_000_000, RATE, 3) == [
        judge.Window(0, 0, 1.0),
        judge.Window(323_600, 323_600, 1.0),
        judge.Window(647_200, 647_200, 1.0),
    ]


def test_the_same_cli_passes_an_unchanged_voice(tmp_path: Path) -> None:
    voice = write(tmp_path / "voice.wav", speech(9.0))
    report = tmp_path / "report.json"
    assert judge.main(["same", str(voice), str(voice), str(report)]) == 0
    data = json.loads(report.read_text(encoding="utf-8"))
    assert data["passed"] is True
    assert len(data["windows"]) == 24
    assert data["limits"]["snrP10Db"] == ["min", 8.0]


def test_the_same_cli_fails_a_clipped_voice(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    voice = speech(9.0)
    source = write(tmp_path / "voice.wav", voice)
    clipped = write(tmp_path / "clipped.wav", np.clip(voice, -0.12, 0.12))
    report = tmp_path / "report.json"
    assert judge.main(["same", str(source), str(clipped), str(report)]) == 1
    assert json.loads(report.read_text(encoding="utf-8"))["passed"] is False
    assert capsys.readouterr().err == "voice judge anomaly: 24 of 24 windows failed\n"


def test_the_tempo_cli_judges_the_resampled_slots(tmp_path: Path) -> None:
    source = write(tmp_path / "voice.wav", speech(9.0))
    plan = tmp_path / "plan.json"
    plan.write_text(json.dumps({"sampleRate": RATE, "cuts": [
        {"source_start_sample": 0, "source_end_sample": 396_004,
         "output_start_sample": 0, "output_end_sample": 396_000}]}), encoding="utf-8")  # fmt: skip
    receipt = tempo.run(source, plan, tmp_path / "tempo")
    report = tmp_path / "report.json"
    candidate = tmp_path / "tempo" / "candidate.wav"
    assert judge.main(["tempo", str(source), str(candidate), str(receipt), str(report)]) == 0
    assert (
        json.loads(report.read_text(encoding="utf-8"))["windows"][0]["outputSeconds"]
        == 0.4897959183673469
    )


def test_a_tempo_receipt_without_cuts_is_refused(tmp_path: Path) -> None:
    source = write(tmp_path / "voice.wav", speech(1.0))
    receipt = tmp_path / "receipt.json"
    receipt.write_text(json.dumps({"cuts": {}}), encoding="utf-8")
    with pytest.raises(ReplayError, match="no cuts"):
        judge.main(["tempo", str(source), str(source), str(receipt), str(tmp_path / "r.json")])


def test_a_changed_rate_or_no_window_is_refused(tmp_path: Path) -> None:
    source = write(tmp_path / "voice.wav", speech(1.0))
    other = tmp_path / "other.wav"
    with wave.open(str(other), "wb") as writer:
        writer.setnchannels(2)
        writer.setsampwidth(2)
        writer.setframerate(48_000)
        writer.writeframes(bytes(4 * 48_000))
    with pytest.raises(ReplayError, match="sample rate"):
        judge.judged(source, other, [])
    with pytest.raises(ReplayError, match="No window"):
        judge.judged(source, source, judge.same_windows(RATE, RATE))
