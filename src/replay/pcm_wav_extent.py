"""PCM WAV frame extent from the file itself, including >4 GiB placeholder sizes.

FFmpeg writes 0xFFFFFFFF chunk sizes past 4 GiB, which the standard library reports as a
truncated frame count. The extent is derived from the format chunk and the real file size.
"""

from dataclasses import dataclass
from pathlib import Path

from replay.core import ReplayError


@dataclass(frozen=True, slots=True)
class PcmExtent:
    offset: int
    frames: int
    channels: int
    width: int


def pcm_extent(path: Path) -> PcmExtent:
    """Data offset, frame count and sample geometry of a PCM WAV."""
    size = path.stat().st_size
    with path.open("rb") as stream:
        header = stream.read(12)
        if len(header) != 12 or header[:4] != b"RIFF" or header[8:] != b"WAVE":
            raise ReplayError("Expected a RIFF/WAVE PCM file")
        channels = width = block = 0
        while True:
            chunk = stream.read(8)
            if len(chunk) != 8:
                raise ReplayError("WAV has no data chunk")
            name, chunk_size = chunk[:4], int.from_bytes(chunk[4:], "little")
            offset = stream.tell()
            if name == b"fmt ":
                body = stream.read(chunk_size)
                if len(body) != chunk_size or len(body) < 16:
                    raise ReplayError("Invalid WAV format chunk")
                channels = int.from_bytes(body[2:4], "little")
                width = int.from_bytes(body[14:16], "little") // 8
                block = int.from_bytes(body[12:14], "little") or channels * width
                if not 1 <= channels <= 8 or not 1 <= width <= 4 or block <= 0:
                    raise ReplayError("Invalid WAV sample geometry")
                stream.seek(offset + chunk_size + chunk_size % 2)
                continue
            if name == b"data" and block:
                data_bytes = size - offset if chunk_size == 0xFFFFFFFF else chunk_size
                if data_bytes < 0 or data_bytes % block:
                    raise ReplayError("Invalid WAV data size")
                return PcmExtent(
                    offset=offset, frames=data_bytes // block, channels=channels, width=width
                )
            if name == b"data":
                raise ReplayError("WAV data chunk precedes its format chunk")
            stream.seek(offset + chunk_size + chunk_size % 2)
