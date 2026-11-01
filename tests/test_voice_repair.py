"""Voice repair on synthetic clipped speech-like signals: detection, AR first, model batches,
the two-criterion gate and the receipt."""

import json
import sys
import wave
from pathlib import Path

import numpy as np
import numpy.typing as npt
import pytest

from replay import voice_repair as repair
from replay import voice_repair_models as models
from replay.core import ReplayError

RATE = 16_000


def speech(seconds: float, *, seed: int = 0) -> npt.NDArray[np.float64]:
    """Voiced syllables (a 150 Hz harmonic series) separated by near silence."""
    rng = np.random.default_rng(seed)
    t = np.arange(int(seconds * RATE)) / RATE
    pitch = 150 + 10 * np.sin(2 * np.pi * 3 * t)
    phase = 2 * np.pi * np.cumsum(pitch) / RATE
    voiced = sum(np.sin(k * phase) / k for k in range(1, 8))
    envelope = np.clip(np.sin(2 * np.pi * 1.5 * t), 0, None) ** 0.5
    return np.asarray(0.25 * voiced * envelope + 1e-4 * rng.standard_normal(len(t)))


def write_wav(path: Path, samples: npt.NDArray[np.float64], *, width: int = 2) -> Path:
    stereo = np.stack((samples, samples), axis=1)
    scaled = np.clip(np.round(stereo * 32768), -32768, 32767).astype("<i2")
    with wave.open(str(path), "wb") as stream:
        stream.setnchannels(2)
        stream.setsampwidth(width)
        stream.setframerate(RATE)
        stream.writeframes(scaled.tobytes() if width == 2 else bytes(len(scaled) * 2 * width))
    return path


def cat(path: Path) -> list[str]:
    """A decode command that streams the WAV's PCM data (a stand-in for the mix decode)."""
    script = "import sys; sys.stdout.buffer.write(open(sys.argv[1], 'rb').read()[44:])"
    return [sys.executable, "-c", script, str(path)]


def same_voice(_first: models.Samples, _second: models.Samples, _rate: int) -> float:
    return 1.0


def other_voice(_first: models.Samples, _second: models.Samples, _rate: int) -> float:
    return 0.2


def clipped_recording(tmp_path: Path, *, gain: float, limit: float) -> tuple[Path, Path]:
    """A voice with flat tops where its mix (twice as loud) clipped at ``limit``."""
    mix = np.clip(gain * speech(4.0), -limit, limit)
    return write_wav(tmp_path / "vocals.wav", mix / 2), write_wav(tmp_path / "mix.wav", mix)


def quality(
    tops: int,
    hnr: float,
    flatness: float,
    full: int = 0,
    *,
    peak: float = 0.5,
    crest: float = 9.0,
    spread: float = 12.0,
) -> repair.Measured:
    frames = np.linspace(-1.0, 1.0, 9)
    return repair.Measured(
        repair.Quality(hnr, flatness, tops, full, peak, crest, spread),
        hnr + frames,
        flatness + frames,
    )


def test_the_ceiling_is_the_pile_up_over_the_amplitude_tail() -> None:
    rng = np.random.default_rng(1)
    levels = np.minimum(np.abs(rng.laplace(scale=3000, size=200_000)).astype(np.int64), 12000)
    histogram = np.bincount(levels, minlength=repair.HISTOGRAM_BINS)
    assert repair.ceiling(histogram) == 11968


def test_an_unclipped_tail_has_no_ceiling() -> None:
    rng = np.random.default_rng(1)
    levels = np.minimum(np.abs(rng.laplace(scale=3000, size=200_000)).astype(np.int64), 32767)
    assert repair.ceiling(np.bincount(levels, minlength=repair.HISTOGRAM_BINS)) is None


def test_a_histogram_without_a_tail_has_no_ceiling() -> None:
    sparse = np.zeros(repair.HISTOGRAM_BINS, dtype=np.int64)
    sparse[[100, 20000]] = 5
    assert repair.ceiling(sparse) is None
    assert repair.ceiling(np.zeros(repair.HISTOGRAM_BINS, dtype=np.int64)) is None


def test_runs_are_found_and_joined_across_blocks() -> None:
    flags = np.array([0, 1, 1, 0, 1, 1, 1], dtype=bool)
    assert repair.runs_of(flags).tolist() == [[1, 2], [4, 3]]
    parts = [np.array([[4, 3]]), np.array([[7, 2], [12, 1]])]
    assert repair.joined_runs(parts).tolist() == [[4, 5]]
    assert repair.joined_runs([]).shape == (0, 2)


def test_sparse_runs_keep_consecutive_samples_at_the_level() -> None:
    indices = np.array([3, 4, 5, 6, 9, 10, 11, 20, 21], dtype=np.int64)
    levels = np.array([9, 9, 9, 5, 9, 9, 9, 9, 9], dtype=np.int32)
    assert repair.sparse_runs(indices, levels, 9).tolist() == [[3, 3], [9, 3]]
    assert repair.sparse_runs(indices, levels, 10).shape == (0, 2)


def test_the_theil_sen_line_ignores_an_outlier() -> None:
    x = np.arange(6, dtype=np.float64)
    y = 2 * x + 1
    y[5] = 100
    assert repair.theil_sen(x, y).tolist() == [1.0, 3.0, 5.0, 7.0, 9.0, 11.0]


def test_ar_interpolation_restores_a_clipped_sinusoid_consistently() -> None:
    t = np.arange(800)
    clean = 0.4 * np.sin(2 * np.pi * t / 40) + 0.12 * np.sin(2 * np.pi * t / 13)
    clipped = np.clip(clean, -0.36, 0.36)
    missing = np.abs(clipped) >= 0.36
    restored = repair.interpolated(clipped, missing)
    assert np.abs(restored - clean).max() < 0.5 * np.abs(clipped - clean).max()
    assert bool(np.all(np.abs(restored[missing]) >= 0.36))
    assert bool(np.array_equal(restored[~missing], clipped[~missing]))


def test_interpolation_leaves_short_or_complete_windows_alone() -> None:
    short = np.ones(10)
    assert repair.interpolated(short, np.ones(10, dtype=bool)).tolist() == short.tolist()
    nothing = np.zeros(100)
    assert repair.interpolated(nothing, np.zeros(100, dtype=bool)).tolist() == nothing.tolist()
    assert repair.ar_coefficients(np.zeros(100)).tolist() == [1.0] + [0.0] * repair.AR_ORDER


def test_ar_repair_interpolates_each_run_in_both_channels() -> None:
    t = np.arange(600)
    clean = 0.4 * np.sin(2 * np.pi * t / 40)
    clipped = np.clip(clean, -0.36, 0.36)
    channels = np.stack((clipped, clipped), axis=1)
    runs = repair.runs_of(np.abs(clipped) >= 0.36)
    repaired = repair.ar_repaired(channels, runs + np.array([1000, 0]), 1000)
    assert float(np.abs(repaired[:, 0] - clean).max()) < 0.05
    assert bool(np.array_equal(repaired[:, 0], repaired[:, 1]))


def test_the_crossfade_ramps_both_ends() -> None:
    weights = repair.fade(400, RATE)
    assert weights[0] == pytest.approx(2.4095520e-05)
    assert weights[200] == 1.0
    assert weights[-1] == pytest.approx(2.4095520e-05)
    assert repair.fade(1, RATE).tolist() == [1.0]


def test_flat_tops_count_runs_of_equal_loud_samples() -> None:
    column = np.array([0.0, 0.5, 1.0, 1.0, 1.0, 0.2, 1.0, 1.0, 0.0])
    assert repair.flat_tops(column) == 3


def test_the_gate_keeps_an_improvement_above_both_criteria() -> None:
    before = quality(40, 5.0, -6.0)
    assert repair.verdict(before, quality(0, 5.0, -6.0), 0.99, (1.0, 1.0)) == (True, "improved")
    assert repair.verdict(before, quality(40, 5.4, -6.0), 0.99, (0.3, 0.3)) == (True, "improved")
    assert repair.verdict(before, quality(40, 5.0, -6.1), 0.99, (0.1, 0.1)) == (True, "improved")


def test_a_noise_level_gain_fails_the_absolute_margin() -> None:
    before = quality(4, 5.0, -6.0)
    assert repair.verdict(before, quality(4, 5.0, -6.011), 0.99, (0.0, 0.0)) == (
        False,
        "no improvement above the absolute margins",
    )


def test_a_gain_within_the_recordings_spread_fails_the_relative_criterion() -> None:
    before = quality(40, 5.0, -6.0)
    assert repair.verdict(before, quality(40, 5.4, -6.0), 0.99, (3.0, 3.0)) == (
        False,
        "no improvement above the recording-relative criterion",
    )
    assert repair.verdict(before, quality(32, 5.0, -6.0), 0.99, (1.0, 1.0)) == (
        False,
        "no improvement above the recording-relative criterion",
    )


def test_the_gate_refuses_what_gets_worse() -> None:
    before = quality(40, 5.0, -6.0)
    assert repair.verdict(before, quality(0, 4.0, -6.0), 0.99, (0.1, 0.1)) == (
        False,
        "harmonicity fell",
    )
    assert repair.verdict(before, quality(0, 4.7, -6.0), 0.99, (0.1, 0.1)) == (
        False,
        "harmonicity fell beyond the recording's spread",
    )
    assert repair.verdict(before, quality(0, 5.0, -5.8), 0.99, (0.1, 0.1)) == (
        False,
        "flatness rose",
    )
    assert repair.verdict(before, quality(0, 5.0, -5.94), 0.99, (0.01, 0.01)) == (
        False,
        "flatness rose beyond the recording's spread",
    )
    assert repair.verdict(quality(0, 5.0, -6.0), quality(3, 5.5, -6.0), 0.99, (0.1, 0.1)) == (
        False,
        "more flat tops",
    )
    assert repair.verdict(before, quality(0, 5.0, -6.0, 1), 0.99, (0.1, 0.1)) == (
        False,
        "new full-scale samples",
    )
    assert repair.verdict(before, quality(0, 5.0, -6.0), 0.5, (0.1, 0.1)) == (
        False,
        "speaker similarity below the threshold",
    )


def test_the_gate_refuses_a_new_peak_or_flattened_dynamics() -> None:
    before = quality(40, 5.0, -6.0)
    assert repair.verdict(before, quality(0, 5.0, -6.0, peak=0.6), 0.99, (0.1, 0.1)) == (
        False,
        "a new peak above the ceiling",
    )
    assert repair.verdict(before, quality(0, 5.0, -6.0, peak=0.6), 0.99, (0.1, 0.1), 0.6) == (
        True,
        "improved",
    )
    assert repair.verdict(before, quality(0, 5.0, -6.0, crest=7.0), 0.99, (0.1, 0.1)) == (
        False,
        "the crest factor collapsed",
    )
    assert repair.verdict(before, quality(0, 5.0, -6.0, spread=9.0), 0.99, (0.1, 0.1)) == (
        False,
        "the short-term loudness range changed",
    )


def test_mild_clipping_is_repaired_by_ar_and_the_compressor(
    tmp_path: Path,
) -> None:
    vocals, mix = clipped_recording(tmp_path, gain=1.2, limit=0.27)
    result = repair.run(vocals, tmp_path / "out", cat(mix), same_voice, {"m": 1})
    output = tmp_path / "out/vocals.wav"
    assert result["repaired"] is True
    assert result["ceiling"] == 8832
    assert result["voice"] == {"path": str(output), "sha256": repair.digest(output)}
    assert result["verification"] == {"frames": 64000, "differingOutsideSegments": 0}
    summary = result["summary"]
    assert isinstance(summary, dict)
    assert summary["byStage"] == [
        {
            "tried": "ar+compressor",
            "kept": True,
            "reason": "improved",
            "segments": 6,
            "seconds": 2.14,
        },
    ]
    before, after = repair.pcm(vocals).samples(), repair.pcm(output).samples()
    assert bool(np.array_equal(before[5440:10560], after[5440:10560]))
    assert not np.array_equal(before[:5440], after[:5440])
    receipt = json.loads((tmp_path / "out/receipt.json").read_text(encoding="utf-8"))
    assert receipt["models"] == {"m": 1}
    assert receipt["qualityApproved"] is False
    assert set(receipt["timings"]) == {
        "detectSeconds", "arSeconds", "compressorAndGateSeconds", "totalSeconds", "audioSeconds",
    }  # fmt: skip
    again = repair.run(vocals, tmp_path / "out", ["false"], other_voice, {})
    assert again == receipt  # the receipt is reused, nothing is decoded again


def test_a_repair_of_another_voice_is_not_kept_and_writes_no_copy(tmp_path: Path) -> None:
    vocals, mix = clipped_recording(tmp_path, gain=1.2, limit=0.27)
    result = repair.run(vocals, tmp_path / "out", cat(mix), other_voice, {})
    assert result["repaired"] is False
    assert result["kept"] == 0
    assert result["voice"] == {"path": str(vocals), "sha256": repair.digest(vocals)}
    assert result["verification"] == {}
    assert not (tmp_path / "out/vocals.wav").exists()


def test_an_unclipped_recording_has_no_segments(tmp_path: Path) -> None:
    vocals = write_wav(tmp_path / "vocals.wav", speech(2.0))
    result = repair.run(vocals, tmp_path / "out", cat(vocals), same_voice, {})
    assert (result["ceiling"], result["clippedRuns"], result["segments"]) == (None, 0, [])
    assert result["repaired"] is False


def test_a_mix_of_another_length_is_refused(tmp_path: Path) -> None:
    vocals = write_wav(tmp_path / "vocals.wav", speech(1.0))
    mix = write_wav(tmp_path / "mix.wav", speech(0.5))
    with pytest.raises(ReplayError, match="differ in length"):
        repair.mix_clipping(cat(mix), repair.pcm(vocals))


def test_a_failed_or_torn_decode_is_refused() -> None:
    with pytest.raises(ReplayError, match="failed with status 1"):
        list(repair.decoded_blocks(["false"], 2, 10))
    torn = [sys.executable, "-c", "import sys; sys.stdout.buffer.write(b'abc')"]
    with pytest.raises(ReplayError, match="ended inside a frame"):
        list(repair.decoded_blocks(torn, 2, 10))


def test_only_16_bit_pcm_is_read(tmp_path: Path) -> None:
    path = write_wav(tmp_path / "wide.wav", speech(0.1), width=4)
    with pytest.raises(ReplayError, match="16-bit PCM only"):
        repair.pcm(path)


def test_a_repair_that_changed_samples_outside_its_segments_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    vocals, mix = clipped_recording(tmp_path, gain=1.2, limit=0.27)
    monkeypatch.setattr(repair, "differing_outside", lambda _voice, _output, _kept: 3)
    with pytest.raises(ReplayError, match=r"differs outside its segments \(3 samples\)"):
        repair.run(vocals, tmp_path / "out", cat(mix), same_voice, {})
    assert not (tmp_path / "out/vocals.wav").exists()


def test_a_repair_that_changed_the_layout_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    vocals, mix = clipped_recording(tmp_path, gain=1.2, limit=0.27)
    voice = repair.pcm(vocals)
    found = repair.detect(voice, cat(mix))
    fixed = repair.judged(voice.samples(), found, same_voice, RATE)
    real = repair.pcm
    calls: list[Path] = []

    def changed(path: Path) -> repair.Pcm:
        calls.append(path)
        return real(path) if len(calls) == 1 else repair.Pcm(path, voice.offset, 1, 2, RATE)

    monkeypatch.setattr(repair, "pcm", changed)
    with pytest.raises(ReplayError, match="changed its length, channels or rate"):
        repair.written_voice(voice, fixed, tmp_path / "vocals-out.wav")


def test_the_cli_decodes_the_mix_like_separation_and_pins_the_speaker_model(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    vocals = write_wav(tmp_path / "vocals.wav", speech(1.0))
    seen: dict[str, object] = {}

    def recorded(
        voice: Path, directory: Path, command: list[str], similarity: object, pins: object
    ) -> dict[str, object]:
        seen.update(voice=voice, directory=directory, command=command, pins=pins)
        return {"repaired": False, "kept": 0, "voice": {"path": str(voice)}, "summary": {}}

    monkeypatch.setattr(models, "verified_speaker", lambda path: {"s": str(path)})
    monkeypatch.setattr(models, "load_similarity", lambda _path: same_voice)
    monkeypatch.setattr(repair, "run", recorded)
    arguments = [str(vocals), str(tmp_path / "out"), "--source", "/s.mp4", "--gain-db", "-0.1",
                 "--ffmpeg", "/ffmpeg", "--speaker", "/wavlm"]  # fmt: skip
    assert repair.main(arguments) == 0
    assert seen["command"] == [
        "/ffmpeg", "-nostdin", "-hide_banner", "-v", "error", "-n", "-i", "/s.mp4",
        "-map", "0:a:0", "-vn", "-af", "asetpts=N/SR/TB,aresample=44100,volume=-0.100000dB",
        "-c:a", "pcm_s16le", "-f", "s16le", "pipe:1",
    ]  # fmt: skip
    assert seen["pins"] == {"speaker": {"path": "/wavlm", "files": {"s": "/wavlm"}}}
    assert json.loads(capsys.readouterr().out) == {
        "repaired": False, "kept": 0, "voice": {"path": str(vocals)}, "summary": {},
    }  # fmt: skip
