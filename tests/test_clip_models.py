import importlib
import json
import threading
import urllib.error
import urllib.request
from collections.abc import Mapping, Sequence
from http.server import HTTPServer
from pathlib import Path
from types import SimpleNamespace

import numpy as np
import pytest

from replay import clip_models
from replay.core import ReplayError

WIDTH = 4
HIDDEN = 6
CONFIG = clip_models.HeadConfig(heads=2, routing_layers=1, layers=1)
SHAPES = {
    "hidden_norm.weight": (HIDDEN,),
    "hidden_norm.bias": (HIDDEN,),
    "memory_projection.weight": (WIDTH, HIDDEN),
    "question_projection.weight": (WIDTH, HIDDEN),
    "option_question_projection.weight": (WIDTH, HIDDEN),
    "global_projection.weight": (WIDTH, HIDDEN),
    "option_context_projection.weight": (WIDTH, HIDDEN),
    "option_lexical_projection.weight": (WIDTH, HIDDEN),
    "type_embedding.weight": (3, WIDTH),
    "evidence_layers.0.query_norm.weight": (WIDTH,),
    "evidence_layers.0.query_norm.bias": (WIDTH,),
    "evidence_layers.0.memory_norm.weight": (WIDTH,),
    "evidence_layers.0.memory_norm.bias": (WIDTH,),
    "evidence_layers.0.attention.in_proj_weight": (3 * WIDTH, WIDTH),
    "evidence_layers.0.attention.in_proj_bias": (3 * WIDTH,),
    "evidence_layers.0.attention.out_proj.weight": (WIDTH, WIDTH),
    "evidence_layers.0.attention.out_proj.bias": (WIDTH,),
    "evidence_layers.0.feedforward_norm.weight": (WIDTH,),
    "evidence_layers.0.feedforward_norm.bias": (WIDTH,),
    "evidence_layers.0.feedforward.0.weight": (5, WIDTH),
    "evidence_layers.0.feedforward.0.bias": (5,),
    "evidence_layers.0.feedforward.3.weight": (WIDTH, 5),
    "evidence_layers.0.feedforward.3.bias": (WIDTH,),
    "option_summary_norm.weight": (WIDTH,),
    "option_summary_norm.bias": (WIDTH,),
    "layers.0.self_attn.in_proj_weight": (3 * WIDTH, WIDTH),
    "layers.0.self_attn.in_proj_bias": (3 * WIDTH,),
    "layers.0.self_attn.out_proj.weight": (WIDTH, WIDTH),
    "layers.0.self_attn.out_proj.bias": (WIDTH,),
    "layers.0.multihead_attn.in_proj_weight": (3 * WIDTH, WIDTH),
    "layers.0.multihead_attn.in_proj_bias": (3 * WIDTH,),
    "layers.0.multihead_attn.out_proj.weight": (WIDTH, WIDTH),
    "layers.0.multihead_attn.out_proj.bias": (WIDTH,),
    "layers.0.norm1.weight": (WIDTH,),
    "layers.0.norm1.bias": (WIDTH,),
    "layers.0.norm2.weight": (WIDTH,),
    "layers.0.norm2.bias": (WIDTH,),
    "layers.0.norm3.weight": (WIDTH,),
    "layers.0.norm3.bias": (WIDTH,),
    "layers.0.linear1.weight": (5, WIDTH),
    "layers.0.linear1.bias": (5,),
    "layers.0.linear2.weight": (WIDTH, 5),
    "layers.0.linear2.bias": (WIDTH,),
    "field_norm.weight": (WIDTH,),
    "field_norm.bias": (WIDTH,),
    "option_norm.weight": (WIDTH,),
    "option_norm.bias": (WIDTH,),
    "residual_scorer.0.weight": (WIDTH, 4 * WIDTH),
    "residual_scorer.0.bias": (WIDTH,),
    "residual_scorer.3.weight": (1, WIDTH),
    "residual_scorer.3.bias": (1,),
    "prior_logit_scale": (),
    "joint_logit_scale": (),
    "residual_gate": (),
}
REQUEST: dict[str, object] = {
    "model": "clef",
    "state": {"b": "い", "a": 1},
    "questions": {
        "ok": {"type": "noul", "instructions": "良い?"},
        "tone": {"type": "choice", "criteria": {"z": "Z", "y": "Y"}},
        "level": {"type": "score", "instructions": {"k": "v"}, "criteria": ["low", "high"]},
    },
}


def weights() -> dict[str, np.ndarray]:
    rng = np.random.default_rng(7)
    return {
        name: rng.standard_normal(shape).astype(np.float32) * 0.5 for name, shape in SHAPES.items()
    }


def characters(text: str) -> list[int]:
    return [ord(c) for c in text]


def text_of(ids: list[int], span: tuple[int, int]) -> str:
    return "".join(chr(i) for i in ids[span[0] : span[1]])


class FakeBackbone:
    def tokenize(self, text: str) -> list[int]:
        return [ord(c) % 50 for c in text]

    def hidden(self, ids: Sequence[int]) -> np.ndarray:
        rows = np.arange(len(ids) * HIDDEN, dtype=np.float32).reshape(len(ids), HIDDEN)
        return np.sin(rows)

    def lexicon(self, ids: Sequence[int]) -> np.ndarray:
        return np.cos(np.outer(np.array(ids, dtype=np.float32), np.ones(HIDDEN, np.float32)))

    def inputs(
        self, encoded: clip_models.Encoded, weights: Mapping[str, np.ndarray]
    ) -> clip_models.HeadInputs:
        return clip_models.head_inputs(
            weights, self.hidden(encoded.ids), encoded.questions, self.lexicon, encoded.ids
        )


def test_options_follow_clef_order_and_reject_unknown_types() -> None:
    assert clip_models.question_options({"type": "noul", "criteria": {"true": "yes"}}) == [
        ("true", "yes"),
        ("false", "The proposition is false or the answer is no."),
    ]
    assert clip_models.question_options({"type": "choice", "criteria": {"b": 1, "a": 2}}) == [
        ("a", 2),
        ("b", 1),
    ]
    assert clip_models.question_options({"type": "score", "criteria": ["x", "y"]}) == [
        ("0", "x"),
        ("1", "y"),
    ]
    with pytest.raises(ReplayError, match="Unsupported question 'score'"):
        clip_models.question_options({"type": "score", "criteria": []})
    with pytest.raises(ReplayError, match="criteria must be an object"):
        clip_models.question_options({"type": "choice", "criteria": ["x"]})


def test_encode_spans_cover_instructions_and_options_and_render_state_sorted() -> None:
    encoded = clip_models.encode(characters, REQUEST)
    ids, questions = encoded.ids, encoded.questions
    text = "".join(chr(i) for i in ids)
    assert text.startswith(
        "<|im_start|>system\nRead the complete state and schema. Decide every field jointly."
    )
    assert (
        '<|im_start|>user\nSTATE:\n{"a":1,"b":"い"}\n\nSCHEMA FIELDS:\n\nFIELD 1\nID: ok\n' in text
    )
    assert text.endswith("<think>\n\n</think>\n\nJOINT SCHEMA DECISIONS:")
    assert [q.kind for q in questions] == [0, 1, 2]
    assert [text_of(ids, q.span) for q in questions] == ["良い?", "tone", '{"k":"v"}']
    assert [text_of(ids, span) for _, span in questions[1].options] == [
        '{"description":"Y","option_id":"y"}',
        '{"description":"Z","option_id":"z"}',
    ]
    assert [oid for oid, _ in questions[0].options] == ["true", "false"]


def test_encode_truncates_the_state_and_refuses_bad_requests(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    small: dict[str, object] = {"state": "x" * 50, "questions": {"q": {"type": "noul"}}}
    full = clip_models.encode(characters, small).ids
    monkeypatch.setattr(clip_models, "MAX_LENGTH", len(full) - 10)
    shortened = clip_models.encode(characters, small)
    cut, questions = shortened.ids, shortened.questions
    assert len(cut) == len(full) - 10
    assert text_of(cut, questions[0].span) == "q"
    monkeypatch.setattr(clip_models, "MAX_LENGTH", 20)
    with pytest.raises(ReplayError, match="maximum is 20"):
        clip_models.encode(characters, small)
    with pytest.raises(ReplayError, match="needs a state"):
        clip_models.encode(characters, {"questions": {"q": {"type": "noul"}}})
    with pytest.raises(ReplayError, match="q: type must be"):
        clip_models.encode(characters, {"state": 1, "questions": {"q": {"type": "x"}}})
    with pytest.raises(ReplayError, match="questions must be an object"):
        clip_models.encode(characters, {"state": 1, "questions": []})


def test_head_gives_one_logit_per_option() -> None:
    backbone = FakeBackbone()
    encoded = clip_models.encode(backbone.tokenize, REQUEST)
    logits = clip_models.head(
        weights(), CONFIG, backbone.inputs(encoded, weights()), encoded.questions
    )
    assert [[round(float(value), 3) for value in row] for row in logits] == [
        [0.714, 0.715],
        [0.696, 0.705],
        [1.009, 1.011],
    ]


def test_the_metal_head_matches_the_cpu_head() -> None:
    mx = importlib.import_module("mlx.core")
    backbone = FakeBackbone()
    encoded = clip_models.encode(backbone.tokenize, REQUEST)
    inputs = backbone.inputs(encoded, weights())
    cpu = clip_models.head(weights(), CONFIG, inputs, encoded.questions)
    metal = clip_models.head(weights(), CONFIG, inputs, encoded.questions, gpu=mx)
    assert [[round(float(value), 3) for value in row] for row in metal] == [
        [0.714, 0.715],
        [0.696, 0.705],
        [1.009, 1.011],
    ]
    # Metal sums float32 products in another order (and in tiles): about 1e-4 apart, not bit-equal.
    assert max(float(np.max(np.abs(a - b))) for a, b in zip(cpu, metal, strict=True)) < 5e-4


def test_answers_take_the_systemone_form() -> None:
    assert clip_models.answer({"type": "noul"}, {"true": 0.25, "false": 0.75}) == {
        "type": "noul",
        "noul": 0.25,
    }
    assert clip_models.answer(
        {"type": "choice", "criteria": {"z": "Z", "y": "Y"}}, {"y": 0.6, "z": 0.4}
    ) == {"type": "choice", "choice": "y", "confidence": 0.6, "probabilities": {"z": 0.4, "y": 0.6}}
    assert clip_models.answer({"type": "score"}, {"0": 0.5, "1": 0.25, "2": 0.25}) == {
        "type": "score",
        "score": 0.75,
        "confidence": 0.5,
        "probabilities": {"0": 0.5, "1": 0.25, "2": 0.25},
    }


def test_decider_answers_every_question_with_usage() -> None:
    reply = clip_models.Decider(FakeBackbone(), weights(), CONFIG).respond(REQUEST)
    assert reply["model"] == "clef"
    assert reply["usage"] == {"input_tokens": 894, "output_tokens": 0}
    assert reply["answers"] == {
        "ok": {"type": "noul", "noul": 0.4998},
        "tone": {
            "type": "choice",
            "choice": "z",
            "confidence": 0.5023,
            "probabilities": {"z": 0.5023, "y": 0.4977},
        },
        "level": {
            "type": "score",
            "score": 0.5005,
            "confidence": 0.5005,
            "probabilities": {"0": 0.4995, "1": 0.5005},
        },
    }


class FakeArray:
    def __init__(self, value: object) -> None:
        self.value = np.asarray(value)

    def astype(self, dtype: object) -> np.ndarray:
        assert dtype == "f32"
        return self.value.astype(np.float32)

    def __getitem__(self, index: "FakeArray | int | None") -> "FakeArray":
        if index is None:
            return FakeArray(self.value[np.newaxis])
        if isinstance(index, int):
            return FakeArray(self.value[index])
        return FakeArray(self.value[index.value.astype(np.intp)])


class FakeTokenizer:
    def encode(self, text: str, add_special_tokens: bool) -> list[int]:
        assert add_special_tokens is False
        return [len(text)]

    def apply_chat_template(self, messages: list[object], **options: object) -> str:
        return f"{len(messages)} {sorted(options.items())}"


def fake_mlx(quantized: bool, tied: bool) -> SimpleNamespace:
    weight = FakeArray(np.arange(12).reshape(4, 3))
    output = SimpleNamespace(
        weight=weight,
        scales=FakeArray(np.ones(4)),
        biases=FakeArray(np.zeros(4)),
        group_size=64,
        bits=4,
    )
    plain = SimpleNamespace(weight=weight)
    chunks: list[int] = []

    def text(inputs: FakeArray, cache: object) -> FakeArray:
        assert cache == "cache"
        chunks.append(inputs.value.shape[1])
        return FakeArray(np.ones((1, inputs.value.shape[1], 3)))

    lm = SimpleNamespace(
        model=SimpleNamespace(embed_tokens=plain, layers=[]),
        lm_head=output if quantized else plain,
        args=SimpleNamespace(tie_word_embeddings=tied),
    )
    lm.model.__call__ = text
    model = SimpleNamespace(language_model=lm, make_cache=lambda: "cache")
    core = SimpleNamespace(
        float32="f32",
        array=lambda values: FakeArray(values),
        clear_cache=lambda: None,
        dequantize=lambda w, scales, biases, g, bits: FakeArray(w.value * 2 + g + bits),
        load=lambda path: {"prior_logit_scale": FakeArray(0.5)},
    )
    return SimpleNamespace(core=core, model=model, text=text, chunks=chunks)


class FakeKeys:
    """An attention layer's cache: keys/values are the ids it has seen, as (1, 1, n, 1)."""

    def __init__(self) -> None:
        self.state: tuple[object, ...] = (np.zeros((1, 1, 0, 1)), np.zeros((1, 1, 0, 1)), 0)


class FakeRecurrent:
    """A recurrent layer's cache: its running sum of the ids, kept in a list it replaces."""

    def __init__(self) -> None:
        self.state: tuple[object, ...] = ([np.zeros(1)], None, None)


def numpy_mlx(prefilled: list[int]) -> SimpleNamespace:
    """mlx.core and a two-layer model on numpy: each output row depends on everything before it."""

    def text(inputs: np.ndarray, cache: list[FakeKeys | FakeRecurrent]) -> np.ndarray:
        keys, recurrent = cache
        ids = inputs[0].astype(np.float32)
        prefilled.append(len(ids))
        past = np.asarray(keys.state[0])
        seen = np.concatenate([past[0, 0, :, 0], ids])
        keys.state = (seen[None, None, :, None], seen[None, None, :, None], len(seen))
        carried = recurrent.state[0]
        assert isinstance(carried, list)
        total = np.asarray(carried[0]) + np.cumsum(ids)
        recurrent.state = ([total[-1:]], None, None)
        rows = np.stack([np.sin(total + k) for k in range(HIDDEN)], axis=-1)
        return (rows * (1 + len(seen) / 100))[None].astype(np.float32)

    output = SimpleNamespace(
        weight=np.arange(60 * HIDDEN, dtype=np.float32).reshape(60, HIDDEN) / 60
    )
    lm = SimpleNamespace(
        model=SimpleNamespace(embed_tokens=output, layers=[]),
        lm_head=output,
        args=SimpleNamespace(tie_word_embeddings=True),
    )
    model = SimpleNamespace(language_model=lm, make_cache=lambda: [FakeKeys(), FakeRecurrent()])
    core = SimpleNamespace(
        float32=np.float32,
        array=np.asarray,
        mean=np.mean,
        sqrt=np.sqrt,
        concatenate=np.concatenate,
        transpose=np.transpose,
        take=np.take,
        arange=np.arange,
        clear_cache=lambda: None,
    )
    return SimpleNamespace(core=core, model=model, text=text)


def numpy_backbone(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, prefilled: list[int]
) -> clip_models.MlxBackbone:
    fake = numpy_mlx(prefilled)
    tokenizer = SimpleNamespace(encode=lambda text, add_special_tokens: [ord(c) % 50 for c in text])
    modules = {
        "mlx.core": fake.core,
        "mlx_lm": SimpleNamespace(load=lambda path: (fake.model, tokenizer)),
    }
    monkeypatch.setattr(clip_models, "import_module", modules.__getitem__)
    backbone = clip_models.MlxBackbone(tmp_path)
    backbone.text = fake.text
    return backbone


def test_the_backbone_reuses_a_shared_state_and_chunks_every_request_the_same_way(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setattr(clip_models, "PREFILL_CHUNK", 2)
    prefilled: list[int] = []
    backbone = numpy_backbone(monkeypatch, tmp_path, prefilled)
    first = np.asarray(backbone.hidden([1, 2, 3, 4, 5], 3))
    second = np.asarray(backbone.hidden([1, 2, 3, 9, 8, 7], 3))
    fresh = numpy_backbone(monkeypatch, tmp_path, [])
    assert prefilled == [2, 1, 2, 2, 1]
    assert np.allclose(second, np.asarray(fresh.hidden([1, 2, 3, 9, 8, 7], 3)))
    assert first.shape == (5, HIDDEN)
    backbone.hidden([4, 2, 3, 1], 3)
    assert prefilled == [2, 1, 2, 2, 1, 2, 1, 1]
    assert backbone.tokenize("ab") == [47, 48]


def test_the_gpu_head_inputs_match_the_cpu_reference(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    backbone = numpy_backbone(monkeypatch, tmp_path, [])
    encoded = clip_models.encode(backbone.tokenize, REQUEST)
    gpu = backbone.inputs(encoded, weights())
    cpu = clip_models.head_inputs(
        weights(), np.asarray(backbone.hidden(encoded.ids, encoded.cut)), encoded.questions,
        backbone.lexicon, encoded.ids,
    )  # fmt: skip
    assert np.allclose(gpu.memory, cpu.memory, atol=1e-5)
    assert np.allclose(gpu.last, cpu.last, atol=1e-5)
    assert np.allclose(gpu.questions, cpu.questions, atol=1e-5)
    assert all(np.allclose(a, b, atol=1e-5) for a, b in zip(gpu.options, cpu.options, strict=True))
    assert all(np.allclose(a, b, atol=1e-5) for a, b in zip(gpu.lexical, cpu.lexical, strict=True))


def test_the_backbone_dequantizes_lexicon_rows(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    fake = fake_mlx(quantized=True, tied=False)
    lm_module = SimpleNamespace(load=lambda path: (fake.model, FakeTokenizer()))
    monkeypatch.setattr(
        clip_models, "import_module", {"mlx.core": fake.core, "mlx_lm": lm_module}.__getitem__
    )
    assert clip_models.MlxBackbone(tmp_path).lexicon([1]).tolist() == [[74.0, 76.0, 78.0]]
    plain = fake_mlx(quantized=False, tied=True)
    monkeypatch.setattr(
        clip_models,
        "import_module",
        {
            "mlx.core": plain.core,
            "mlx_lm": SimpleNamespace(load=lambda path: (plain.model, 0)),
        }.__getitem__,
    )
    assert clip_models.MlxBackbone(tmp_path).lexicon([2]).tolist() == [[6.0, 7.0, 8.0]]


def quantized_linear(weight: np.ndarray, bits: int) -> SimpleNamespace:
    return SimpleNamespace(
        weight=weight, scales=np.ones(len(weight)), biases=np.zeros(len(weight)),
        bits=bits, group_size=64, mode="affine",
    )  # fmt: skip


def test_the_backbone_fuses_gate_and_up_where_they_are_quantized_alike(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    gate = np.array([[1.0, 0.0], [0.0, 2.0]], np.float32)
    up = np.array([[3.0, 1.0], [1.0, 1.0]], np.float32)
    alike = SimpleNamespace(
        gate_proj=quantized_linear(gate, 4),
        up_proj=quantized_linear(up, 4),
        down_proj=lambda h: h * 10,
    )
    mixed = SimpleNamespace(gate_proj=quantized_linear(gate, 4), up_proj=quantized_linear(up, 8))
    dense = SimpleNamespace(
        gate_proj=SimpleNamespace(weight=gate), up_proj=SimpleNamespace(weight=up)
    )
    layers = [SimpleNamespace(mlp=alike), SimpleNamespace(mlp=mixed), SimpleNamespace(mlp=dense)]
    layers.append(SimpleNamespace(mlp=SimpleNamespace(experts=[])))
    lm = SimpleNamespace(
        model=SimpleNamespace(embed_tokens=None, layers=layers),
        args=SimpleNamespace(tie_word_embeddings=True),
    )
    core = SimpleNamespace(
        concatenate=np.concatenate,
        quantized_matmul=lambda x, w, scales, biases, transpose, group_size, bits, mode: x @ w.T,
        split=lambda a, at, axis: np.split(a, at, axis=axis),
    )
    modules = {
        "mlx.core": core,
        "mlx_lm": SimpleNamespace(load=lambda path: (SimpleNamespace(language_model=lm), 0)),
        "mlx_lm.models.activations": SimpleNamespace(swiglu=lambda g, x: g * x),
    }
    monkeypatch.setattr(clip_models, "import_module", modules.__getitem__)
    clip_models.MlxBackbone(tmp_path)
    assert isinstance(layers[0].mlp, clip_models.FusedMlp)
    assert layers[1].mlp is mixed
    assert layers[2].mlp is dense
    assert np.asarray(layers[0].mlp(np.array([[1.0, 2.0]], np.float32))).tolist() == [[50.0, 120.0]]


def test_load_decider_reads_the_head_config_and_weights(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    fake = fake_mlx(quantized=True, tied=False)
    modules = {
        "mlx.core": fake.core,
        "mlx_lm": SimpleNamespace(load=lambda path: (fake.model, FakeTokenizer())),
    }
    monkeypatch.setattr(clip_models, "import_module", modules.__getitem__)
    (tmp_path / "joint_head_config.json").write_text(
        json.dumps({"heads": 16, "routing_layers": 2, "layers": 4, "width": 1}), encoding="utf-8"
    )
    decider = clip_models.load_decider(tmp_path)
    assert decider.config == clip_models.HeadConfig(heads=16, routing_layers=2, layers=4)
    assert float(decider.weights["prior_logit_scale"]) == 0.5
    assert decider.gpu is fake.core


def test_chat_renders_without_thinking_and_caps_the_answer_length(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    asked: list[tuple[str, int]] = []

    def generate(model: object, processor: object, prompt: str, **options: object) -> object:
        asked.append((prompt, int(str(options["max_tokens"]))))
        return SimpleNamespace(text='{"clips": []}')

    vlm = SimpleNamespace(
        load=lambda path: ("model", SimpleNamespace(tokenizer=FakeTokenizer())), generate=generate
    )
    monkeypatch.setattr(clip_models, "import_module", {"mlx_vlm": vlm}.__getitem__)
    chat = clip_models.MlxChat(tmp_path)
    messages = [{"role": "user", "content": "x"}]
    assert chat.respond({"messages": messages, "max_completion_tokens": 6000}) == {
        "choices": [{"message": {"role": "assistant", "content": '{"clips": []}'}}]
    }
    chat.respond({"messages": messages, "max_completion_tokens": 30})
    assert asked == [
        (
            "1 [('add_generation_prompt', True), ('enable_thinking', False), ('tokenize', False)]",
            1024,
        ),
        (
            "1 [('add_generation_prompt', True), ('enable_thinking', False), ('tokenize', False)]",
            30,
        ),
    ]
    with pytest.raises(ReplayError, match="needs messages"):
        chat.respond({"messages": []})


def test_free_memory_is_read_from_memory_pressure() -> None:
    report = "The system has 1 pages\nSystem-wide memory free percentage: 63%\n"
    assert clip_models.free_percent(lambda command: report) == 63
    with pytest.raises(ReplayError, match="did not report"):
        clip_models.free_percent(lambda command: "")


class Echo:
    def respond(self, body: dict[str, object]) -> dict[str, object]:
        if body.get("fail"):
            raise ReplayError("bad request")
        return {"echo": body}


def post(port: int, path: str, body: bytes) -> tuple[int, dict[str, object]]:
    request = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=body, method="POST")
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            return response.status, json.loads(response.read())
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read())


def test_server_routes_by_path_and_reports_bad_requests() -> None:
    server = HTTPServer(("127.0.0.1", 0), clip_models.handler({"clef": Echo()}))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    port = server.server_address[1]
    try:
        assert post(port, "/clef", b'{"a": 1}') == (200, {"echo": {"a": 1}})
        assert post(port, "/other", b"{}") == (404, {"error": "unknown model /other"})
        assert post(port, "/clef", b'{"fail": true}') == (400, {"error": "bad request"})
        assert post(port, "/clef", b"[1]") == (400, {"error": "request must be an object"})
        assert post(port, "/clef", b"{")[0] == 400
    finally:
        server.shutdown()
        server.server_close()


class FakeServer:
    def __init__(self, address: tuple[str, int], handler: type) -> None:
        self.address = address

    def serve_forever(self) -> None:
        return None


def test_main_checks_models_and_memory_before_serving(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    clef, flash, lfm3 = tmp_path / "clef", tmp_path / "flash", tmp_path / "lfm3"
    bonsai, laya = tmp_path / "bonsai", tmp_path / "laya"
    arguments = ["--port", "8791", "--clef", str(clef), "--flash", str(flash)]
    arguments += ["--lfm3", str(lfm3), "--bonsai", str(bonsai), "--laya", str(laya)]
    with pytest.raises(ReplayError, match="never downloaded here"):
        clip_models.main(arguments, run=lambda c: "", server=FakeServer)
    clef.mkdir()
    flash.mkdir()
    lfm3.mkdir()
    (clef / "config.json").write_text("{}", encoding="utf-8")
    (flash / "config.json").write_text("{}", encoding="utf-8")
    (lfm3 / "config.json").write_text("{}", encoding="utf-8")
    with pytest.raises(ReplayError, match="bonsai"):
        clip_models.main([*arguments, "--judges"], run=lambda c: "", server=FakeServer)
    bonsai.mkdir()
    (bonsai / "config.json").write_text("{}", encoding="utf-8")
    laya.mkdir()
    (laya / "mlx_config.json").write_text("{}", encoding="utf-8")
    low = "System-wide memory free percentage: 12%"
    with pytest.raises(ReplayError, match="Only 12% of memory is free"):
        clip_models.main(arguments, run=lambda c: low, server=FakeServer)
    high = "System-wide memory free percentage: 70%"
    loaded: list[str] = []

    def decider(path: Path) -> Echo:
        loaded.append(path.name)
        return Echo()

    def writer(path: Path) -> Echo:
        loaded.append(path.name)
        return Echo()

    for extra in ([], ["--judges"]):
        assert (
            clip_models.main(
                arguments + extra,
                run=lambda c: high,
                decider=decider,
                writer=writer,
                reader=writer,
                server=FakeServer,
                guard=lambda run: None,
            )
            == 0
        )
    assert loaded == ["clef", "flash", "lfm3", "clef", "flash", "lfm3", "bonsai", "laya"]
    assert capsys.readouterr().out == "ready 8791\nready 8791\n"


def test_the_watch_stops_the_server_when_memory_runs_low() -> None:
    reports = iter(
        ["System-wide memory free percentage: 40%", "System-wide memory free percentage: 9%"]
    )
    stopped: list[int] = []
    slept: list[float] = []
    clip_models.watch(lambda c: next(reports), stopped.append, sleep=slept.append)
    assert stopped == [1]
    assert slept == [30.0, 30.0]
    calm: list[int] = []
    clip_models.watch(
        lambda c: "System-wide memory free percentage: 50%",
        calm.append,
        sleep=slept.append,
        rounds=2,
    )
    assert calm == []


def test_laya_answers_the_rubric_request_and_refuses_one_without_a_state(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    asked: list[object] = []

    class Agent:
        def predict(self, state: object, questions: object) -> dict[str, object]:
            asked.append((state, questions))
            return {
                "answers": {"good": {"type": "noul", "noul": 0.5}},
                "usage": {"input_tokens": 9},
            }

    loaded: list[str] = []

    def load(path: str) -> Agent:
        loaded.append(path)
        return Agent()

    monkeypatch.setattr(
        clip_models, "import_module", {"laya_mlx": SimpleNamespace(load=load)}.__getitem__
    )
    laya = clip_models.MlxLaya(tmp_path)
    reply = laya.respond(
        {"model": "laya", "state": {"u": 1}, "questions": {"good": {"type": "noul"}}}
    )
    assert reply == {
        "model": "laya",
        "answers": {"good": {"type": "noul", "noul": 0.5}},
        "usage": {"input_tokens": 9},
    }
    assert asked == [({"u": 1}, {"good": {"type": "noul"}})]
    assert loaded == [str(tmp_path)]
    with pytest.raises(ReplayError, match="needs a state"):
        laya.respond({"questions": {}})
