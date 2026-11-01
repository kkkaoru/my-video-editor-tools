"""The one-chunk job wires real collaborators around synthetic media and a fake native tool."""

import json
import subprocess
import sys
import threading
from collections.abc import Sequence
from fractions import Fraction
from pathlib import Path

import numpy as np
import pytest
import torch
from numpy.typing import NDArray

from replay import subtitle_band_audit as band_audit
from replay import subtitle_colour_audit as colour_audit
from replay import subtitle_colour_scene as colour_scene
from replay import subtitle_detector_mps as mps
from replay import subtitle_sampled_job as job
from replay import subtitle_sampled_run as runner
from replay import subtitle_yolo_regions as yolo_regions
from replay import yolo_boxes
from replay.caption_io import FrameObservation
from replay.caption_masks import TextDetection, TextRectangle
from replay.local_asr import fingerprint
from replay.recordings import record
from replay.subtitle_clock import VideoFrameClock
from replay.subtitle_repair_audit import Residual
from replay.subtitle_stream import SubtitleSpan
from replay.subtitle_video import VideoRepairJob

WIDTH = 320
HEIGHT = 240
FRAMES = 40
START = 5 * 90000  # the chunk starts five seconds into the source
PTS = [START + offset for offset in range(0, FRAMES * 3000, 3000)]
LINE = {"x": 60, "y": 90, "width": 200, "height": 30}

FAKE_NATIVE = """#!{python}
import json, sys
from pathlib import Path

request = json.loads(Path(sys.argv[3]).read_text())
repaired = request["path"].endswith("candidate.mp4")
confidence = {low} if repaired else 0.9
if repaired and any(s["value"] / s["timescale"] > 2 for s in request["samples"]):
    print("Invalid request: Frame sample lies outside source duration", file=sys.stderr)
    sys.exit(2)
frames = [
    {{
        "requestedTimeSeconds": s["value"] / s["timescale"],
        "actualTimeSeconds": s["value"] / s["timescale"],
        "width": request["width"],
        "height": request["height"],
        "lines": [{{"text": "こんにちは世界です", "confidence": confidence, "region": {line}}}],
    }}
    for s in request["samples"]
]
directory = Path(request["checkpointDirectory"])
for number in range(0, len(frames), 8):
    (directory / f"block-{{number // 8}}.json").write_text(json.dumps(frames[number:number + 8]))
"""


def _bar(batch: NDArray[np.float32]) -> NDArray[np.float32]:
    count, _, height, width = batch.shape
    maps = np.zeros((count, 1, height, width), dtype=np.float32)
    maps[:, :, int(height * 0.40) : int(height * 0.50), width // 8 : width // 2] = 0.95
    return maps


def _configuration(tmp_path: Path, *, residual: float) -> dict[str, object]:
    source = tmp_path / "source.mp4"
    subprocess.run(
        ["ffmpeg", "-nostdin", "-v", "error", "-f", "lavfi", "-i",
         f"color=c=black:s={WIDTH}x{HEIGHT}:r=30:d=8", "-pix_fmt", "yuv420p", "-y", str(source)],
        check=True,
        timeout=60,
    )  # fmt: skip
    profile = tmp_path / "profile"
    profile.mkdir()
    observations = [
        {
            "requestedTimeSeconds": 0.0,
            "actualTimeSeconds": 0.0,
            "width": WIDTH,
            "height": HEIGHT,
            "lines": [{"text": text, "confidence": 0.9, "region": LINE}],
        }
        # Captions change their wording; one text with one-letter variants would be a fixed label.
        for text in (
            "こんにちは世界です",
            "今日はいい天気ですね",
            "配信を始めます",
            "ありがとうございます",
            "それでは歌います",
            "コメント読みます",
            "また明日会いましょう",
            "楽しかったです",
            "少し休憩します",
            "おかえりなさい",
            "準備ができました",
            "最後まで見てね",
        )
    ]
    (profile / "0000.result.json").write_text(json.dumps(observations[:8]), encoding="utf-8")
    (profile / "0001.result.json").write_text(json.dumps(observations[8:]), encoding="utf-8")
    native = tmp_path / "native"
    native.write_text(
        FAKE_NATIVE.format(python=sys.executable, low=residual, line=json.dumps(LINE)),
        encoding="utf-8",
    )
    native.chmod(0o700)
    return {
        "source": str(source),
        "native": str(native),
        "clock": {"pts": PTS, "timeBase": "1/90000", "lastDuration": 3000},
        "width": WIDTH,
        "height": HEIGHT,
        "checkoutRoot": str(tmp_path),
        "output": str(tmp_path / "chunk"),
        "profileDirectory": str(profile),
    }


def _fake_render(video_job: VideoRepairJob, _detector: job.Detects) -> None:
    video_job.output.write_bytes(b"video")


def test_sampled_attempt_runs_plan_render_and_audit(tmp_path: Path) -> None:
    config = _configuration(tmp_path, residual=0.3)
    tools, pins = job.real_collaborators(config, network=_bar, render=_fake_render)
    receipt = runner.run(tmp_path / "chunk", tools, stride=13, pins=pins)
    assert receipt["accepted"] == "attempt-sampled"
    assert receipt["escalated"] is False
    admission = json.loads(
        (tmp_path / "chunk" / "attempt-sampled" / "admission.json").read_text(encoding="utf-8")
    )
    assert admission["sampling"]["sampledFrames"][0] == 0
    assert pins["native"] == fingerprint(tmp_path / "native")
    learning = receipt["learning"]
    assert isinstance(learning, dict)
    assert (learning["noSeedReason"], learning["seededFrames"]) == ("no band confirmed yet", [0])


def test_the_video_may_live_in_a_separate_media_root(tmp_path: Path) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config["mediaOutput"] = str(tmp_path / "media")
    config["output"] = str(tmp_path / "state")
    tools, _ = job.real_collaborators(config, network=_bar, render=_fake_render)
    receipt = runner.run(tmp_path / "state", tools, stride=13)
    assert receipt["accepted"] == "attempt-sampled"
    assert (tmp_path / "media" / "attempt-sampled" / "candidate.mp4").read_bytes() == b"video"
    assert (tmp_path / "state" / "attempt-sampled" / "audit" / "child-000").is_dir()


def test_a_confident_residual_escalates_and_fails_without_acceptance(tmp_path: Path) -> None:
    config = _configuration(tmp_path, residual=1.0)
    tools, _ = job.real_collaborators(config, network=_bar, render=_fake_render)
    with pytest.raises(ValueError, match="nothing accepted"):
        runner.run(tmp_path / "chunk", tools, stride=13)
    receipt = json.loads((tmp_path / "chunk" / "chunk-receipt.json").read_text(encoding="utf-8"))
    assert receipt["escalated"] is True
    assert receipt["accepted"] is None


def test_missing_profile_observations_are_rejected(tmp_path: Path) -> None:
    config = _configuration(tmp_path, residual=0.3)
    empty = tmp_path / "empty"
    empty.mkdir()
    config["profileDirectory"] = str(empty)
    with pytest.raises(ValueError, match="profile"):
        job.real_collaborators(config, network=_bar, render=_fake_render)


def test_invalid_clock_is_rejected(tmp_path: Path) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config["clock"] = {"pts": ["x"], "timeBase": "1/90000", "lastDuration": 3000}
    with pytest.raises(ValueError, match="clock"):
        job.real_collaborators(config, network=_bar, render=_fake_render)
    config["clock"] = {"pts": PTS, "timeBase": 1, "lastDuration": 3000}
    with pytest.raises(ValueError, match="clock"):
        job.real_collaborators(config, network=_bar, render=_fake_render)


def test_main_loads_the_pinned_network_and_runs(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config.update(output=str(tmp_path / "out"), stride=13)

    class Head(torch.nn.Module):
        def forward(self, x: torch.Tensor) -> torch.Tensor:
            return x[:, :1] * 0.0

    model = tmp_path / "head.pt2"
    torch.export.save(torch.export.export(Head(), (torch.zeros(1, 3, 4, 6),)), str(model))
    config.update(detectorProgram=str(model), detectorProgramSHA256=fingerprint(model))
    path = tmp_path / "config.json"
    path.write_text(json.dumps(config), encoding="utf-8")
    monkeypatch.setattr(
        job, "render_repaired_video_fast", lambda video_job, **_: video_job.output.write_bytes(b"v")
    )
    monkeypatch.setattr(mps, "lazy_network", lambda *_args, **_kw: _bar)
    assert job.main([str(path)]) == 0
    assert json.loads(capsys.readouterr().out) == {
        "accepted": "attempt-sampled",
        "escalated": False,
    }


def test_main_stores_the_candidates_of_a_chunk_that_failed_every_attempt(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config.update(output=str(tmp_path / "work" / "state" / "chunk-00007"), stride=13)
    config.update(detectorProgram=str(tmp_path / "x.pt2"), detectorProgramSHA256="0")
    path = tmp_path / "config.json"
    path.write_text(json.dumps(config), encoding="utf-8")
    monkeypatch.setattr(mps, "lazy_network", lambda *_args, **_kw: _bar)
    line = {"frame": 1, "text": "X", "confidence": 1.0, "box": [10.0, 20.0, 30.0, 8.0]}

    def failed(output: Path, *_args: object, **_kwargs: object) -> dict[str, object]:
        output.mkdir(parents=True)
        receipt = {"accepted": None, "attempts": [{"directory": "a", "residualLines": [line]}]}
        (output / "chunk-receipt.json").write_text(json.dumps(receipt), encoding="utf-8")
        raise ValueError("Audit failed after escalation; receipt retained, nothing accepted")

    monkeypatch.setattr(runner, "run", failed)
    with pytest.raises(ValueError, match="nothing accepted"):
        job.main([str(path)])
    stored = (tmp_path / "work" / "learned" / "residuals.jsonl").read_text(encoding="utf-8")
    assert [json.loads(row) for row in stored.splitlines()] == [
        {"chunk": "chunk-00007", "box": [10, 20, 40, 28], "status": "verified", "seconds": 5.033333}
    ]


def test_main_without_a_retained_receipt_stores_nothing(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config.update(output=str(tmp_path / "work" / "state" / "chunk-00007"), stride=13)
    config.update(detectorProgram=str(tmp_path / "x.pt2"), detectorProgramSHA256="0")
    path = tmp_path / "config.json"
    path.write_text(json.dumps(config), encoding="utf-8")
    monkeypatch.setattr(mps, "lazy_network", lambda *_args, **_kw: _bar)

    def broken(*_args: object, **_kwargs: object) -> dict[str, object]:
        raise ValueError("Native OCR session failed; evidence retained")

    monkeypatch.setattr(runner, "run", broken)
    with pytest.raises(ValueError, match="Native OCR"):
        job.main([str(path)])
    assert not (tmp_path / "work" / "learned").exists()


def test_main_rejects_an_oversized_configuration(tmp_path: Path) -> None:
    path = tmp_path / "config.json"
    path.write_bytes(b"x" * (job.MAX_CONFIG_BYTES + 1))
    with pytest.raises(ValueError, match="byte budget"):
        job.main([str(path)])


def test_fraction_clock_round_trip() -> None:
    clock = job._clock({"pts": PTS[:3], "timeBase": "1/90000", "lastDuration": 3000})
    assert clock.time_base == Fraction(1, 90000)


def _detecting_render(video_job: VideoRepairJob, detector: job.Detects) -> None:
    for span in video_job.spans:
        for _ in range(span.start_frame, span.end_frame):
            detector.detect([np.zeros((span.height, span.width, 3), dtype=np.uint8)])
    video_job.output.write_bytes(b"video")


def _partial_render(video_job: VideoRepairJob, detector: job.Detects) -> None:
    span = video_job.spans[0]
    detector.detect([np.zeros((span.height, span.width, 3), dtype=np.uint8)])
    video_job.output.write_bytes(b"video")


def test_colour_key_wraps_the_detector_and_is_pinned(tmp_path: Path) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config["colourKey"] = True
    tools, pins = job.real_collaborators(config, network=_bar, render=_detecting_render)
    receipt = runner.run(tmp_path / "chunk", tools, stride=13, pins=pins)
    pinned = receipt["pins"]
    assert isinstance(pinned, dict)
    assert (receipt["accepted"], pinned["colourKey"]) == ("attempt-sampled", "pink-key-v1")


def test_colour_key_requires_every_planned_frame_to_be_consumed(tmp_path: Path) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config["colourKey"] = True
    tools, _ = job.real_collaborators(config, network=_bar, render=_partial_render)
    with pytest.raises(ValueError, match="not all consumed"):
        runner.run(tmp_path / "chunk", tools, stride=13)


def test_key_frames_skip_unknown_frames_and_read_span_origins() -> None:
    admitted: dict[str, object] = {
        "frames": [
            {"classification": "unknown"},
            {"classification": "overlay-subtitle", "lineBoxes": [[1, 2, 30, 40]]},
        ]
    }
    spans = (SubtitleSpan(1, 2, 7, 9, 64, 32),)
    frames = job.key_frames(admitted, spans)
    assert [(f.origin, f.boxes) for f in frames] == [((7, 9), ((1, 2, 30, 40),))]


def test_key_frames_key_an_unread_detected_region() -> None:
    region = {"x": 3, "y": 4, "width": 10, "height": 20}
    admitted: dict[str, object] = {
        "frames": [{"classification": "overlay-subtitle", "lineBoxes": [], "region": region}]
    }
    frames = job.key_frames(admitted, (SubtitleSpan(0, 1, 0, 0, 64, 32),))
    assert [f.boxes for f in frames] == [((3, 4, 13, 24),)]


def test_key_frames_reject_missing_boxes_spans_and_entries() -> None:
    span = (SubtitleSpan(0, 1, 0, 0, 64, 32),)
    with pytest.raises(ValueError, match="line boxes"):
        job.key_frames({"frames": [{"classification": "overlay-subtitle"}]}, span)
    with pytest.raises(ValueError, match="no repair span"):
        job.key_frames({"frames": [{"classification": "overlay-subtitle", "lineBoxes": []}]}, ())
    with pytest.raises(ValueError, match="per-frame entries"):
        job.key_frames({"frames": 3}, span)


def test_colour_gate_collaborator_counts_unchanged_caption_colour(tmp_path: Path) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config["colourGate"] = True
    tools, _ = job.real_collaborators(config, network=_bar, render=_fake_render)
    assert tools.colour is not None


def test_colour_regions_collaborator_reads_only_the_flagged_frames(tmp_path: Path) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config["colourGate"] = True
    tools, _ = job.real_collaborators(config, network=_bar, render=_fake_render)
    assert tools.colour_regions is not None
    admitted: dict[str, object] = {"frames": [{"lineBoxes": [[10, 10, 40, 20]]}] * 240}
    # The black source has no caption colour: nothing left to repair in the flagged frames.
    assert tools.colour_regions(admitted, Path(str(config["source"])), [5, 2, 5]) == []
    with pytest.raises(ValueError, match="per-frame entries"):
        tools.colour_regions({"frames": 3}, Path(str(config["source"])), [2])


def test_the_moving_interface_rule_uses_the_profile_vocabulary(tmp_path: Path) -> None:
    tools, _ = job.real_collaborators(
        _configuration(tmp_path, residual=0.3), network=_bar, render=_fake_render
    )
    assert tools.moving_interface is not None
    lone = FrameObservation(
        0, 0, WIDTH, HEIGHT, (TextDetection("決着", 1.0, TextRectangle(1, 2, 30, 20)),)
    )
    assert tools.moving_interface({0: lone}, 0, 0) is False


def test_a_misread_still_label_is_interface_when_its_label_read_holds_no_caption_colour(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config["colourGate"] = True
    tools, _ = job.real_collaborators(config, network=_bar, render=_fake_render)
    assert tools.moving_interface is not None
    misread = TextDetection("上分に目", 0.3, TextRectangle(10, 10, 60, 20))
    label = TextDetection("桐おさき", 1.0, TextRectangle(12, 10, 60, 20))
    observed = {0: FrameObservation(0, 0, WIDTH, HEIGHT, (misread,))}
    assert tools.moving_interface(observed, 0, 0) is False  # no label read at its place
    monkeypatch.setattr(job, "still_label_reads", lambda *_args: [(5, label)])
    assert tools.moving_interface(observed, 0, 0) is True  # the black source holds no pink


def test_interface_boxes_grow_by_their_height_and_skip_any_another_line_overlaps() -> None:
    gift = TextDetection("星降りメリー", 1.0, TextRectangle(100, 1400, 200, 40))
    beside = TextDetection("星降りメリー", 1.0, TextRectangle(600, 1300, 200, 40))
    caption = TextDetection("無理だった", 1.0, TextRectangle(560, 1290, 300, 40))
    observed = {0: FrameObservation(0, 0, WIDTH, HEIGHT, (gift, beside, caption))}
    excluded: list[list[tuple[int, int, int, int]]] = [[]]

    def interface(_observed: object, _frame: int, index: int) -> bool:
        return index < 2

    assert job._interface_boxes(observed, 0, excluded, interface) is True
    assert excluded == [[(60, 1360, 341, 1481)]]  # the one beside the caption is kept counted
    none: list[list[tuple[int, int, int, int]]] = [[]]
    assert job._interface_boxes(observed, 0, none, lambda *_args: False) is False


def test_without_the_colour_gate_there_are_no_colour_regions(tmp_path: Path) -> None:
    tools, _ = job.real_collaborators(
        _configuration(tmp_path, residual=0.3), network=_bar, render=_fake_render
    )
    assert tools.colour_regions is None


def test_entry_boxes_reject_non_list_boxes() -> None:
    assert job._entry_boxes({"lineBoxes": [[1, 2, 3, 4]]}) == [(1, 2, 3, 4)]
    assert job._entry_boxes({}) == []
    with pytest.raises(ValueError, match="must be a list"):
        job._entry_boxes({"lineBoxes": 4})


def test_caption_coloured_collaborator_classifies_residuals_by_source_pink(tmp_path: Path) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config["colourGate"] = True
    tools, _ = job.real_collaborators(config, network=_bar, render=_fake_render)
    assert tools.caption_coloured is not None
    residual = Residual(0, "UI", 1.0, (10.0, 10.0, 30.0, 10.0))
    assert tools.caption_coloured([residual]) == [False]


def test_caption_coloured_collaborator_needs_every_residual_box(tmp_path: Path) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config["colourGate"] = True
    tools, _ = job.real_collaborators(config, network=_bar, render=_fake_render)
    assert tools.caption_coloured is not None
    with pytest.raises(ValueError, match="lack their boxes"):
        tools.caption_coloured([Residual(0, "UI", 1.0)])


def test_without_the_colour_gate_there_is_no_interface_classifier(tmp_path: Path) -> None:
    tools, _ = job.real_collaborators(
        _configuration(tmp_path, residual=0.3), network=_bar, render=_fake_render
    )
    assert tools.caption_coloured is None


def _caption_detector(
    images: list[NDArray[np.uint8]],
) -> list[list[tuple[float, float, float, float]]]:
    """A caption box over the line's rows in the 640-wide detector frame."""
    return [[(120.0, 180.0, 520.0, 240.0)] for _ in images]


def test_a_yolo_caption_detector_supplies_the_search_regions(tmp_path: Path) -> None:
    config = _configuration(tmp_path, residual=0.3)

    def unused_network(batch: NDArray[np.float32]) -> NDArray[np.float32]:
        raise AssertionError("the PP-OCR network must not plan regions when YOLO is given")

    tools, _ = job.real_collaborators(
        config, network=unused_network, render=_fake_render, caption_detector=_caption_detector
    )
    receipt = runner.run(tmp_path / "chunk", tools, stride=13, pins={})
    assert receipt["accepted"] == "attempt-sampled"


def test_main_pins_the_yolo_region_detector(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config.update(output=str(tmp_path / "out"), stride=13, detectorProgram="d.pt2",
                  detectorProgramSHA256="0" * 64)  # fmt: skip
    config["regionDetector"] = {
        "kind": "yolo", "weights": "w.pt", "sha256": "1" * 64, "configDirectory": "c",
        "work": "w", "imgsz": 640,
    }  # fmt: skip
    path = tmp_path / "config.json"
    path.write_text(json.dumps(config), encoding="utf-8")
    monkeypatch.setattr(mps, "lazy_network", lambda *_args, **_kw: _bar)
    monkeypatch.setattr(yolo_regions, "load_detector", lambda settings: _caption_detector)
    monkeypatch.setattr(
        job, "render_repaired_video_fast", lambda video_job, **_: video_job.output.write_bytes(b"v")
    )
    assert job.main([str(path)]) == 0
    receipt = json.loads((tmp_path / "out" / "chunk-receipt.json").read_text(encoding="utf-8"))
    assert receipt["pins"]["regionDetector"] == "yolo:" + "1" * 64 + ":640"


def test_yolo_masks_replace_the_learned_detector_in_the_real_render(tmp_path: Path) -> None:
    config = _configuration(tmp_path, residual=0.3)

    def unused_network(batch: NDArray[np.float32]) -> NDArray[np.float32]:
        raise AssertionError("the PP-OCR network must not run when YOLO masks are used")

    tools, _ = job.real_collaborators(
        config, network=unused_network, caption_detector=_caption_detector, caption_masks=True
    )
    receipt = runner.run(tmp_path / "chunk", tools, stride=13, pins={})
    assert receipt["accepted"] == "attempt-sampled"
    assert (tmp_path / "chunk" / "attempt-sampled" / "candidate.mp4").stat().st_size > 0


def test_yolo_masks_need_the_yolo_detector(tmp_path: Path) -> None:
    config = _configuration(tmp_path, residual=0.3)
    with pytest.raises(ValueError, match="need the YOLO caption detector"):
        job.real_collaborators(config, network=_bar, caption_masks=True)


def test_box_frames_follow_the_active_frames_and_their_span_origins() -> None:
    admitted: dict[str, object] = {
        "frames": [{"classification": "unknown"}, {"classification": "overlay-subtitle"}]
    }
    spans = [SubtitleSpan(1, 2, 7, 9, 16, 16)]
    boxes = [[(0.0, 0.0, 1.0, 1.0)], [(5.0, 6.0, 7.0, 8.0)]]
    assert job.box_frames(admitted, spans, boxes) == [
        yolo_regions.PlannedBoxes((7, 9), ((5.0, 6.0, 7.0, 8.0),))
    ]
    with pytest.raises(ValueError, match="do not match the admitted frames"):
        job.box_frames(admitted, spans, boxes[:1])


def test_a_yolo_admission_plans_without_ocr_and_records_its_kind(tmp_path: Path) -> None:
    config = _configuration(tmp_path, residual=0.3)
    tools, _ = job.real_collaborators(
        config,
        network=_bar,
        caption_detector=_caption_detector,
        caption_masks=True,
        yolo_admission=True,
    )
    receipt = runner.run(tmp_path / "chunk", tools, stride=13, pins={})
    attempts = receipt["attempts"]
    assert isinstance(attempts, list)
    attempt = attempts[0]
    assert receipt["accepted"] == "attempt-sampled"
    assert attempt["sampling"]["kind"] == "yolo-caption-boxes"
    assert list(attempt["stageSeconds"]) == ["yoloBoxes"]
    assert not (tmp_path / "chunk" / "attempt-sampled" / "ocr").exists()


def test_the_band_audit_reads_tiled_bands_and_is_pinned(tmp_path: Path) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config["auditTile"] = 2
    tools, pins = job.real_collaborators(
        config, network=_bar, caption_detector=_caption_detector, caption_masks=True,
        yolo_admission=True,
    )  # fmt: skip
    assert pins["auditBand"] == "tile2"
    # The fake OCR reads confident text in any image that is not the repaired candidate itself,
    # so the band audit flags residual text and both attempts fail.
    with pytest.raises(ValueError, match="Audit failed after escalation"):
        runner.run(tmp_path / "chunk", tools, stride=13, pins=pins)
    band = json.loads(
        (tmp_path / "chunk" / "attempt-sampled" / "audit" / "band.json").read_text(encoding="utf-8")
    )
    assert band["tile"] == 2 and band["indices"][0] == 0


def test_the_colour_gate_counts_during_the_band_ocr_with_identical_counts(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config["auditTile"] = 2
    config["colourGate"] = True
    calls: list[tuple[str, bool]] = []
    counts = colour_audit.colour_counts

    def recorded(
        *,
        ffmpeg: str,
        source: Path,
        source_start: float,
        cleaned: Path,
        indices: Sequence[int],
        boxes: Sequence[Sequence[tuple[int, int, int, int]]],
        size: tuple[int, int],
        band: tuple[int, int] | None = None,
        cleaned_bands: Sequence[NDArray[np.uint8]] | None = None,
        source_bands: Sequence[NDArray[np.uint8]] | None = None,
        outlined: bool = False,
    ) -> list[int]:
        calls.append((threading.current_thread().name, cleaned_bands is not None))
        return counts(
            ffmpeg=ffmpeg, source=source, source_start=source_start, cleaned=cleaned,
            indices=indices, boxes=boxes, size=size, band=band, cleaned_bands=cleaned_bands,
            source_bands=source_bands, outlined=outlined,
        )  # fmt: skip

    monkeypatch.setattr(colour_audit, "colour_counts", recorded)
    tools, _ = job.real_collaborators(
        config, network=_bar, caption_detector=_caption_detector, caption_masks=True,
        yolo_admission=True,
    )  # fmt: skip
    assert tools.colour is not None
    directory = tmp_path / "attempt"
    directory.mkdir()
    plan = tools.plan(13, directory)
    video = tools.render(plan, directory)
    indices = [0, 5, 10]
    tools.audit(video, indices)
    during = tools.colour(plan, video, indices)
    afterwards = tools.colour(plan, video, indices)  # no audit before it: decoded on its own
    assert during == afterwards
    assert [(name == "MainThread", banded) for name, banded in calls] == [
        (False, True),
        (True, False),
    ]


def test_flagged_frames_are_counted_again_without_the_interface_boxes(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config["auditTile"] = 2
    config["colourGate"] = True
    asked: list[tuple[list[int], bool]] = []

    def counted(
        *,
        ffmpeg: str,
        source: Path,
        source_start: float,
        cleaned: Path,
        indices: Sequence[int],
        boxes: Sequence[Sequence[tuple[int, int, int, int]]],
        size: tuple[int, int],
        band: tuple[int, int] | None = None,
        cleaned_bands: Sequence[NDArray[np.uint8]] | None = None,
        source_bands: Sequence[NDArray[np.uint8]] | None = None,
        excluded: Sequence[Sequence[tuple[int, int, int, int]]] | None = None,
        outlined: bool = False,
    ) -> list[int]:
        asked.append((list(indices), excluded is not None))
        # Scene caption colour beside the interface text: flagged until its boxes are left out.
        return [0 if excluded is not None else (10 if index == 10 else 60) for index in indices]

    monkeypatch.setattr(colour_audit, "colour_counts", counted)
    monkeypatch.setattr(job, "audited_interface", lambda *_args: True)  # every line: interface
    tools, _ = job.real_collaborators(
        config, network=_bar, caption_detector=_caption_detector, caption_masks=True,
        yolo_admission=True,
    )  # fmt: skip
    assert tools.colour is not None
    directory = tmp_path / "attempt"
    directory.mkdir()
    plan = tools.plan(13, directory)
    video = tools.render(plan, directory)
    observed = tools.audit(video, [0, 5, 10])
    assert [len(observed[index].detections) for index in (0, 5, 10)] == [1, 0, 1]
    assert tools.colour(plan, video, [0, 5, 10]) == [0, 60, 10]
    # Only the flagged frame with interface text is counted again; frame 5 has none to leave out.
    assert asked == [([0, 5, 10], False), ([0], True)]


def test_frames_still_flagged_are_counted_again_without_what_the_source_shows_as_scene(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config["auditTile"] = 2
    config["colourGate"] = True
    asked: list[tuple[list[int], list[int], list[int], bool, bool]] = []

    def counted(
        *,
        indices: Sequence[int],
        excluded: Sequence[Sequence[tuple[int, int, int, int]]] | None = None,
        **_kwargs: object,
    ) -> list[int]:
        # Frames 0 and 10 hold interface text and fall under the gate once it is left out.
        return [0 if excluded is not None else 60 for _ in indices]

    def scened(
        *,
        indices: Sequence[int],
        counts: Sequence[int],
        evidence: colour_scene.Evidence,
        excluded: Sequence[Sequence[tuple[int, int, int, int]]] | None = None,
        **_kwargs: object,
    ) -> list[int]:
        asked.append((list(indices), list(counts), sorted(evidence.lines), evidence.interface(0, 0),
                      evidence.interface(0, 0)))  # fmt: skip
        assert excluded is not None and len(evidence.presence) == len(excluded)
        return [value if value < 40 else 1 for value in counts]

    monkeypatch.setattr(colour_audit, "colour_counts", counted)
    monkeypatch.setattr(colour_scene, "scene_counts", scened)
    monkeypatch.setattr(job, "audited_interface", lambda *_args: True)  # every line: interface
    tools, _ = job.real_collaborators(
        config, network=_bar, caption_detector=_caption_detector, caption_masks=True,
        yolo_admission=True,
    )  # fmt: skip
    assert tools.colour is not None
    directory = tmp_path / "attempt"
    directory.mkdir()
    plan = tools.plan(13, directory)
    video = tools.render(plan, directory)
    tools.audit(video, [0, 5, 10])
    assert tools.colour(plan, video, [0, 5, 10]) == [0, 1, 0]
    assert asked == [([0, 5, 10], [0, 60, 0], [0, 5, 10], True, True)]


def test_a_below_floor_label_read_is_no_caption_read_but_a_confident_read_is(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config["auditTile"] = 2
    config["colourGate"] = True
    label = TextDetection("ゆーかみーな", 0.3, TextRectangle(0.0, 90.0, 120.0, 20.0))
    caption = TextDetection("こんにちは", 0.95, TextRectangle(60.0, 90.0, 120.0, 20.0))
    reads = {0: (label, caption), 5: (), 10: ()}
    judged: list[tuple[bool, bool]] = []

    def audited(_video: Path, indices: Sequence[int], **_kwargs: object) -> dict[int, object]:
        return {index: FrameObservation(0.0, 0.0, WIDTH, HEIGHT, reads[index]) for index in indices}

    def scened(*, counts: Sequence[int], evidence: colour_scene.Evidence, **_: object) -> list[int]:
        judged.append((evidence.interface(0, 0), evidence.interface(0, 1)))
        return list(counts)

    monkeypatch.setattr(band_audit, "band_audit", audited)
    monkeypatch.setattr(colour_audit, "colour_counts", lambda **_kwargs: [60, 0, 0])
    monkeypatch.setattr(colour_scene, "scene_counts", scened)
    monkeypatch.setattr(job, "audited_interface", lambda *_args: False)
    tools, _ = job.real_collaborators(
        config, network=_bar, caption_detector=_caption_detector, caption_masks=True,
        yolo_admission=True,
    )  # fmt: skip
    assert tools.colour is not None
    directory = tmp_path / "attempt"
    directory.mkdir()
    plan = tools.plan(13, directory)
    video = tools.render(plan, directory)
    tools.audit(video, [0, 5, 10])
    assert tools.colour(plan, video, [0, 5, 10]) == [60, 0, 0]
    assert judged == [(True, False)]


def test_frames_under_the_gate_skip_the_scene_recount(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config["auditTile"] = 2
    config["colourGate"] = True
    monkeypatch.setattr(colour_audit, "colour_counts", lambda **_kwargs: [3, 3, 3])
    monkeypatch.setattr(colour_scene, "scene_counts", lambda **_kwargs: pytest.fail("decoded"))
    tools, _ = job.real_collaborators(
        config, network=_bar, caption_detector=_caption_detector, caption_masks=True,
        yolo_admission=True,
    )  # fmt: skip
    assert tools.colour is not None
    directory = tmp_path / "attempt"
    directory.mkdir()
    plan = tools.plan(13, directory)
    video = tools.render(plan, directory)
    tools.audit(video, [0, 5, 10])
    assert tools.colour(plan, video, [0, 5, 10]) == [3, 3, 3]


def test_an_invalid_audit_tile_is_refused(tmp_path: Path) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config["auditTile"] = 9
    with pytest.raises(ValueError, match="auditTile"):
        job.real_collaborators(config, network=_bar)


CLOCK2 = VideoFrameClock((0, 3000), Fraction(1, 90000), 3000)


def test_residual_boxes_join_caption_frames_and_promote_unknown_ones() -> None:
    region = {"x": 0, "y": 0, "width": 10, "height": 10}
    admitted: dict[str, object] = {
        "frames": [
            {"classification": "unknown", "region": {"x": 0, "y": 0, "width": 99, "height": 99}},
            {"classification": "overlay-subtitle", "lineBoxes": [[1, 2, 3, 4]], "region": region},
        ],
        "masks": [{"startSeconds": 1 / 30, "endSeconds": 2 / 30, "region": dict(region)}],
    }
    merged = job.with_residual(
        admitted,
        [[], [(0.0, 0.0, 9.0, 9.0)]],
        {1: [(5, 6, 7, 8)], 0: [(20, 30, 40, 36)], 4: [(1, 1, 2, 2)], -1: [(1, 1, 2, 2)]},
        CLOCK2,
    )
    assert merged == [[(20.0, 30.0, 40.0, 36.0)], [(0.0, 0.0, 9.0, 9.0), (5.0, 6.0, 7.0, 8.0)]]
    assert admitted["frames"] == [
        {
            "classification": "overlay-subtitle",
            "region": {"x": 20, "y": 24, "width": 20, "height": 18},
            "lineBoxes": [[20, 30, 40, 36]],
            "sampled": False,
        },
        {
            "classification": "overlay-subtitle",
            "lineBoxes": [(1, 2, 3, 4), [5, 6, 7, 8]],
            "region": {"x": 0, "y": 0, "width": 10, "height": 10},
        },
    ]
    assert admitted["masks"] == [
        {"startSeconds": 0.0, "endSeconds": 1 / 30,
         "region": {"x": 20, "y": 24, "width": 20, "height": 18}},
        {"startSeconds": 1 / 30, "endSeconds": 2 / 30,
         "region": {"x": 0, "y": 0, "width": 10, "height": 10}},
    ]  # fmt: skip
    above: dict[str, object] = {
        "frames": [{"classification": "overlay-subtitle", "lineBoxes": [], "region": dict(region)}],
        "masks": [{"region": dict(region)}],
    }
    job.with_residual(above, [[]], {0: [(-5, -8, 4, 2), (6, 20, 30, 24)]}, CLOCK2)
    above_frames, above_masks = above["frames"], above["masks"]
    assert isinstance(above_frames, list)
    assert isinstance(above_masks, list)
    assert above_frames[0]["region"] == {"x": -5, "y": -18, "width": 35, "height": 46}
    assert above_masks[0]["region"] == {"x": -5, "y": -18, "width": 35, "height": 46}
    with pytest.raises(ValueError, match="per-frame entries"):
        job.with_residual({"frames": 3}, [], {0: [(1, 1, 2, 2)]}, CLOCK2)


def test_stored_yolo_boxes_supply_the_regions_without_decoding(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config = _configuration(tmp_path, residual=0.3)
    pts = record(config["clock"])["pts"]
    assert isinstance(pts, list)
    scale = int(str(config["width"])) / 640

    def no_decode(*_args: object, **_kwargs: object) -> object:
        raise AssertionError("stored boxes must not decode the chunk again")

    monkeypatch.setattr(yolo_regions, "chunk_frames", no_decode)
    box = (120.0 * scale, 180.0 * scale, 520.0 * scale, 240.0 * scale)
    tools, _ = job.real_collaborators(
        config, network=_bar, render=_fake_render, stored_boxes=[[box]] * len(pts)
    )
    receipt = runner.run(tmp_path / "chunk", tools, stride=13, pins={})
    assert receipt["accepted"] == "attempt-sampled"


def test_main_uses_stored_boxes_and_skips_the_detector_model(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config.update(output=str(tmp_path / "out"), stride=13, detectorProgram="d.pt2",
                  detectorProgramSHA256="0" * 64, yoloBoxes=str(tmp_path / "boxes"),
                  sourceSHA256="5" * 64)  # fmt: skip
    config["regionDetector"] = {
        "kind": "yolo", "weights": "w.pt", "sha256": "1" * 64, "configDirectory": "c",
        "work": "w", "imgsz": 640,
    }  # fmt: skip
    path = tmp_path / "config.json"
    path.write_text(json.dumps(config), encoding="utf-8")
    asked: list[tuple[Path, str, str, int]] = []

    def load(directory: Path, pts: object, **keywords: object) -> list[list[object]]:
        asked.append((directory, str(keywords["source_sha256"]), str(keywords["weights_sha256"]),
                      int(str(keywords["imgsz"]))))  # fmt: skip
        assert isinstance(pts, tuple)
        return [[] for _ in pts]

    def no_model(_settings: object) -> object:
        raise AssertionError("the detector model must not load when boxes are stored")

    monkeypatch.setattr(mps, "lazy_network", lambda *_args, **_kw: _bar)
    monkeypatch.setattr(yolo_boxes, "load", load)
    monkeypatch.setattr(yolo_regions, "load_detector", no_model)
    monkeypatch.setattr(
        job, "render_repaired_video_fast", lambda video_job, **_: video_job.output.write_bytes(b"v")
    )
    assert job.main([str(path)]) == 0
    assert asked == [(tmp_path / "boxes", "5" * 64, "1" * 64, 640)]
    assert capsys.readouterr().out.splitlines()[0] == "YOLO_BOXES stored"


def test_the_caption_style_is_measured_once_from_the_profile_and_cached(tmp_path: Path) -> None:
    config = _configuration(tmp_path, residual=0.3)
    directory = Path(str(config["profileDirectory"]))
    frames = job._profile_frames(directory)
    profile = job._profile(directory)
    cache = tmp_path / "caption-style.json"
    style = job.caption_style(
        frames, profile, source=Path(str(config["source"])), ffmpeg="ffmpeg", cache=cache
    )
    # The black source holds no caption pink: not the outlined style, every residual counts.
    assert style == {"outlined": False, "lines": 12, "medianOutlinedShare": 0.0}
    assert json.loads(cache.read_text(encoding="utf-8")) == style
    cache.write_text('{"outlined": true}', encoding="utf-8")  # read back, not measured again
    assert job.caption_style(
        frames, profile, source=Path(str(config["source"])), ffmpeg="ffmpeg", cache=cache
    ) == {"outlined": True}


def test_a_below_floor_line_is_a_caption_when_clearly_outlined_or_framed_or_long() -> None:
    assert job._caption_styled("星降りメリー", 0.031) is True  # clearly outlined
    assert job._caption_styled("なつとん", 0.019) is False  # a short unframed gift label
    assert job._caption_styled("<いけると>>", 0.018) is True  # read with the caption frame
    assert job._caption_styled("く3よいしょよいしょ", 0.018) is True
    assert job._caption_styled("おいろつしゃいかけそありがろう", 0.023) is True  # long
    assert job._caption_styled("<いけると>>", 0.01) is False  # framed, but not outlined


def test_the_styled_caption_collaborator_measures_only_under_the_outlined_style(
    tmp_path: Path,
) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config["colourGate"] = True
    tools, _ = job.real_collaborators(config, network=_bar, render=_fake_render)
    assert tools.styled_caption is not None
    lines = [
        Residual(0, "こんにちは世界です", 0.3, (10.0, 10.0, 60.0, 20.0)),
        Residual(0, "abc", 0.3, (10.0, 40.0, 60.0, 20.0)),
    ]
    assert tools.styled_caption(lines) == [False, False]  # the black source is not outlined
    cache = Path(str(config["output"])).parent / "caption-style.json"
    assert json.loads(cache.read_text(encoding="utf-8"))["outlined"] is False
    cache.write_text('{"outlined": true}', encoding="utf-8")
    fresh, _ = job.real_collaborators(config, network=_bar, render=_fake_render)
    assert fresh.styled_caption is not None
    assert fresh.styled_caption(lines) == [False, False]  # measured: no caption pink there
    assert fresh.styled_caption(lines[1:]) == [False]  # no Japanese line: nothing decoded


def test_a_residual_pass_restores_the_unknown_list_from_its_frames() -> None:
    # A residual pass rewrites frames and masks; the stored unknown list must describe the plan
    # the audit and the eligibility check read, or the accepted chunk fails its own check.
    clock = VideoFrameClock((0, 3000, 6000), Fraction(1, 30000), 3000)
    admitted: dict[str, object] = {
        "masks": [],
        "frames": [
            {"sourcePTS": 0, "classification": "overlay-subtitle"},
            {"sourcePTS": 3000, "classification": "unknown"},
        ],
        "unknownSourcePTS": [999],
    }
    job.with_residual(admitted, [[], []], {}, clock)
    assert admitted["unknownSourcePTS"] == [3000]


def test_a_residual_fragment_grows_to_the_planned_caption_line_on_its_row() -> None:
    entries: list[object] = [
        {"lineBoxes": [[468, 1292, 716, 1321], [487, 1259, 649, 1297]]},
        {"classification": "unknown"},
        {"lineBoxes": [[470, 1293, 730, 1320]]},
        {"lineBoxes": [[100, 1292, 900, 1321]]},  # beyond the reach
    ]
    region: list[object] = [{"lineBoxes": [[0, 1139, 1077, 1431]]}]  # a region keyed as one box
    assert job.line_extent((465, 1296, 513, 1319), region, 0, 1) == (465, 1296, 513, 1319)
    assert job.line_extent((465, 1296, 513, 1319), entries, 1, 1) == (465, 1292, 730, 1321)


def test_a_residual_fragment_without_a_planned_line_on_its_row_stays_as_read() -> None:
    entries: list[object] = [{"lineBoxes": [[487, 1259, 649, 1297], [900, 1296, 1000, 1319]]}]
    assert job.line_extent((465, 1296, 513, 1319), entries, 0, 5) == (465, 1296, 513, 1319)


def test_a_promoted_frame_keeps_whether_the_plan_read_it() -> None:
    clock = VideoFrameClock((0, 3000, 6000), Fraction(1, 30000), 3000)
    admitted: dict[str, object] = {
        "masks": [],
        "frames": [
            {"sourcePTS": 0, "classification": "unknown"},
            {"sourcePTS": 3000, "classification": "unknown"},
            {"sourcePTS": 6000, "classification": "unknown"},
        ],
        "sampling": {"sampledFrames": [0, 2]},
    }
    job.with_residual(admitted, [[], [], []], {0: [(1, 2, 30, 20)], 1: [(1, 2, 30, 20)]}, clock)
    assert admitted["frames"] == [
        {"sourcePTS": 0, "classification": "overlay-subtitle", "lineBoxes": [[1, 2, 30, 20]],
         "region": {"x": 1, "y": -16, "width": 29, "height": 54}, "sampled": True},
        {"sourcePTS": 3000, "classification": "overlay-subtitle", "lineBoxes": [[1, 2, 30, 20]],
         "region": {"x": 1, "y": -16, "width": 29, "height": 54}, "sampled": False},
        {"sourcePTS": 6000, "classification": "unknown"},
    ]  # fmt: skip


def test_an_unread_english_line_counts_when_pink_filled_and_left_in_the_candidate(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config = _configuration(tmp_path, residual=0.3)
    config["colourGate"] = True
    tools, _ = job.real_collaborators(config, network=_bar, render=_fake_render)
    assert tools.styled_caption is not None
    lines = [
        Residual(0, "Thank you very much", 0.3, (10.0, 10.0, 200.0, 20.0)),
        Residual(0, "Wow", 0.3, (10.0, 40.0, 60.0, 20.0)),  # too few letters
        Residual(0, "POPULAR", 0.3, (10.0, 70.0, 100.0, 90.0)),  # not a horizontal line
    ]
    assert tools.styled_caption(lines) == [False, False, False]  # nothing rendered yet
    directory = tmp_path / "attempt"
    directory.mkdir()
    tools.render(tools.plan(13, directory), directory)
    assert tools.styled_caption(lines) == [False, False, False]  # the black source has no pink
    monkeypatch.setattr(colour_audit, "source_pink_counts", lambda **_kwargs: [400])
    monkeypatch.setattr(colour_audit, "colour_counts", lambda **_kwargs: [39])
    assert tools.styled_caption(lines) == [False, False, False]  # too little left
    monkeypatch.setattr(colour_audit, "colour_counts", lambda **_kwargs: [40])
    assert tools.styled_caption(lines) == [True, False, False]


def test_a_residual_heals_over_every_frame_its_caption_persists() -> None:
    present = [False, False, True, True, True, False, False]  # frames 0, 5, ... 30
    assert job.persistence(12, 35, 5, present) == (5, 25)
    assert job.persistence(0, 35, 5, [False] * 7) == (0, 5)
    assert job.persistence(34, 35, 5, [False] * 6 + [True]) == (25, 34)
