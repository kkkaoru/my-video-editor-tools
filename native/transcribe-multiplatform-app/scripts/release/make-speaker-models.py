#!/usr/bin/env python3
"""Builds Nemotron 3.5's speaker-only encoder and the decoder stand-in's header from the release.

Usage: make-speaker-models.py --nemo-speech-cpp DIR --llama-cpp DIR --work DIR

Inputs:
  nvidia/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B, revision below. Only byte ranges of its 67.5 GB
  model.safetensors are read: about 1.7 GB of perception.* and ctc_timestamp.*, and 8 KiB per tensor
  for the checkpoint id.
  --nemo-speech-cpp  A NVIDIA/NeMo-Speech.cpp v0.3.0 checkout (commit 20999cc9), for conversion/.
  --llama-cpp        llama.cpp at the commit that checkout pins (bd4f514d), for convert_hf_to_gguf,
                     conversion/ and gguf-py/.
Python needs torch, safetensors, numpy, pyyaml, tqdm, transformers and tokenizers.

`nemo-speech serve` answers /v1/audio/diarizations from the speaker branch alone. It still loads and
runs a whole model, so the rest is kept as small stand-ins of the shapes the program asks for:
  Speaker branch  diar.*, preprocessor.fb, fusion.diar_norm: NVIDIA's weights through the official
                  conversion to F16, not quantized.
  Speech branch   asr.*, the other fusion.*, ctc.*: settings cut to one small layer, weights zero.
                  They feed the decoder and the word timestamps, and no speaker result reads them.
  CTC tokenizer   Cut to its first pieces: the program only compares its size with the vocabulary.
  Decoder         The header holds NVIDIA's vocabulary and settings with one layer and zero weights.
                  The app appends the zeros (engines.rs), so only the header ships.
The output cannot transcribe.
"""

import argparse
import base64
import hashlib
import json
import struct
import subprocess
import sys
import urllib.request
from collections.abc import Iterable, Iterator
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from pathlib import Path

REPOSITORY = "nvidia/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B"
REVISION = "c17667641642c4265ff2b7037dd101b7d4567634"
BASE_URL = f"https://huggingface.co/{REPOSITORY}/resolve/{REVISION}"
CHECKPOINT_NAME = "NVIDIA-Nemotron-3.5-Transcribe-30B-A4B"
ENCODER_NAME = f"{CHECKPOINT_NAME}-Speaker-Encoder-F16.gguf"
HEAD_NAME = "speaker-decoder-head.gguf"
SOURCE_FILES = (
    "config.json",
    "generation_config.json",
    "tokenizer.json",
    "tokenizer_config.json",
    "llm_backbone/config.json",
)
KEPT_PREFIXES = ("perception.", "ctc_timestamp.")
METADATA_KEY = "__metadata__"
ID_BYTES = 4096
CHUNK_BYTES = 1 << 26
ID_WORKERS = 16
DATA_WORKERS = 8
RETRIES = 4

# The stand-in's sizes: one layer of two heads. The head size is the speaker branch's own, because
# flash attention takes a fixed list of them per backend and the branch needs that one to run.
SMALL_MODEL = 128
SMALL_HEADS = 2
SMALL_HEAD = SMALL_MODEL // SMALL_HEADS
SMALL_FEED_FORWARD = 8
SMALL_PIECES = 3
CTC_VOCABULARY = 23863
CTC_TOKENIZER_KEY = "ctc.tokenizer.spm_model"
SPM_PIECES_FIELD = 1
SPM_TEST_DATA_FIELD = 4
SPEECH_BRANCH_KEYS = {
    "d_model": SMALL_MODEL,
    "n_heads": SMALL_HEADS,
    "n_layers": 1,
    "ff_hidden": SMALL_FEED_FORWARD,
    "rotary_dims": SMALL_HEAD,
}
# The program compares the tokenizer's size with the head's, so both shrink together.
SMALL_VOCABULARY = SMALL_PIECES + 1
ASR_SIZES = {1280: SMALL_MODEL, 3840: 3 * SMALL_MODEL, 5120: SMALL_FEED_FORWARD, 128: SMALL_HEAD}
CTC_SIZES = {
    1280: SMALL_MODEL,
    3840: 3 * SMALL_MODEL,
    640: SMALL_FEED_FORWARD,
    80: SMALL_HEAD,
    2560: 2 * SMALL_MODEL,
    CTC_VOCABULARY: SMALL_VOCABULARY,
}
OFFICIAL_SPEAKER_TENSORS = ("diar.", "preprocessor.", "fusion.diar_norm.")
LATER_LAYERS = ("asr.layers.", "ctc.transformer.layers.")

ARCHITECTURE = "nemotron_h_moe"
LAYERS = 1
PER_LAYER_KEYS = (f"{ARCHITECTURE}.feed_forward_length", f"{ARCHITECTURE}.attention.head_count_kv")
ALIGNMENT = 32
GGML_F32 = 0
GGML_Q4_0 = 2
GGML_Q8_0 = 8
QUANTIZED_BLOCK = {GGML_Q4_0: (32, 18), GGML_Q8_0: (32, 34)}
FILE_TYPE = 7
QUANTIZATION_VERSION = 2
FORMAT_VERSION = 3


@dataclass(frozen=True)
class Tensor:
    name: str
    shape: tuple[int, ...]
    kind: int


def read_range(url: str, start: int, end: int) -> bytes:
    """Bytes start..end-1, retried because thousands of requests meet a dropped connection."""
    request = urllib.request.Request(url, headers={"Range": f"bytes={start}-{end - 1}"})
    for attempt in range(RETRIES):
        try:
            with urllib.request.urlopen(request, timeout=120) as response:
                return response.read()
        except OSError:
            if attempt == RETRIES - 1:
                raise
    raise AssertionError("unreachable")


def official_header(url: str) -> tuple[dict, int]:
    size = struct.unpack("<Q", read_range(url, 0, 8))[0]
    return json.loads(read_range(url, 8, 8 + size)), 8 + size


def checkpoint_id(url: str, header: dict, data_start: int) -> str:
    """conversion/salm.py's checkpoint_id, reading its 8 KiB per tensor by range."""
    names = sorted(name for name in header if name != METADATA_KEY)

    def edges(name: str) -> tuple[bytes, bytes]:
        begin, end = header[name]["data_offsets"]
        size = min(ID_BYTES, end - begin)
        first = read_range(url, data_start + begin, data_start + begin + size) if size else b""
        last = read_range(url, data_start + end - size, data_start + end) if size else b""
        return first, last

    digest = hashlib.sha256()
    with ThreadPoolExecutor(ID_WORKERS) as pool:
        for name, (first, last) in zip(names, pool.map(edges, names), strict=True):
            info = header[name]
            digest.update(f"{name}|{info['dtype']}|{info['shape']}|".encode())
            digest.update(first)
            digest.update(last)
    return digest.hexdigest()[:32]


def partial_checkpoint(url: str, header: dict, data_start: int, target: Path) -> None:
    """A valid safetensors of only the kept tensors, so the official conversion reads it unchanged."""
    if target.is_file():
        return
    kept = sorted(
        (name for name in header if name.startswith(KEPT_PREFIXES)),
        key=lambda name: header[name]["data_offsets"][0],
    )
    new_header: dict = {METADATA_KEY: header[METADATA_KEY]}
    pieces: list[tuple[int, int, int]] = []
    position = 0
    for name in kept:
        begin, end = header[name]["data_offsets"]
        new_header[name] = {**header[name], "data_offsets": [position, position + end - begin]}
        pieces.extend(chunks(begin, end, position))
        position += end - begin
    encoded = json.dumps(new_header).encode()
    written = target.with_name(target.name + ".writing")
    with written.open("wb") as out:
        out.write(struct.pack("<Q", len(encoded)) + encoded)
        out.truncate(8 + len(encoded) + position)
        base = 8 + len(encoded)

        def fetch(piece: tuple[int, int, int]) -> tuple[int, bytes]:
            begin, end, place = piece
            return place, read_range(url, data_start + begin, data_start + end)

        with ThreadPoolExecutor(DATA_WORKERS) as pool:
            for place, data in pool.map(fetch, pieces):
                out.seek(base + place)
                out.write(data)
    written.replace(target)


def chunks(begin: int, end: int, place: int) -> Iterator[tuple[int, int, int]]:
    for start in range(begin, end, CHUNK_BYTES):
        yield start, min(end, start + CHUNK_BYTES), place + start - begin


def fetch_source(source: Path) -> tuple[str, dict]:
    """The official files; the header and the id are cached: they take thousands of requests."""
    for name in SOURCE_FILES:
        target = source / name
        target.parent.mkdir(parents=True, exist_ok=True)
        if not target.is_file():
            urllib.request.urlretrieve(f"{BASE_URL}/{name}", target)
    weights = f"{BASE_URL}/model.safetensors"
    header, data_start = official_header(weights)
    identity_file = source / "checkpoint-id.txt"
    if not identity_file.is_file():
        identity_file.write_text(checkpoint_id(weights, header, data_start), encoding="utf-8")
    partial_checkpoint(weights, header, data_start, source / "model.safetensors")
    return identity_file.read_text(encoding="utf-8"), header


def official_encoder(source: Path, work: Path, identity: str) -> Path:
    """The whole encoder file, written the way conversion/salm.py writes it."""
    from conversion import ctc_timestamp, pee
    from conversion.salm_encoder import write_encoder_file

    target = work / f"{CHECKPOINT_NAME}-Encoder-F16.gguf"
    if target.is_file():
        return target
    pee.convert(source, work / "encoder.gguf", "fp16")
    ctc_timestamp.convert(source / "model.safetensors", work / "ctc.gguf", CHECKPOINT_NAME)
    write_encoder_file(
        work / "encoder.gguf",
        work / "ctc.gguf",
        target,
        name=f"{CHECKPOINT_NAME} Encoder F16",
        checkpoint_id=identity,
    )
    (work / "encoder.gguf").unlink()
    (work / "ctc.gguf").unlink()
    return target


def is_later_layer(name: str) -> bool:
    return any(
        name.startswith(prefix) and not name.startswith(f"{prefix}0.") for prefix in LATER_LAYERS
    )


def small_shape(name: str, shape: Iterable[int]) -> tuple[int, ...]:
    sizes = CTC_SIZES if name.startswith("ctc.") else ASR_SIZES
    return tuple(sizes.get(int(size), int(size)) for size in shape)


def proto_fields(data: bytes) -> Iterator[tuple[int, bytes]]:
    """Field numbers and whole encoded fields of a protobuf message of length-delimited fields."""

    def varint(start: int) -> tuple[int, int]:
        value, shift, position = 0, 0, start
        while True:
            byte = data[position]
            value |= (byte & 0x7F) << shift
            shift += 7
            position += 1
            if byte < 0x80:
                return value, position

    position = 0
    while position < len(data):
        tag, body = varint(position)
        if tag & 7 != 2:
            raise ValueError(f"unexpected wire type in tag {tag}")
        size, body = varint(body)
        yield tag >> 3, data[position : body + size]
        position = body + size


def small_tokenizer(encoded: str) -> str:
    """The sentencepiece model with its first pieces, without the self test needing the rest."""
    fields = list(proto_fields(base64.b64decode(encoded)))
    pieces = [field for number, field in fields if number == SPM_PIECES_FIELD][:SMALL_PIECES]
    rest = [
        field for number, field in fields if number not in (SPM_PIECES_FIELD, SPM_TEST_DATA_FIELD)
    ]
    return base64.b64encode(b"".join(pieces + rest)).decode()


def speaker_encoder(official: Path, target: Path) -> None:
    """The official file with the speech branch's settings and weights replaced by the stand-in."""
    import gguf
    import numpy as np

    reader = gguf.GGUFReader(official)
    overrides = {
        f"{prefix}.{key}": value
        for prefix in ("pee.asr", "ctc")
        for key, value in SPEECH_BRANCH_KEYS.items()
    }
    overrides[gguf.Keys.General.NAME] = f"{CHECKPOINT_NAME} Speaker Encoder F16"
    overrides["ctc.vocab_size"] = SMALL_VOCABULARY
    overrides[CTC_TOKENIZER_KEY] = small_tokenizer(reader.fields[CTC_TOKENIZER_KEY].contents())
    writer = gguf.GGUFWriter(target, arch="pee")
    for field in reader.fields.values():
        if field.name.startswith("GGUF.") or field.name == gguf.Keys.General.ARCHITECTURE:
            continue
        kind = field.types[0]
        sub = field.types[-1] if kind == gguf.GGUFValueType.ARRAY else None
        writer.add_key_value(
            field.name, overrides.get(field.name, field.contents()), kind, sub_type=sub
        )
    for tensor in reader.tensors:
        if is_later_layer(tensor.name):
            continue
        if tensor.name.startswith(OFFICIAL_SPEAKER_TENSORS):
            writer.add_tensor(tensor.name, tensor.data, raw_dtype=tensor.tensor_type)
            continue
        shape = small_shape(tensor.name, tensor.shape)
        writer.add_tensor(
            tensor.name,
            np.zeros(shape[::-1], dtype=tensor.data.dtype),
            raw_dtype=tensor.tensor_type,
        )
    writer.write_header_to_file()
    writer.write_kv_data_to_file()
    writer.write_tensors_to_file()
    writer.close()


def vocabulary_only(source: Path, work: Path, identity: str, nemo: Path, llama: Path) -> Path:
    """NVIDIA's vocabulary and settings through the official converter's own --vocab-only."""
    from conversion.salm import stage

    target = work / "decoder-vocabulary.gguf"
    if target.is_file():
        return target
    staging = work / "hf"
    stage(source, staging, weights=False)
    keys = [f"nemo_speech.checkpoint_id={identity}", f"nemo_speech.speech_model={ENCODER_NAME}"]
    subprocess.run(
        [
            sys.executable,
            str(nemo / "conversion" / "salm_decoder_hf.py"),
            str(llama),
            str(staging),
            "--outfile",
            str(target),
            "--outtype",
            "bf16",
            "--vocab-only",
            "--model-name",
            CHECKPOINT_NAME,
            *[arg for key in keys for arg in ("--nemo-speech-key", key)],
        ],
        check=True,
    )
    return target


def decoder_tensors(header: dict, groups: int) -> list[Tensor]:
    """The real decoder's shapes, reversed to ggml's order. Large tables take the smallest type."""
    layer = "llm.model.layers.0."

    def shape(name: str) -> tuple[int, ...]:
        return tuple(header[name]["shape"][::-1])

    mixer = f"{layer}mixer."
    norm_size = shape(f"{mixer}norm.weight")[0]
    conv_size, _, conv_kernel = header[f"{mixer}conv1d.weight"]["shape"]
    return [
        Tensor("output.weight", shape("llm.lm_head.weight"), GGML_Q4_0),
        Tensor("output_norm.weight", shape("llm.model.norm.weight"), GGML_F32),
        Tensor("token_embd.weight", shape("llm.model.embed_tokens.weight"), GGML_Q4_0),
        Tensor("blk.0.attn_norm.weight", shape(f"{layer}norm.weight"), GGML_F32),
        Tensor("blk.0.ssm_a", (1, *shape(f"{mixer}_fp32_params.A_log")), GGML_F32),
        Tensor("blk.0.ssm_conv1d.bias", shape(f"{mixer}conv1d.bias"), GGML_F32),
        Tensor("blk.0.ssm_conv1d.weight", (conv_kernel, conv_size), GGML_F32),
        Tensor("blk.0.ssm_d", (1, *shape(f"{mixer}_fp32_params.D")), GGML_F32),
        Tensor("blk.0.ssm_dt.bias", shape(f"{mixer}_fp32_params.dt_bias"), GGML_F32),
        Tensor("blk.0.ssm_in.weight", shape(f"{mixer}in_proj.weight"), GGML_Q8_0),
        Tensor("blk.0.ssm_norm.weight", (norm_size // groups, groups), GGML_F32),
        Tensor("blk.0.ssm_out.weight", shape(f"{mixer}out_proj.weight"), GGML_Q8_0),
    ]


def tensor_bytes(tensor: Tensor) -> int:
    if tensor.kind == GGML_F32:
        count = 1
        for size in tensor.shape:
            count *= size
        return 4 * count
    elements, block = QUANTIZED_BLOCK[tensor.kind]
    return (tensor.shape[0] + elements - 1) // elements * block * tensor.shape[1]


def padded(count: int) -> int:
    return (count + ALIGNMENT - 1) // ALIGNMENT * ALIGNMENT


def text(value: str) -> bytes:
    raw = value.encode("utf-8")
    return struct.pack("<Q", len(raw)) + raw


def key_value(key: str, kind: int, sub: int | None, value: object) -> bytes:
    import gguf

    scalars = {
        gguf.GGUFValueType.UINT32: "<I",
        gguf.GGUFValueType.INT32: "<i",
        gguf.GGUFValueType.FLOAT32: "<f",
        gguf.GGUFValueType.BOOL: "<?",
        gguf.GGUFValueType.UINT64: "<Q",
    }

    def scalar(scalar_kind: int, item: object) -> bytes:
        return (
            text(item)
            if scalar_kind == gguf.GGUFValueType.STRING
            else struct.pack(scalars[scalar_kind], item)
        )

    head = text(key) + struct.pack("<I", int(kind))
    if kind != gguf.GGUFValueType.ARRAY:
        return head + scalar(kind, value)
    body = b"".join(scalar(sub, item) for item in value)
    return head + struct.pack("<IQ", int(sub), len(value)) + body


def decoder_head(vocabulary: Path, header: dict, target: Path) -> tuple[int, int]:
    """Metadata and tensor table of the one-layer decoder, and the size the zeros extend it to."""
    import gguf

    reader = gguf.GGUFReader(vocabulary)
    groups = int(reader.fields[f"{ARCHITECTURE}.ssm.group_count"].contents())
    pairs = []
    for key, field in reader.fields.items():
        if key.startswith("GGUF.") or key in (
            gguf.Keys.General.FILE_TYPE,
            gguf.Keys.General.QUANTIZATION_VERSION,
        ):
            continue
        value = field.contents()
        if key == f"{ARCHITECTURE}.block_count":
            value = LAYERS
        elif key == f"{ARCHITECTURE}.nextn_predict_layers":
            value = 0
        elif key in PER_LAYER_KEYS:
            value = value[:LAYERS]
        kind = field.types[0]
        pairs.append(
            key_value(
                key, kind, field.types[-1] if kind == gguf.GGUFValueType.ARRAY else None, value
            )
        )
    pairs.append(
        key_value(
            gguf.Keys.General.QUANTIZATION_VERSION,
            gguf.GGUFValueType.UINT32,
            None,
            QUANTIZATION_VERSION,
        )
    )
    pairs.append(key_value(gguf.Keys.General.FILE_TYPE, gguf.GGUFValueType.UINT32, None, FILE_TYPE))
    tensors = decoder_tensors(header, groups)
    table = []
    offset = 0
    for tensor in tensors:
        table.append(text(tensor.name) + struct.pack("<I", len(tensor.shape)))
        table.append(b"".join(struct.pack("<Q", size) for size in tensor.shape))
        table.append(struct.pack("<IQ", tensor.kind, offset))
        offset += padded(tensor_bytes(tensor))
    body = (
        b"GGUF"
        + struct.pack("<IQQ", FORMAT_VERSION, len(tensors), len(pairs))
        + b"".join(pairs)
        + b"".join(table)
    )
    head = body + b"\0" * (padded(len(body)) - len(body))
    target.write_bytes(head)
    return len(head), len(head) + offset


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1 << 24), b""):
            digest.update(block)
    return digest.hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--nemo-speech-cpp", type=Path, required=True)
    parser.add_argument("--llama-cpp", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    options = parser.parse_args()
    nemo = options.nemo_speech_cpp.resolve()
    llama = options.llama_cpp.resolve()
    work = options.work.resolve()
    sys.path[:0] = [str(nemo), str(llama / "gguf-py")]
    source = work / "source"
    source.mkdir(parents=True, exist_ok=True)
    identity, header = fetch_source(source)
    print(f"checkpoint id {identity}")
    official = official_encoder(source, work, identity)
    encoder = work / ENCODER_NAME
    speaker_encoder(official, encoder)
    head = work / HEAD_NAME
    head_size, full_size = decoder_head(
        vocabulary_only(source, work, identity, nemo, llama), header, head
    )
    print(f"{encoder.name} {encoder.stat().st_size} sha256 {sha256(encoder)}")
    print(f"{head.name} {head_size} extends to {full_size} sha256 {sha256(head)}")


if __name__ == "__main__":
    main()
