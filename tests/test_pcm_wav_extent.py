import struct
from pathlib import Path

import pytest

from replay.core import ReplayError
from replay.pcm_wav_extent import PcmExtent, pcm_extent


def _fmt(channels: int = 2, width: int = 2, block: int | None = None) -> bytes:
    align = channels * width if block is None else block
    body = struct.pack("<HHIIHH", 1, channels, 48000, 48000 * align, align, width * 8)
    return b"fmt " + struct.pack("<I", len(body)) + body


def _wav(path: Path, *chunks: bytes, payload: bytes = b"") -> Path:
    path.write_bytes(b"RIFF\0\0\0\0WAVE" + b"".join(chunks) + payload)
    return path


def test_reads_extent_after_skipping_other_chunks(tmp_path: Path) -> None:
    path = _wav(
        tmp_path / "a.wav",
        b"LIST" + struct.pack("<I", 3) + b"abc\0",
        _fmt(),
        b"data" + struct.pack("<I", 8),
        payload=bytes(8),
    )
    assert pcm_extent(path) == PcmExtent(offset=56, frames=2, channels=2, width=2)


def test_placeholder_data_size_uses_file_size(tmp_path: Path) -> None:
    path = _wav(
        tmp_path / "a.wav", _fmt(), b"data" + struct.pack("<I", 0xFFFFFFFF), payload=bytes(12)
    )
    assert pcm_extent(path).frames == 3


def test_rejects_non_wave(tmp_path: Path) -> None:
    path = tmp_path / "a.wav"
    path.write_bytes(b"RIFX\0\0\0\0WAVE")
    with pytest.raises(ReplayError, match="RIFF/WAVE"):
        pcm_extent(path)


def test_rejects_missing_data_chunk(tmp_path: Path) -> None:
    with pytest.raises(ReplayError, match="no data chunk"):
        pcm_extent(_wav(tmp_path / "a.wav", _fmt()))


def test_rejects_short_format_chunk(tmp_path: Path) -> None:
    path = _wav(tmp_path / "a.wav", b"fmt " + struct.pack("<I", 4) + bytes(4))
    with pytest.raises(ReplayError, match="format chunk"):
        pcm_extent(path)


def test_rejects_bad_geometry(tmp_path: Path) -> None:
    with pytest.raises(ReplayError, match="geometry"):
        pcm_extent(_wav(tmp_path / "a.wav", _fmt(channels=9)))


def test_rejects_partial_frame(tmp_path: Path) -> None:
    path = _wav(tmp_path / "a.wav", _fmt(), b"data" + struct.pack("<I", 6), payload=bytes(6))
    with pytest.raises(ReplayError, match="data size"):
        pcm_extent(path)


def test_rejects_data_before_format(tmp_path: Path) -> None:
    path = _wav(tmp_path / "a.wav", b"data" + struct.pack("<I", 4), payload=bytes(4))
    with pytest.raises(ReplayError, match="precedes"):
        pcm_extent(path)
