"""The speaker model ``replay.voice_repair``'s gate uses, from pinned files only.

microsoft/wavlm-base-plus-sv (revision ``feb593a6``), x-vector cosine similarity on raw 16 kHz
audio (its feature extractor does not normalize), on the CPU. It runs in
``edits/.venv-voice-repair`` (``--extra voice-repair``: torch, transformers, scipy); nothing is
downloaded. The repair itself is computation only (2026-10-29: the user rejected MossFormer2's
sound).
"""

from __future__ import annotations

import hashlib
import importlib
from collections.abc import Callable
from math import gcd
from pathlib import Path

import numpy as np
import numpy.typing as npt

from replay.core import ReplayError

Samples = npt.NDArray[np.float32]
Similarity = Callable[[Samples, Samples, int], float]
Resample = Callable[[Samples, int, int], npt.ArrayLike]

# The digests of the speaker model's local files (revision feb593a6, fetched 2026-10-20).
SPEAKER_FILES = {
    "config.json": "c6ac8c0ce55b18d4612677931036fe5ce78093e87361c552a173f00a0bb07514",
    "pytorch_model.bin": "e906bce2fa42fb497a1d1a9ecf81548adb7e03b12a5644e32d2f42f0d6500fad",
}
SPEAKER_RATE = 16_000
DEVICE = "cpu"  # ~20 ms an embedding: no GPU lease for the gate
CHUNK_BYTES = 1 << 20


def digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(CHUNK_BYTES):
            hasher.update(chunk)
    return hasher.hexdigest()


def verified_speaker(directory: Path) -> dict[str, str]:
    for name, expected in SPEAKER_FILES.items():
        if digest(directory / name) != expected:
            raise ReplayError(f"The speaker model file {name} does not match its pin")
    return dict(SPEAKER_FILES)


def resampled(resample: Resample, samples: Samples, source: int, target: int) -> Samples:
    """Polyphase resampling (``scipy.signal.resample_poly``, passed in: only the model
    environment has scipy)."""
    common = gcd(source, target)
    return np.asarray(resample(samples, target // common, source // common), np.float32)


def load_similarity(directory: Path) -> Similarity:
    """Cosine similarity of two clips' WavLM x-vectors (1 is the same voice)."""
    verified_speaker(directory)
    torch = importlib.import_module("torch")
    transformers = importlib.import_module("transformers")
    signal = importlib.import_module("scipy.signal")
    model = transformers.WavLMForXVector.from_pretrained(str(directory), local_files_only=True)
    model = model.to(DEVICE).eval()

    def embedding(samples: Samples, rate: int) -> npt.NDArray[np.float64]:
        audio = resampled(signal.resample_poly, samples, rate, SPEAKER_RATE)
        with torch.inference_mode():
            vector = model(torch.from_numpy(audio)[None].to(DEVICE)).embeddings[0]
            return np.asarray(vector.float().cpu().numpy(), np.float64)

    def similarity(first: Samples, second: Samples, rate: int) -> float:
        a, b = embedding(first, rate), embedding(second, rate)
        norm = float(np.linalg.norm(a) * np.linalg.norm(b))
        return float(a @ b / norm) if norm > 0 else 0.0

    return similarity
