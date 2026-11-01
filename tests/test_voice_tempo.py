"""Cut-slot tempo by band-limited resampling."""

from __future__ import annotations

import json
import wave
from pathlib import Path

import numpy as np
import pytest

from replay import voice_repair as repair
from replay import voice_tempo as tempo
from replay.core import ReplayError

RATE = 44_100


def write(path: Path, frames: int) -> Path:
    t = np.arange(frames) / RATE
    tone = (8000 * np.sin(2 * np.pi * 220 * t)).astype("<i2")
    with wave.open(str(path), "wb") as writer:
        writer.setnchannels(2)
        writer.setsampwidth(2)
        writer.setframerate(RATE)
        writer.writeframes(np.stack([tone, -tone], axis=1).tobytes())
    return path


def plan(path: Path, cuts: list[dict[str, int]], rate: int = RATE) -> Path:
    path.write_text(json.dumps({"sampleRate": rate, "cuts": cuts}), encoding="utf-8")
    return path


def cut(low: int, high: int, start: int, end: int) -> dict[str, int]:
    return {"source_start_sample": low, "source_end_sample": high,
            "output_start_sample": start, "output_end_sample": end}  # fmt: skip


def test_unit_ratio_interpolation_reproduces_the_samples() -> None:
    samples = np.arange(200, dtype=np.float64).reshape(100, 2)
    result = tempo.resampled(samples, 40.0, 1.0, 10)
    assert np.allclose(result, samples[40:50])


def test_positions_outside_the_source_read_as_silence() -> None:
    samples = np.ones((50, 1))
    result = tempo.resampled(samples, 1000.0, 1.0, 4)
    assert result.tolist() == [[0.0], [0.0], [0.0], [0.0]]


def test_slots_are_copied_or_resampled_to_their_exact_length(tmp_path: Path) -> None:
    voice = write(tmp_path / "voice.wav", 30_000)
    spans = [cut(0, 10_000, 500, 10_500), cut(10_000, 20_010, 10_500, 20_500)]
    receipt = tempo.run(voice, plan(tmp_path / "plan.json", spans), tmp_path / "out")
    data = json.loads(receipt.read_text(encoding="utf-8"))
    made = repair.pcm(tmp_path / "out" / "candidate.wav")
    source = repair.pcm(voice).samples()
    assert made.frames == 20_000
    assert np.array_equal(made.samples()[:10_000], source[:10_000])
    assert data["kind"] == "sinc-resample-exact-cut-slot-candidate"
    assert data["outputStartSample"] == 500
    assert data["outputEndSample"] == 20_500
    assert data["clippedSamples"] == 0
    assert [entry["resampled"] for entry in data["cuts"]] == [False, True]
    assert tempo.run(voice, tmp_path / "missing.json", tmp_path / "out") == receipt


def test_full_scale_overshoot_is_clipped_and_counted(tmp_path: Path) -> None:
    voice = tmp_path / "voice.wav"
    square = np.where(np.arange(4_000) % 40 < 20, 32767, -32768).astype("<i2")
    with wave.open(str(voice), "wb") as writer:
        writer.setnchannels(1)
        writer.setsampwidth(2)
        writer.setframerate(RATE)
        writer.writeframes(square.tobytes())
    receipt = tempo.run(voice, plan(tmp_path / "plan.json", [cut(0, 4_000, 0, 3_990)]),
                        tmp_path / "out")  # fmt: skip
    assert json.loads(receipt.read_text(encoding="utf-8"))["clippedSamples"] > 0


def test_invalid_plans_are_refused(tmp_path: Path) -> None:
    voice = write(tmp_path / "voice.wav", 1_000)
    with pytest.raises(ReplayError, match="no cuts"):
        tempo.run(voice, plan(tmp_path / "a.json", []), tmp_path / "a")
    with pytest.raises(ReplayError, match="not contiguous"):
        tempo.run(voice, plan(tmp_path / "b.json", [cut(0, 10, 0, 10), cut(10, 20, 11, 21)]),
                  tmp_path / "b")  # fmt: skip
    with pytest.raises(ReplayError, match="empty slot"):
        tempo.run(voice, plan(tmp_path / "c.json", [cut(0, 0, 0, 10)]), tmp_path / "c")
    with pytest.raises(ReplayError, match="does not match"):
        tempo.run(voice, plan(tmp_path / "d.json", [cut(0, 10, 0, 10)], 48_000), tmp_path / "d")
    with pytest.raises(ReplayError, match="does not match"):
        tempo.run(voice, plan(tmp_path / "e.json", [cut(0, 2_000, 0, 2_000)]), tmp_path / "e")


def test_an_output_over_the_wav_limit_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    voice = write(tmp_path / "voice.wav", 1_000)
    monkeypatch.setattr(tempo, "MAX_WAVE_BYTES", 100)
    with pytest.raises(ReplayError, match="4 GiB"):
        tempo.run(voice, plan(tmp_path / "plan.json", [cut(0, 100, 0, 100)]), tmp_path / "out")


def test_the_cli_replaces_a_render_cut_short(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    voice = write(tmp_path / "voice.wav", 1_000)
    output = tmp_path / "out"
    output.mkdir()
    (output / ".candidate.wav.partial").write_bytes(b"cut short")
    spans = plan(tmp_path / "plan.json", [cut(0, 1_000, 0, 990)])
    assert tempo.main([str(voice), str(spans), str(output)]) == 0
    assert json.loads(capsys.readouterr().out) == {"receipt": str(output / "receipt.json")}
    assert sorted(path.name for path in output.iterdir()) == ["candidate.wav", "receipt.json"]
