"""The voice repair's speaker model wrapper: pinned files, offline, CPU (fake libraries)."""

import hashlib
import importlib
import types
from contextlib import nullcontext
from pathlib import Path

import numpy as np
import numpy.typing as npt
import pytest

from replay import voice_repair_models as models
from replay.core import ReplayError


class FakeTensor:
    def __init__(self, values: npt.ArrayLike) -> None:
        self.values = np.asarray(values, dtype=np.float64)

    def __getitem__(self, key: int | slice | tuple[int | slice | None, ...] | None) -> "FakeTensor":
        return FakeTensor(self.values[key])

    def to(self, _device: str) -> "FakeTensor":
        return self

    def float(self) -> "FakeTensor":
        return self

    def cpu(self) -> "FakeTensor":
        return self

    def numpy(self) -> npt.NDArray[np.float64]:
        return self.values


def resample_poly(values: npt.NDArray[np.float32], up: int, down: int) -> npt.NDArray[np.float64]:
    return np.asarray(values, np.float64)[np.arange(len(values) * up // down) * down // up]


def fake_libraries(monkeypatch: pytest.MonkeyPatch) -> dict[str, object]:
    seen: dict[str, object] = {}

    def xvector(path: str, local_files_only: bool) -> object:
        seen["speaker"] = (Path(path).name, local_files_only)

        def embed(audio: FakeTensor) -> object:
            values = audio.values[0]
            return types.SimpleNamespace(
                embeddings=FakeTensor(np.array([[values.std(), values.mean()]]))
            )

        model = types.SimpleNamespace(eval=lambda: embed)
        return types.SimpleNamespace(to=lambda device: seen.update(device=device) or model)

    modules = {
        "scipy.signal": types.SimpleNamespace(resample_poly=resample_poly),
        "torch": types.SimpleNamespace(inference_mode=nullcontext, from_numpy=FakeTensor),
        "transformers": types.SimpleNamespace(
            WavLMForXVector=types.SimpleNamespace(from_pretrained=xvector)
        ),
    }
    monkeypatch.setattr(importlib, "import_module", modules.__getitem__)
    return seen


def test_the_speaker_model_must_match_its_pins(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    (tmp_path / "config.json").write_bytes(b"c")
    monkeypatch.setattr(models, "SPEAKER_FILES", {"config.json": hashlib.sha256(b"c").hexdigest()})
    assert models.verified_speaker(tmp_path) == {
        "config.json": "2e7d2c03a9507ae265ecf5b5356885a53393a2029d241394997265a1a25aefc6"
    }
    (tmp_path / "config.json").write_bytes(b"d")
    with pytest.raises(ReplayError, match="config.json does not match its pin"):
        models.verified_speaker(tmp_path)


def test_similarity_is_the_cosine_of_the_x_vectors_on_the_cpu(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    seen = fake_libraries(monkeypatch)
    monkeypatch.setattr(models, "SPEAKER_FILES", {})
    similarity = models.load_similarity(tmp_path)
    first = np.array([1.0, -1.0, 1.0, -1.0], dtype=np.float32)
    assert similarity(first, first, 16_000) == pytest.approx(1.0)
    assert similarity(first, np.zeros(4, dtype=np.float32), 16_000) == 0.0
    assert seen == {"speaker": (tmp_path.name, True), "device": "cpu"}


def test_resampling_uses_the_given_polyphase_function() -> None:
    def resample(values: models.Samples, up: int, down: int) -> npt.ArrayLike:
        return values[:: down // up]

    shorter = models.resampled(resample, np.arange(6, dtype=np.float32), 48_000, 16_000)
    assert shorter.tolist() == [0.0, 3.0]
