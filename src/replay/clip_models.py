"""The Mac's local models for the web viewer's clip points (切り抜き), served on 127.0.0.1.

The web viewer computes clip points on the Mac by default (``bun run clips:mac``); Cloudflare AI
runs only when a user asks for a reload. These local models stand in for the Cloudflare ones:

- ``POST /clef`` answers a SystemOne decision request (``state`` plus typed ``questions``) with
  ``mlx-community/clef-OptiQ-4bit``, the 4-bit MLX quant of Cloudflare's clef: the backbone runs
  on MLX and the release's joint schema head (``joint_schema_model.py`` of Cloudflare/clef) is
  ported here to numpy. On 191 judged clips its rubric answers matched Cloudflare clef's token
  counts exactly (r 0.989) and ranked the clips as well (AUC 0.756 with clairic against 0.762);
- ``POST /clef-flash`` answers the same requests with ``mlx-community/clef-flash-OptiQ-4bit`` (the
  same head over a 9B backbone) for the gate's screen;
- ``POST /laya`` answers a rubric request with Laya (``aac6fef/laya-multilingual-mlx`` through
  laya-mlx) for the surrogate judge (``bun run clips:judge``); the gate no longer asks it;
- ``POST /lfm3`` answers an OpenAI-style chat body with ``LiquidAI/LFM3-1.7B`` (8-bit MLX) through
  mlx-vlm (thinking off, greedy), the stand-in for GLM's titles and summaries: about 3-9 s an ask
  against Ternary-Bonsai-2-9B's 58 s;
- ``POST /bonsai`` answers the same chat body with ``Ternary-Bonsai-2-9B`` for the strict local
  judge of ``bun run clips:eval``.

Bonsai and Laya load only with ``--judges``: making clips needs neither, and they would add about
4 GB beside the recording pipelines. The models load once from ``edits/models/`` and are never
downloaded here; the server refuses
to start while less than ``--min-free`` percent of memory is free, since the models take
about 30 GB beside the recording pipelines, and stops itself while serving once less than 15% is.
"""

import argparse
import json
import math
import os
import re
import subprocess
import sys
import threading
import time
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler, HTTPServer
from importlib import import_module
from pathlib import Path
from types import ModuleType
from typing import Protocol

import numpy as np
from numpy.typing import NDArray

from replay.core import ReplayError
from replay.repository import root as checkout_root

type Array = NDArray[np.float32]
type Json = dict[str, object]

CHECKOUT = checkout_root()
CLEF_DIR = CHECKOUT / "edits" / "models" / "clef-OptiQ-4bit"
BONSAI_DIR = CHECKOUT / "edits" / "models" / "Ternary-Bonsai-2-9B-mlx-2bit"
WRITER_DIR = CHECKOUT / "edits" / "models" / "LFM3-1.7B-MLX-8bit"
FLASH_DIR = CHECKOUT / "edits" / "models" / "clef-flash-OptiQ-4bit"
LAYA_DIR = CHECKOUT / "edits" / "models" / "laya-multilingual-mlx"
SYSTEM_PROMPT = (
    "Read the complete state and schema. Decide every field jointly. Each answer "
    "must be exactly one of that field's allowed options."
)
QUESTION_TYPES: dict[str, int] = {"noul": 0, "choice": 1, "score": 2}
NOUL_CRITERIA: dict[str, str] = {
    "true": "The proposition is true or the answer is yes.",
    "false": "The proposition is false or the answer is no.",
}
MAX_LENGTH = 16384
PREFILL_CHUNK = 2048
LAYER_NORM_EPS = 1e-5
LOGIT_SCALE_CAP = math.log(100.0)
CHAT_TOKENS = 1024
MIN_FREE_PERCENT = 40
# Serving stops itself below this so recording pipelines are not pushed into swap.
MIN_RUNNING_FREE_PERCENT = 15
WATCH_SECONDS = 30.0
FREE_PATTERN = re.compile(r"System-wide memory free percentage:\s*(\d+)%")
erf = np.vectorize(math.erf, otypes=[np.float32])


@dataclass(frozen=True)
class Question:
    qid: str
    kind: int
    span: tuple[int, int]
    options: tuple[tuple[str, tuple[int, int]], ...]


@dataclass(frozen=True)
class HeadConfig:
    heads: int
    routing_layers: int
    layers: int


class Backbone(Protocol):
    """The decision model's language backbone: tokens in, final hidden states out."""

    def tokenize(self, text: str) -> list[int]: ...
    def inputs(self, encoded: "Encoded", weights: Mapping[str, Array]) -> "HeadInputs": ...


class LayerCache(Protocol):
    """One layer's mlx-lm cache: (keys, values, offset) for attention, (list, ...) for recurrent."""

    state: tuple[object, ...]


class Quantized(Protocol):
    """An mlx QuantizedLinear's packed weight and its quantization."""

    weight: Sequence[object]
    scales: object
    biases: object
    bits: int
    group_size: int
    mode: str


class SwigluMlp(Protocol):
    gate_proj: Quantized
    up_proj: Quantized
    down_proj: Callable[[object], object]


class Answerer(Protocol):
    def respond(self, body: Json) -> Json: ...


class Serving(Protocol):
    def serve_forever(self) -> None: ...


def render(value: object) -> str:
    if isinstance(value, str):
        return value
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True)


def _mapping(value: object, what: str) -> Json:
    if not isinstance(value, dict):
        raise ReplayError(f"{what} must be an object")
    return {str(key): item for key, item in value.items()}


def question_options(question: Mapping[str, object]) -> list[tuple[str, object]]:
    kind = question.get("type")
    if kind == "noul":
        extra = question.get("criteria") or {}
        noul = {**NOUL_CRITERIA, **_mapping(extra, "noul criteria")}
        return [(key, noul[key]) for key in ("true", "false")]
    criteria = question.get("criteria")
    if kind == "choice":
        return sorted((str(key), value) for key, value in _mapping(criteria, "criteria").items())
    if kind == "score" and isinstance(criteria, list) and criteria:
        return [(str(index), value) for index, value in enumerate(criteria)]
    raise ReplayError(f"Unsupported question {kind!r} or empty criteria")


@dataclass(frozen=True)
class HeadInputs:
    """What the joint head reads from a record's hidden states, all of it O(length) work that the
    backbone can do where the states are (on the GPU) before only these small arrays leave it."""

    #: (length, width): the normalized states projected for the head's attention.
    memory: Array
    #: The normalized final state.
    last: Array
    #: (questions, hidden): the normalized states averaged over each question's instruction.
    questions: Array
    #: Per question, (options, hidden): the normalized states averaged over each option.
    options: list[Array]
    #: Per question, (options, hidden): the output embedding rows averaged over each option.
    lexical: list[Array]


@dataclass(frozen=True)
class Encoded:
    ids: list[int]
    questions: list[Question]
    #: Where the state ends: requests about the same state share every token before it.
    cut: int


def encode(tokenize: Callable[[str], list[int]], record: Mapping[str, object]) -> Encoded:
    """Token ids and question spans, built piece by piece exactly as clef's encode_record does."""
    questions = _mapping(record.get("questions"), "questions")
    if not questions or "state" not in record:
        raise ReplayError("A decision request needs a state and at least one question")
    schema = tokenize("\n\nSCHEMA FIELDS:\n")
    parsed: list[Question] = []
    for number, (qid, raw) in enumerate(questions.items()):
        question = _mapping(raw, f"question {qid}")
        kind = question.get("type")
        if kind not in QUESTION_TYPES:
            raise ReplayError(f"{qid}: type must be noul, choice, or score")
        options = question_options(question)
        schema += tokenize(f"\nFIELD {number + 1}\nID: {qid}\nTYPE: {kind}\nINSTRUCTION: ")
        start = len(schema)
        schema += tokenize(render(question.get("instructions") or str(qid)))
        span = (start, len(schema))
        schema += tokenize("\nALLOWED OPTIONS:\n")
        spans: list[tuple[str, tuple[int, int]]] = []
        for index, (oid, description) in enumerate(options):
            schema += tokenize(f"OPTION {index + 1}: ")
            begin = len(schema)
            semantics: Json = {"option_id": oid}
            if description is not None:
                semantics["description"] = description
            schema += tokenize(render(semantics))
            spans.append((oid, (begin, len(schema))))
            schema += tokenize("\n")
        schema += tokenize("END FIELD\n")
        parsed.append(Question(str(qid), QUESTION_TYPES[str(kind)], span, tuple(spans)))
    prefix = tokenize(f"<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n<|im_start|>user\nSTATE:\n")
    suffix = tokenize(
        "\n<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\nJOINT SCHEMA DECISIONS:"
    )
    room = MAX_LENGTH - len(prefix) - len(schema) - len(suffix)
    if room < 0:
        raise ReplayError(
            f"The schema needs {MAX_LENGTH - room} tokens; the maximum is {MAX_LENGTH}"
        )
    state = tokenize(render(record["state"]))[:room]
    offset = len(prefix) + len(state)
    shifted = [
        Question(
            q.qid,
            q.kind,
            (q.span[0] + offset, q.span[1] + offset),
            tuple((oid, (b + offset, e + offset)) for oid, (b, e) in q.options),
        )
        for q in parsed
    ]
    return Encoded(prefix + state + schema + suffix, shifted, offset)


def layer_norm(x: Array, w: Mapping[str, Array], name: str) -> Array:
    mean = x.mean(-1, keepdims=True)
    var = ((x - mean) ** 2).mean(-1, keepdims=True)
    normalized = (x - mean) / np.sqrt(var + LAYER_NORM_EPS)
    return np.asarray(normalized * w[f"{name}.weight"] + w[f"{name}.bias"], dtype=np.float32)


def linear(x: Array, w: Mapping[str, Array], name: str, *, bias: bool = True) -> Array:
    y = x @ w[f"{name}.weight"].T
    return y + w[f"{name}.bias"] if bias else y


def gelu(x: Array) -> Array:
    return np.asarray(0.5 * x * (1 + erf(x / math.sqrt(2))), dtype=np.float32)


def softmax(x: Array, axis: int = -1) -> Array:
    shifted = np.exp(x - x.max(axis=axis, keepdims=True))
    return np.asarray(shifted / shifted.sum(axis=axis, keepdims=True), dtype=np.float32)


def unit(x: Array) -> Array:
    return np.asarray(
        x / np.maximum(np.linalg.norm(x, axis=-1, keepdims=True), 1e-12), dtype=np.float32
    )


def attention(
    q: Array,
    kv: Array,
    w: Mapping[str, Array],
    name: str,
    heads: int,
    *,
    gpu: ModuleType | None = None,
) -> Array:
    """torch.nn.MultiheadAttention (batch_first, no mask) on unbatched (length, width) inputs."""
    if gpu is not None:
        return _metal_attention(gpu, q, kv, w, name, heads)
    width = q.shape[-1]
    dh = width // heads
    wq, wk, wv = np.split(w[f"{name}.in_proj_weight"], 3)
    bq, bk, bv = np.split(w[f"{name}.in_proj_bias"], 3)

    def split(x: Array) -> Array:
        return x.reshape(x.shape[0], heads, dh).transpose(1, 0, 2)

    qh, kh, vh = split(q @ wq.T + bq), split(kv @ wk.T + bk), split(kv @ wv.T + bv)
    weights = softmax((qh @ kh.transpose(0, 2, 1)) / math.sqrt(dh))
    out = (weights @ vh).transpose(1, 0, 2).reshape(q.shape[0], width)
    return linear(out, w, f"{name}.out_proj")


def _metal_attention(
    mx: ModuleType, q: Array, kv: Array, w: Mapping[str, Array], name: str, heads: int
) -> Array:
    """`attention` with its projections, scores and weighted sum on Metal (``mlx.core``): against
    the 13k-token memory they were 70% of the head's CPU time (11/1: 1.66 s a request, 0.56 s on
    the GPU; answers move by at most 2e-4, float32 sums taken in another order)."""
    width = q.shape[-1]
    dh = width // heads
    wq, wk, wv = (mx.array(part) for part in np.split(w[f"{name}.in_proj_weight"], 3))
    bq, bk, bv = (mx.array(part) for part in np.split(w[f"{name}.in_proj_bias"], 3))
    query, memory = mx.array(q), mx.array(kv)

    def split(x: object, length: int) -> object:
        return mx.transpose(mx.reshape(x, (length, heads, dh)), (1, 0, 2))

    qh = split(query @ wq.T + bq, q.shape[0])
    kh, vh = split(memory @ wk.T + bk, kv.shape[0]), split(memory @ wv.T + bv, kv.shape[0])
    scores = mx.matmul(qh, mx.transpose(kh, (0, 2, 1))) / math.sqrt(dh)
    out = mx.reshape(
        mx.transpose(mx.matmul(mx.softmax(scores, axis=-1, precise=True), vh), (1, 0, 2)),
        (q.shape[0], width),
    )
    return linear(np.array(out, dtype=np.float32), w, f"{name}.out_proj")


def _routed(
    w: Mapping[str, Array],
    config: HeadConfig,
    memory: Array,
    queries: Array,
    gpu: ModuleType | None,
) -> Array:
    for n in range(config.routing_layers):
        p = f"evidence_layers.{n}"
        mem = layer_norm(memory, w, f"{p}.memory_norm")
        normalized = layer_norm(queries, w, f"{p}.query_norm")
        queries = queries + attention(normalized, mem, w, f"{p}.attention", config.heads, gpu=gpu)
        ff = gelu(linear(layer_norm(queries, w, f"{p}.feedforward_norm"), w, f"{p}.feedforward.0"))
        queries = queries + linear(ff, w, f"{p}.feedforward.3")
    return queries


def _decoded(
    w: Mapping[str, Array],
    config: HeadConfig,
    fields: Array,
    memory: Array,
    gpu: ModuleType | None,
) -> Array:
    for n in range(config.layers):
        p = f"layers.{n}"
        x = layer_norm(fields, w, f"{p}.norm1")
        fields = fields + attention(x, x, w, f"{p}.self_attn", config.heads)
        cross = layer_norm(fields, w, f"{p}.norm2")
        fields = fields + attention(cross, memory, w, f"{p}.multihead_attn", config.heads, gpu=gpu)
        ff = gelu(linear(layer_norm(fields, w, f"{p}.norm3"), w, f"{p}.linear1"))
        fields = fields + linear(ff, w, f"{p}.linear2")
    return layer_norm(fields, w, "field_norm")


def head_inputs(
    w: Mapping[str, Array],
    hidden: Array,
    questions: Sequence[Question],
    lexicon: Callable[[Sequence[int]], Array],
    ids: Sequence[int],
) -> HeadInputs:
    """The head's inputs from hidden states on the CPU (the reference the GPU path matches)."""
    seq = layer_norm(hidden, w, "hidden_norm")
    return HeadInputs(
        memory=linear(seq, w, "memory_projection", bias=False),
        last=seq[-1],
        questions=np.stack([seq[s:e].mean(0) for s, e in (q.span for q in questions)]),
        options=[np.stack([seq[b:e].mean(0) for _, (b, e) in q.options]) for q in questions],
        lexical=[
            np.stack([lexicon(ids[b:e]).mean(0) for _, (b, e) in q.options]) for q in questions
        ],
    )


def head(
    w: Mapping[str, Array],
    config: HeadConfig,
    inputs: HeadInputs,
    questions: Sequence[Question],
    *,
    gpu: ModuleType | None = None,
) -> list[Array]:
    """JointSchemaHead.forward for one record: the option logits of every question; ``gpu`` runs
    its attention over the memory on Metal."""
    memory, global_vector, qvec = inputs.memory, inputs.last, inputs.questions
    contexts, lexical = inputs.options, inputs.lexical
    queries = np.concatenate(
        [
            linear(c, w, "option_context_projection", bias=False)
            + linear(x, w, "option_lexical_projection", bias=False)
            + linear(qvec[i], w, "option_question_projection", bias=False)[None]
            for i, (c, x) in enumerate(zip(contexts, lexical, strict=True))
        ]
    )
    routed = np.split(
        _routed(w, config, memory, queries, gpu),
        np.cumsum([len(q.options) for q in questions])[:-1],
    )
    base = linear(qvec, w, "question_projection", bias=False)
    summaries = [
        (softmax(options @ field / math.sqrt(options.shape[-1]), 0)[:, None] * options).sum(0)
        for field, options in zip(base, routed, strict=True)
    ]
    fields = (
        base
        + layer_norm(np.stack(summaries), w, "option_summary_norm")
        + linear(global_vector, w, "global_projection", bias=False)[None]
        + w["type_embedding.weight"][[q.kind for q in questions]]
    )
    fields = _decoded(w, config, fields, memory, gpu)
    prior_scale = math.exp(min(float(w["prior_logit_scale"]), LOGIT_SCALE_CAP))
    joint_scale = math.exp(min(float(w["joint_logit_scale"]), LOGIT_SCALE_CAP))
    gate = 1 / (1 + math.exp(-float(w["residual_gate"])))
    logits: list[Array] = []
    for i, (field, lex, options) in enumerate(zip(fields, lexical, routed, strict=True)):
        prior = prior_scale * (unit(lex) @ unit(qvec[i] + global_vector))
        options = layer_norm(options, w, "option_norm")
        repeated = np.broadcast_to(field[None], options.shape)
        cosine = (unit(repeated) * unit(options)).sum(-1)
        features = np.concatenate(
            [repeated, options, repeated * options, np.abs(repeated - options)], axis=-1
        )
        residual = linear(gelu(linear(features, w, "residual_scorer.0")), w, "residual_scorer.3")
        logits.append(prior + gate * (joint_scale * cosine + residual[:, 0]))
    return logits


def answer(question: Mapping[str, object], probabilities: Mapping[str, float]) -> Json:
    """One SystemOne answer from the per-option probabilities of a question."""
    if question["type"] == "noul":
        return {"type": "noul", "noul": round(probabilities["true"], 4)}
    if question["type"] == "choice":
        options = [str(key) for key in _mapping(question["criteria"], "criteria")]
        choice = max(options, key=probabilities.__getitem__)
        return {
            "type": "choice",
            "choice": choice,
            "confidence": round(probabilities[choice], 4),
            "probabilities": {key: round(probabilities[key], 4) for key in options},
        }
    levels = [str(i) for i in range(len(probabilities))]
    return {
        "type": "score",
        "score": round(sum(i * probabilities[level] for i, level in enumerate(levels)), 4),
        "confidence": round(max(probabilities[level] for level in levels), 4),
        "probabilities": {level: round(probabilities[level], 4) for level in levels},
    }


class Decider:
    """clef's SystemOne endpoint over a backbone and the joint schema head's weights."""

    def __init__(
        self,
        backbone: Backbone,
        weights: Mapping[str, Array],
        config: HeadConfig,
        gpu: ModuleType | None = None,
    ):
        """``gpu`` (``mlx.core``) runs the head's attention over the memory on Metal."""
        self.backbone, self.weights, self.config, self.gpu = backbone, weights, config, gpu

    def respond(self, body: Json) -> Json:
        request = body
        encoded = encode(self.backbone.tokenize, request)
        ids, questions = encoded.ids, encoded.questions
        logits = head(
            self.weights,
            self.config,
            self.backbone.inputs(encoded, self.weights),
            questions,
            gpu=self.gpu,
        )
        asked = _mapping(request["questions"], "questions")
        answers = {
            q.qid: answer(
                _mapping(asked[q.qid], q.qid),
                dict(zip([oid for oid, _ in q.options], softmax(row).tolist(), strict=True)),
            )
            for q, row in zip(questions, logits, strict=True)
        }
        return {
            "model": request.get("model"),
            "answers": answers,
            "usage": {"input_tokens": len(ids), "output_tokens": 0},
        }


class FusedMlp:
    """A SwiGLU MLP whose gate and up projections, quantized alike, run as one quantized matmul.
    Each output column is the same dot product as before, so the result is identical, and the
    prefill is about 5% faster (gate+up 88 -> 77 ms per 2048-token chunk on an M5 Pro)."""

    def __init__(self, mx: ModuleType, mlp: SwigluMlp):
        gate, up = mlp.gate_proj, mlp.up_proj
        self.mx, self.down = mx, mlp.down_proj
        self.weight = mx.concatenate([gate.weight, up.weight])
        self.scales = mx.concatenate([gate.scales, up.scales])
        self.biases = mx.concatenate([gate.biases, up.biases])
        self.width = len(gate.weight)
        self.bits, self.group_size, self.mode = gate.bits, gate.group_size, gate.mode
        self.swiglu = import_module("mlx_lm.models.activations").swiglu

    def __call__(self, x: object) -> object:
        both = self.mx.quantized_matmul(
            x, self.weight, scales=self.scales, biases=self.biases, transpose=True,
            group_size=self.group_size, bits=self.bits, mode=self.mode,
        )  # fmt: skip
        gate, up = self.mx.split(both, [self.width], axis=-1)
        return self.down(self.swiglu(gate, up))


def fusable(mlp: SwigluMlp) -> bool:
    gate, up = mlp.gate_proj, mlp.up_proj
    if not hasattr(gate, "scales") or not hasattr(up, "scales"):
        return False
    return (gate.bits, gate.group_size, gate.mode) == (up.bits, up.group_size, up.mode)


class MlxBackbone:
    """The 4-bit clef backbone on MLX through mlx-lm (``uv sync --extra clips``)."""

    def __init__(self, model_dir: Path):
        self.mx = import_module("mlx.core")
        self.model, self.tokenizer = import_module("mlx_lm").load(str(model_dir))
        lm = self.model.language_model
        self.text = lm.model
        self.output = lm.model.embed_tokens if lm.args.tie_word_embeddings else lm.lm_head
        # The previous request's ids up to its state's end, the cache there and the hidden states.
        self.kept_ids: list[int] = []
        self.kept: tuple[list[tuple[object, ...]], list[object]] | None = None
        self.gpu_weights: dict[str, object] = {}
        for layer in self.text.layers:
            if hasattr(layer.mlp, "gate_proj") and fusable(layer.mlp):
                layer.mlp = FusedMlp(self.mx, layer.mlp)

    def tokenize(self, text: str) -> list[int]:
        return list(self.tokenizer.encode(text, add_special_tokens=False))

    def _snapshot(self, cache: Sequence[LayerCache]) -> list[tuple[object, ...]]:
        """Each layer's state as it is now: attention keys/values cut at their offset (MLX arrays
        never change in place, so a slice keeps these values), the recurrent layers' list copied
        (they replace its items as they go)."""
        out: list[tuple[object, ...]] = []
        for layer in cache:
            state = tuple(layer.state)
            if isinstance(state[0], list):
                out.append((list(state[0]), *state[1:]))
            else:
                keys, values, offset = state
                take = self.mx.take
                out.append(
                    (
                        take(keys, self.mx.arange(offset), 2),
                        take(values, self.mx.arange(offset), 2),
                        offset,
                    )
                )
        return out

    def _restored(self, states: Sequence[tuple[object, ...]]) -> list[LayerCache]:
        cache = self.model.make_cache()
        for layer, state in zip(cache, states, strict=True):
            layer.state = (list(state[0]), *state[1:]) if isinstance(state[0], list) else state
        return list(cache)

    def _prefill(self, ids: Sequence[int], cache: Sequence[LayerCache]) -> list[object]:
        """Final hidden states chunk by chunk, left on the GPU."""
        mx = self.mx
        return [
            self.text(mx.array(list(ids[at : at + PREFILL_CHUNK]))[None], cache=cache)[0]
            for at in range(0, len(ids), PREFILL_CHUNK)
        ]

    def hidden(self, ids: Sequence[int], cut: int) -> object:
        """Final hidden states of `ids` (float32, on the GPU), prefilled in chunks up to `cut` (the
        state's end) and on from there, so every request is chunked the same way. The region
        requests of one region share their state and differ only in the questions after it: when
        the previous request had the same tokens up to `cut`, its cache there is restored rather
        than computed again, and the result is the same."""
        mx, ids = self.mx, list(ids)
        if self.kept is not None and ids[:cut] == self.kept_ids:
            states, before = self.kept
            cache = self._restored(states)
        else:
            cache = list(self.model.make_cache())
            before = self._prefill(ids[:cut], cache)
            self.kept_ids, self.kept = ids[:cut], (self._snapshot(cache), before)
        return mx.concatenate([*before, *self._prefill(ids[cut:], cache)]).astype(mx.float32)

    def _on_gpu(self, weights: Mapping[str, Array], name: str) -> object:
        if name not in self.gpu_weights:
            self.gpu_weights[name] = self.mx.array(weights[name])
        return self.gpu_weights[name]

    def inputs(self, encoded: Encoded, weights: Mapping[str, Array]) -> HeadInputs:
        """The head's O(length) work on the GPU: normalize the states, project them for the head's
        attention, and average them over every question and option span with one matrix product;
        the output embedding rows of all option tokens are dequantized in one gather. Only these
        small arrays are copied to the CPU (the states themselves are 13k x 5120 floats)."""
        mx, questions = self.mx, encoded.questions
        states = self.hidden(encoded.ids, encoded.cut)
        mean = mx.mean(states, axis=-1, keepdims=True)
        spread = mx.mean((states - mean) ** 2, axis=-1, keepdims=True)
        seq = (states - mean) / mx.sqrt(spread + LAYER_NORM_EPS)
        seq = seq * self._on_gpu(weights, "hidden_norm.weight") + self._on_gpu(
            weights, "hidden_norm.bias"
        )
        memory = seq @ mx.transpose(self._on_gpu(weights, "memory_projection.weight"))
        spans = [q.span for q in questions] + [span for q in questions for _, span in q.options]
        averaging = np.zeros((len(spans), len(encoded.ids)), np.float32)
        for row, (start, end) in enumerate(spans):
            averaging[row, start:end] = 1.0 / (end - start)
        means = np.array(mx.array(averaging) @ seq)
        last = np.array(seq[-1])
        memory_np = np.array(memory)
        self.mx.clear_cache()
        tokens = sorted({t for q in questions for _, (b, e) in q.options for t in encoded.ids[b:e]})
        rows = dict(zip(tokens, self.lexicon(tokens), strict=True))
        counts = np.cumsum([len(q.options) for q in questions])[:-1]
        option_means = np.split(means[len(questions) :], counts)
        lexical = [
            np.stack(
                [np.mean([rows[t] for t in encoded.ids[b:e]], axis=0) for _, (b, e) in q.options]
            )
            for q in questions
        ]
        return HeadInputs(
            memory=memory_np.astype(np.float32),
            last=last.astype(np.float32),
            questions=means[: len(questions)].astype(np.float32),
            options=[o.astype(np.float32) for o in option_means],
            lexical=[x.astype(np.float32) for x in lexical],
        )

    def lexicon(self, ids: Sequence[int]) -> Array:
        mx, output = self.mx, self.output
        rows = mx.array(list(ids))
        if hasattr(output, "scales"):
            values = mx.dequantize(
                output.weight[rows], output.scales[rows], output.biases[rows],
                output.group_size, output.bits,
            )  # fmt: skip
        else:
            values = output.weight[rows]
        return np.array(values.astype(mx.float32))


def load_decider(model_dir: Path) -> Decider:
    mx = import_module("mlx.core")
    config = json.loads((model_dir / "joint_head_config.json").read_text(encoding="utf-8"))
    raw = mx.load(str(model_dir / "joint_head.safetensors"))
    weights = {key: np.array(value.astype(mx.float32)) for key, value in raw.items()}
    head_config = HeadConfig(config["heads"], config["routing_layers"], config["layers"])
    return Decider(MlxBackbone(model_dir), weights, head_config, mx)


class MlxLaya:
    """Laya (multilingual, MLX) in clairic's place: the same SystemOne rubric request as clef."""

    def __init__(self, model_dir: Path):
        self.agent = import_module("laya_mlx").load(str(model_dir))

    def respond(self, body: Json) -> Json:
        if "state" not in body:
            raise ReplayError("A decision request needs a state and at least one question")
        questions = _mapping(body.get("questions"), "questions")
        result = _mapping(self.agent.predict(body["state"], questions), "Laya's reply")
        return {
            "model": body.get("model"),
            "answers": result.get("answers"),
            "usage": result.get("usage"),
        }


class MlxChat:
    """A chat model on mlx-vlm, text only, thinking off and greedy, so equal asks agree."""

    def __init__(self, model_dir: Path):
        vlm = import_module("mlx_vlm")
        self.generate = vlm.generate
        self.model, self.processor = vlm.load(str(model_dir))
        self.tokenizer = getattr(self.processor, "tokenizer", self.processor)

    def respond(self, body: Json) -> Json:
        messages = body.get("messages")
        if not isinstance(messages, list) or not messages:
            raise ReplayError("A chat request needs messages")
        prompt = self.tokenizer.apply_chat_template(
            messages, tokenize=False, add_generation_prompt=True, enable_thinking=False
        )
        asked = body.get("max_completion_tokens")
        limit = min(asked, CHAT_TOKENS) if isinstance(asked, int) and asked > 0 else CHAT_TOKENS
        result = self.generate(
            self.model, self.processor, prompt, max_tokens=limit, temperature=0.0, verbose=False
        )
        return {"choices": [{"message": {"role": "assistant", "content": str(result.text)}}]}


def free_percent(run: Callable[[list[str]], str]) -> int:
    found = FREE_PATTERN.search(run(["memory_pressure"]))
    if not found:
        raise ReplayError("memory_pressure did not report the free memory percentage")
    return int(found.group(1))


def _run(command: list[str]) -> str:
    return subprocess.run(command, check=True, capture_output=True, text=True).stdout


def watch(
    run: Callable[[list[str]], str],
    stop: Callable[[int], None],
    *,
    sleep: Callable[[float], None] = time.sleep,
    rounds: int | None = None,
) -> None:
    """Stops the process once free memory falls below MIN_RUNNING_FREE_PERCENT."""
    done = 0
    while rounds is None or done < rounds:
        sleep(WATCH_SECONDS)
        done += 1
        free = free_percent(run)
        if free < MIN_RUNNING_FREE_PERCENT:
            print(f"stopping: only {free}% of memory is free", file=sys.stderr, flush=True)
            stop(1)
            return


def handler(routes: Mapping[str, Answerer]) -> type[BaseHTTPRequestHandler]:
    """POST /<name> answers a JSON body with the model registered under that name."""

    class Handler(BaseHTTPRequestHandler):
        def do_POST(self) -> None:  # noqa: N802 (http.server's hook name)
            route = routes.get(self.path.strip("/"))
            try:
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"] or 0)))
                if route is None:
                    self._send(404, {"error": f"unknown model {self.path}"})
                    return
                self._send(200, route.respond(_mapping(body, "request")))
            except (ReplayError, ValueError, KeyError) as error:
                self._send(400, {"error": str(error)})

        def _send(self, status: int, payload: Json) -> None:
            data = json.dumps(payload, ensure_ascii=False).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def log_message(self, format: str, *args: object) -> None:  # noqa: A002
            del format, args  # one line per answer would flood a pipeline step's log

    return Handler


def main(
    argv: Sequence[str] | None = None,
    *,
    run: Callable[[list[str]], str] = _run,
    decider: Callable[[Path], Answerer] = load_decider,
    writer: Callable[[Path], Answerer] = MlxChat,
    reader: Callable[[Path], Answerer] = MlxLaya,
    server: Callable[[tuple[str, int], type[BaseHTTPRequestHandler]], Serving] = HTTPServer,
    guard: Callable[[Callable[[list[str]], str]], None] = lambda run: threading.Thread(
        target=watch, args=(run, os._exit), daemon=True
    ).start(),
) -> int:
    parser = argparse.ArgumentParser(prog="python -m replay.clip_models", description=__doc__)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--clef", type=Path, default=CLEF_DIR)
    parser.add_argument("--lfm3", type=Path, default=WRITER_DIR)
    parser.add_argument("--bonsai", type=Path, default=BONSAI_DIR)
    parser.add_argument("--flash", type=Path, default=FLASH_DIR)
    parser.add_argument("--laya", type=Path, default=LAYA_DIR)
    parser.add_argument("--min-free", type=int, default=MIN_FREE_PERCENT)
    parser.add_argument("--judges", action="store_true", help="also serve /bonsai and /laya")
    args = parser.parse_args(argv)
    needed = [(args.clef, "config.json"), (args.flash, "config.json"), (args.lfm3, "config.json")]
    if args.judges:
        needed += [(args.bonsai, "config.json"), (args.laya, "mlx_config.json")]
    for folder, marker in needed:
        if not (folder / marker).is_file():
            raise ReplayError(f"Model not found (it is never downloaded here): {folder}")
    free = free_percent(run)
    if free < args.min_free:
        raise ReplayError(f"Only {free}% of memory is free; the local models need {args.min_free}%")
    routes: dict[str, Answerer] = {
        "clef": decider(args.clef),
        "clef-flash": decider(args.flash),
        "lfm3": writer(args.lfm3),
    }
    if args.judges:
        routes |= {"bonsai": writer(args.bonsai), "laya": reader(args.laya)}
    http = server(("127.0.0.1", args.port), handler(routes))
    guard(run)
    print(f"ready {args.port}", flush=True)
    http.serve_forever()
    return 0


if __name__ == "__main__":
    sys.exit(main())
