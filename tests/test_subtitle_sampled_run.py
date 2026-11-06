"""The chunk runner audits each attempt, escalates once, and never accepts a failed audit."""

import dataclasses
import json
from collections.abc import Mapping, Sequence
from pathlib import Path

import pytest

from replay import subtitle_sampled_run as runner
from replay.caption_io import FrameObservation
from replay.caption_masks import TextDetection, TextRectangle
from replay.subtitle_repair_audit import Residual

BOX = {"x": 0, "y": 100, "width": 1000, "height": 200}
PLAN: dict[str, object] = {
    "frames": [{"region": BOX}, {"classification": "unknown"}, {"region": BOX}],
    "sampling": {"rawOcr": False},
    "stageSeconds": {"ocr": 1.0},
}
CAPTION = TextDetection("決着", 1.0, TextRectangle(100, 150, 60, 30))
SMEAR = TextDetection("Ter.ne", 0.3, TextRectangle(100, 150, 60, 30))


def _tools(clean_stride: int, strides: list[int]) -> runner.Collaborators:
    def plan(stride: int, directory: Path) -> dict[str, object]:
        strides.append(stride)
        return PLAN

    def render(_plan: dict[str, object], directory: Path) -> Path:
        video = directory / "candidate.mp4"
        video.write_bytes(b"video")
        return video

    def audit(video: Path, indices: Sequence[int]) -> Mapping[int, FrameObservation]:
        stride = strides[-1]
        line = SMEAR if stride <= clean_stride else CAPTION
        return {index: FrameObservation(0.0, 0.0, 1080, 1920, (line,)) for index in indices}

    return runner.Collaborators(plan, render, audit)


def test_a_passing_sampled_attempt_is_accepted_without_escalation(tmp_path: Path) -> None:
    strides: list[int] = []
    receipt = runner.run(tmp_path / "chunk", _tools(6, strides), pins={"native": "abc"})
    assert strides == [6]
    assert receipt["accepted"] == "attempt-sampled"
    assert receipt["escalated"] is False
    assert receipt["pins"] == {"native": "abc"}
    assert receipt["qualityApproved"] is False
    stored = json.loads((tmp_path / "chunk" / "chunk-receipt.json").read_text(encoding="utf-8"))
    assert stored == receipt


def test_a_failed_audit_escalates_once_to_full_ocr(tmp_path: Path) -> None:
    strides: list[int] = []
    receipt = runner.run(tmp_path / "chunk", _tools(1, strides))
    assert strides == [6, 1]
    assert receipt["accepted"] == "attempt-full"
    assert receipt["escalated"] is True
    attempts = receipt["attempts"]
    assert isinstance(attempts, list)
    assert attempts[0]["passed"] is False
    assert attempts[0]["residualLines"] == [
        {"frame": 0, "text": "決着", "confidence": 1.0, "box": [100.0, 150.0, 60.0, 30.0]},
        {"frame": 2, "text": "決着", "confidence": 1.0, "box": [100.0, 150.0, 60.0, 30.0]},
    ]
    assert attempts[1]["passed"] is True


def test_nothing_is_accepted_when_the_full_attempt_also_fails(tmp_path: Path) -> None:
    strides: list[int] = []
    with pytest.raises(ValueError, match="nothing accepted"):
        runner.run(tmp_path / "chunk", _tools(0, strides))
    stored = json.loads((tmp_path / "chunk" / "chunk-receipt.json").read_text(encoding="utf-8"))
    assert stored["accepted"] is None
    assert stored["escalated"] is True


def test_invalid_strides_are_rejected(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="stride"):
        runner.run(tmp_path / "a", _tools(6, []), stride=1)
    with pytest.raises(ValueError, match="stride"):
        runner.run(tmp_path / "b", _tools(6, []), audit_stride=0)


def test_a_plan_without_frame_entries_is_rejected(tmp_path: Path) -> None:
    broken = runner.Collaborators(
        plan=lambda _stride, _directory: {"frames": "none"},
        render=lambda _plan, directory: directory / "x.mp4",
        audit=lambda _video, _indices: {},
    )
    with pytest.raises(ValueError, match="per-frame"):
        runner.run(tmp_path / "chunk", broken)


def _with_colour(tools: runner.Collaborators, counts: dict[int, list[int]]) -> runner.Collaborators:
    def colour(_plan: dict[str, object], video: Path, indices: Sequence[int]) -> Sequence[int]:
        return counts[1 if video.parent.name == "attempt-full" else 0][: len(indices)]

    return runner.Collaborators(tools.plan, tools.render, tools.audit, colour)


def test_colour_residual_below_the_gate_is_recorded_and_accepted(tmp_path: Path) -> None:
    tools = _with_colour(_tools(6, []), {0: [0, 39], 1: [0, 0]})
    receipt = runner.run(tmp_path / "chunk", tools)
    attempts = receipt["attempts"]
    assert isinstance(attempts, list)
    assert attempts[0]["colourResidual"] == {"gate": 40, "flaggedFrames": [], "maximum": 39}
    assert receipt["accepted"] == "attempt-sampled"


def test_colour_residual_at_the_gate_escalates(tmp_path: Path) -> None:
    tools = _with_colour(_tools(6, []), {0: [0, 40], 1: [0, 5]})
    receipt = runner.run(tmp_path / "chunk", tools)
    attempts = receipt["attempts"]
    assert isinstance(attempts, list)
    assert attempts[0]["colourResidual"] == {"gate": 40, "flaggedFrames": [2], "maximum": 40}
    assert (receipt["accepted"], receipt["escalated"]) == ("attempt-full", True)


def _coloured_tools(flags: list[bool], strides: list[int]) -> runner.Collaborators:
    base = _tools(0, strides)

    def caption_coloured(residuals: Sequence[object]) -> list[bool]:
        return [flags[0] for _ in residuals]

    return runner.Collaborators(
        base.plan, base.render, base.audit, caption_coloured=caption_coloured
    )


def test_a_residual_without_caption_colour_is_recorded_as_interface_text(tmp_path: Path) -> None:
    receipt = runner.run(tmp_path / "chunk", _coloured_tools([False], []))
    attempts = receipt["attempts"]
    assert isinstance(attempts, list)
    assert attempts[0]["residualLines"] == []
    assert attempts[0]["interfaceText"] == [
        {"frame": 0, "text": "決着", "confidence": 1.0, "box": [100.0, 150.0, 60.0, 30.0]},
        {"frame": 2, "text": "決着", "confidence": 1.0, "box": [100.0, 150.0, 60.0, 30.0]},
    ]
    assert receipt["accepted"] == "attempt-sampled"


def test_a_caption_coloured_residual_still_fails_the_audit(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="Audit failed"):
        runner.run(tmp_path / "chunk", _coloured_tools([True], []))


def test_a_failed_sampled_attempt_is_healed_with_its_own_plan_before_full_ocr(
    tmp_path: Path,
) -> None:
    strides: list[int] = []
    taken: list[tuple[int, int]] = []
    base = _tools(0, strides)

    def audit(video: Path, indices: Sequence[int]) -> Mapping[int, FrameObservation]:
        line = SMEAR if taken else CAPTION
        return {index: FrameObservation(0.0, 0.0, 1080, 1920, (line,)) for index in indices}

    def residual(lines: Sequence[Residual], audit_stride: int) -> None:
        taken.extend((line.frame, audit_stride) for line in lines)

    tools = runner.Collaborators(base.plan, base.render, audit, residual=residual)
    receipt = runner.run(tmp_path / "chunk", tools)
    assert strides == [6]  # no full OCR plan: the sampled plan is reused
    attempts = receipt["attempts"]
    assert isinstance(attempts, list)
    assert [(a["directory"], a["replanned"], a["passed"]) for a in attempts] == [
        ("attempt-sampled", True, False),
        ("attempt-sampled-residual", False, True),
    ]
    assert receipt["accepted"] == "attempt-sampled-residual"
    assert taken == [(0, 5), (2, 5)]


def test_attempts_run_sampled_healed_full_then_healed_full(tmp_path: Path) -> None:
    strides: list[int] = []
    base = _tools(0, strides)

    def audit(video: Path, indices: Sequence[int]) -> Mapping[int, FrameObservation]:
        line = SMEAR if video.parent.name == "attempt-residual" else CAPTION
        return {index: FrameObservation(0.0, 0.0, 1080, 1920, (line,)) for index in indices}

    tools = runner.Collaborators(base.plan, base.render, audit, residual=lambda *_a: None)
    receipt = runner.run(tmp_path / "chunk", tools)
    assert strides == [6, 1]
    attempts = receipt["attempts"]
    assert isinstance(attempts, list)
    assert [(a["directory"], a["stride"], a["replanned"]) for a in attempts] == [
        ("attempt-sampled", 6, True),
        ("attempt-sampled-residual", 6, False),
        ("attempt-full", 1, True),
        ("attempt-residual", 1, False),
    ]
    assert receipt["accepted"] == "attempt-residual"


def test_no_residual_attempt_without_boxes_or_collaborator(tmp_path: Path) -> None:
    strides: list[int] = []
    with pytest.raises(ValueError, match="nothing accepted"):
        runner.run(tmp_path / "chunk", _tools(0, strides))
    assert strides == [6, 1]


def test_a_caption_line_stacked_above_the_region_still_fails_the_audit(tmp_path: Path) -> None:
    strides: list[int] = []
    above = TextDetection("ならちょ待って", 1.0, TextRectangle(100, 20, 60, 30))
    far = TextDetection("遠い", 1.0, TextRectangle(100, 900, 60, 30))
    base = _tools(0, strides)

    def audit(video: Path, indices: Sequence[int]) -> Mapping[int, FrameObservation]:
        line = far if strides[-1] == 1 else above
        return {index: FrameObservation(0.0, 0.0, 1080, 1920, (line,)) for index in indices}

    receipt = runner.run(tmp_path / "chunk", runner.Collaborators(base.plan, base.render, audit))
    assert receipt["accepted"] == "attempt-full"
    attempts = receipt["attempts"]
    assert isinstance(attempts, list)
    assert attempts[0]["residualLines"][0]["text"] == "ならちょ待って"


def _colour_failing_tools(
    strides: list[int], found: list[Residual], seen: list[tuple[str, list[int]]], taken: list[int]
) -> runner.Collaborators:
    """OCR finds nothing, but the colour gate flags frame 2 until boxes are taken."""
    base = _tools(6, strides)

    def colour(_plan: dict[str, object], video: Path, indices: Sequence[int]) -> Sequence[int]:
        return [0, 0 if taken else 838][: len(indices)]

    def regions(plan: dict[str, object], video: Path, frames: Sequence[int]) -> Sequence[Residual]:
        assert plan == PLAN
        seen.append((video.parent.name, list(frames)))
        return found

    def residual(lines: Sequence[Residual], audit_stride: int) -> None:
        taken.extend(line.frame for line in lines)

    return runner.Collaborators(
        base.plan, base.render, base.audit, colour, residual=residual, colour_regions=regions
    )


def test_a_colour_only_failure_repairs_just_its_flagged_frames(tmp_path: Path) -> None:
    strides: list[int] = []
    seen: list[tuple[str, list[int]]] = []
    taken: list[int] = []
    found = [Residual(2, "caption colour", 0.0, (100.0, 150.0, 12.0, 6.0))]
    tools = _colour_failing_tools(strides, found, seen, taken)
    receipt = runner.run(tmp_path / "chunk", tools)
    assert strides == [6]
    assert seen == [("attempt-sampled", [2])]
    assert taken == [2]
    assert receipt["accepted"] == "attempt-sampled-residual"
    attempts = receipt["attempts"]
    assert isinstance(attempts, list)
    assert attempts[0]["colourResidual"] == {"gate": 40, "flaggedFrames": [2], "maximum": 838}
    assert attempts[0]["colourRegions"] == [
        {"frame": 2, "text": "caption colour", "confidence": 0.0, "box": [100.0, 150.0, 12.0, 6.0]}
    ]
    assert attempts[1]["colourResidual"] == {"gate": 40, "flaggedFrames": [], "maximum": 0}
    # Each audit check's wall time is recorded apart: the OCR, the colour gate, the regions.
    assert set(attempts[0]["auditSeconds"]) == {"ocr", "colourGate", "colourRegions"}
    assert set(attempts[1]["auditSeconds"]) == {"ocr", "colourGate"}


def test_a_colour_only_failure_without_components_escalates_and_still_fails(
    tmp_path: Path,
) -> None:
    strides: list[int] = []
    seen: list[tuple[str, list[int]]] = []
    with pytest.raises(ValueError, match="nothing accepted"):
        runner.run(tmp_path / "chunk", _colour_failing_tools(strides, [], seen, []))
    assert seen == [("attempt-sampled", [2]), ("attempt-full", [2])]
    assert strides == [6, 1]


def test_a_chunk_that_passes_never_asks_for_colour_regions(tmp_path: Path) -> None:
    strides: list[int] = []
    seen: list[tuple[str, list[int]]] = []
    base = _tools(6, strides)
    tools = _colour_failing_tools(strides, [], seen, [])
    receipt = runner.run(
        tmp_path / "chunk",
        runner.Collaborators(
            base.plan, base.render, base.audit, residual=tools.residual,
            colour_regions=tools.colour_regions,
        ),
    )  # fmt: skip
    assert receipt["accepted"] == "attempt-sampled"
    assert seen == []


def test_moving_interface_text_is_recorded_and_not_a_residual(tmp_path: Path) -> None:
    strides: list[int] = []
    base = _tools(0, strides)  # every audited frame reads the caption-like line
    asked: list[tuple[int, int]] = []

    def moving(observed: Mapping[int, FrameObservation], frame: int, index: int) -> bool:
        asked.append((frame, index))
        return index == 0 and frame in observed

    tools = runner.Collaborators(base.plan, base.render, base.audit, moving_interface=moving)
    receipt = runner.run(tmp_path / "chunk", tools)
    assert receipt["accepted"] == "attempt-sampled"
    attempts = receipt["attempts"]
    assert isinstance(attempts, list)
    assert attempts[0]["residualLines"] == []
    assert attempts[0]["interfaceText"] == [
        {"frame": 0, "text": "決着", "confidence": 1.0, "box": [100.0, 150.0, 60.0, 30.0]},
        {"frame": 2, "text": "決着", "confidence": 1.0, "box": [100.0, 150.0, 60.0, 30.0]},
    ]
    assert asked == [(0, 0), (2, 0)]


def test_a_residual_the_rule_does_not_name_stays_a_residual(tmp_path: Path) -> None:
    strides: list[int] = []
    base = _tools(0, strides)
    tools = runner.Collaborators(
        base.plan, base.render, base.audit, moving_interface=lambda _observed, _frame, _index: False
    )
    with pytest.raises(ValueError, match="nothing accepted"):
        runner.run(tmp_path / "chunk", tools)


def test_measured_decisions_are_recorded_in_the_receipt(tmp_path: Path) -> None:
    base = _tools(6, [])
    tools = runner.Collaborators(
        base.plan, base.render, base.audit,
        notes=lambda: {"captionStyle": {"outlined": False, "shareBesideWhite": 0.0}},
    )  # fmt: skip
    receipt = runner.run(tmp_path / "chunk", tools)
    assert receipt["captionStyle"] == {"outlined": False, "shareBesideWhite": 0.0}


def test_a_below_floor_caption_styled_line_counts_and_a_gift_label_does_not(
    tmp_path: Path,
) -> None:
    strides: list[int] = []
    base = _tools(6, strides)  # the confident audit lines are smears: nothing above the floor
    caption = TextDetection("<< 俺今日何回連れてかれるの >>", 0.3, TextRectangle(100, 150, 400, 30))
    gift = TextDetection("星降りメリー", 0.3, TextRectangle(100, 200, 200, 30))

    def audit(video: Path, indices: Sequence[int]) -> Mapping[int, FrameObservation]:
        lines = (gift,) if video.parent.name != "attempt-sampled" else (caption, gift)
        return {index: FrameObservation(0.0, 0.0, 1080, 1920, lines) for index in indices}

    asked: list[list[str]] = []

    def styled(lines: Sequence[Residual]) -> list[bool]:
        asked.append([line.text for line in lines])
        return [line.text.startswith("<<") for line in lines]  # the source styles, measured

    tools = runner.Collaborators(base.plan, base.render, audit, styled_caption=styled)
    receipt = runner.run(tmp_path / "chunk", tools)
    attempts = receipt["attempts"]
    assert isinstance(attempts, list)
    assert [line["text"] for line in attempts[0]["residualLines"]] == [
        "<< 俺今日何回連れてかれるの >>",
        "<< 俺今日何回連れてかれるの >>",
    ]
    assert attempts[0]["passed"] is False
    assert receipt["accepted"] == "attempt-full"  # the gift label alone never fails the audit
    assert asked[0] == [caption.text, gift.text, caption.text, gift.text]
    assert "styledCaption" in attempts[0]["auditSeconds"]


def test_a_caption_the_plan_never_saw_counts_when_caption_styled(tmp_path: Path) -> None:
    # The caption sits above the learned band (frame 2 is the last audited frame, frame 1 the one
    # the plan left without a region): it touches no search region, yet a caption-styled source
    # makes it a residual.
    strides: list[int] = []
    base = _tools(6, strides)
    above = TextDetection("<< 俺今日何回連れてかれるの >>", 0.95, TextRectangle(100, 20, 400, 30))
    label = TextDetection("星降りメリー", 0.95, TextRectangle(100, 900, 200, 30))

    def audit(video: Path, indices: Sequence[int]) -> Mapping[int, FrameObservation]:
        lines = (above, label) if video.parent.name == "attempt-sampled" else (label,)
        return {index: FrameObservation(0.0, 0.0, 1080, 1920, lines) for index in indices}

    def styled(lines: Sequence[Residual]) -> list[bool]:
        return [line.text.startswith("<<") for line in lines]

    tools = runner.Collaborators(base.plan, base.render, audit, styled_caption=styled)
    receipt = runner.run(tmp_path / "chunk", tools)
    attempts = receipt["attempts"]
    assert isinstance(attempts, list)
    assert [(line["frame"], line["text"]) for line in attempts[0]["residualLines"]] == [
        (0, "<< 俺今日何回連れてかれるの >>"),
        (2, "<< 俺今日何回連れてかれるの >>"),
    ]
    assert receipt["accepted"] == "attempt-full"  # an unplanned interface label alone never fails


def _reading(
    calls: list[str],
    frames: list[object],
    *,
    reads: tuple[TextDetection, ...] = (CAPTION,),
    audited: tuple[TextDetection, ...] = (),
) -> runner.Collaborators:
    """Tools that log their order: the source shows ``reads``, the candidate ``audited``."""

    def plan(_stride: int, _directory: Path) -> dict[str, object]:
        calls.append("plan")
        return {"frames": frames}

    def read(directory: Path, indices: Sequence[int]) -> Mapping[int, FrameObservation]:
        calls.append(f"read {directory.name} {list(indices)}")
        return {index: FrameObservation(0.0, 0.0, 1080, 1920, reads) for index in indices}

    def residual(lines: Sequence[Residual], stride: int) -> None:
        calls.append(f"residual {[item.frame for item in lines]} {stride}")

    def render(_plan: dict[str, object], directory: Path) -> Path:
        calls.append(f"render {directory.name}")
        video = directory / "candidate.mp4"
        video.write_bytes(b"video")
        return video

    def audit(_video: Path, indices: Sequence[int]) -> Mapping[int, FrameObservation]:
        return {index: FrameObservation(0.0, 0.0, 1080, 1920, audited) for index in indices}

    return runner.Collaborators(plan, render, audit, residual=residual, source_read=read)


def test_a_plan_with_an_unknown_frame_reads_the_source_before_its_first_render(
    tmp_path: Path,
) -> None:
    calls: list[str] = []
    frames: list[object] = [{"region": BOX}, {"classification": "unknown", "region": BOX}]
    receipt = runner.run(tmp_path / "chunk", _reading(calls, frames), audit_stride=1)
    assert calls == [
        "plan",
        "read attempt-sampled [0, 1]",
        "residual [1] 1",
        "render attempt-sampled",
    ]
    assert receipt["accepted"] == "attempt-sampled"
    attempts = receipt["attempts"]
    assert isinstance(attempts, list)
    read = attempts[0]["planRead"]
    assert read["unknownFrames"] == 1
    assert read["lines"] == [
        {"frame": 1, "text": "決着", "confidence": 1.0, "box": [100.0, 150.0, 60.0, 30.0]}
    ]
    assert sorted(read["seconds"]) == ["ocr", "persistence"]


def test_a_read_that_finds_nothing_on_the_unknown_frame_adds_nothing(tmp_path: Path) -> None:
    calls: list[str] = []
    frames: list[object] = [{"region": BOX}, {"classification": "unknown", "region": BOX}]
    receipt = runner.run(tmp_path / "chunk", _reading(calls, frames, reads=()), audit_stride=1)
    assert calls == ["plan", "read attempt-sampled [0, 1]", "render attempt-sampled"]
    attempts = receipt["attempts"]
    assert isinstance(attempts, list)
    assert attempts[0]["planRead"]["lines"] == []
    assert sorted(attempts[0]["planRead"]["seconds"]) == ["ocr"]


def test_a_plan_without_an_unknown_frame_on_the_audit_grid_reads_nothing(tmp_path: Path) -> None:
    calls: list[str] = []
    known: list[object] = [{"region": BOX}, {"region": BOX}]
    receipt = runner.run(tmp_path / "a", _reading(calls, known), audit_stride=1)
    assert calls == ["plan", "render attempt-sampled"]
    attempts = receipt["attempts"]
    assert isinstance(attempts, list)
    assert "planRead" not in attempts[0]
    calls.clear()
    off_grid: list[object] = [
        {"region": BOX},
        {"classification": "unknown", "region": BOX},
        {"region": BOX},
    ]
    runner.run(tmp_path / "b", _reading(calls, off_grid), audit_stride=2)
    assert calls == ["plan", "render attempt-sampled"]


def test_only_the_first_plan_reads_ahead(tmp_path: Path) -> None:
    calls: list[str] = []
    frames: list[object] = [{"classification": "unknown", "region": BOX}]
    with pytest.raises(ValueError, match="nothing accepted"):
        runner.run(tmp_path / "chunk", _reading(calls, frames, audited=(CAPTION,)), audit_stride=1)
    assert calls == [
        "plan",
        "read attempt-sampled [0]",
        "residual [0] 1",
        "render attempt-sampled",
        "residual [0] 1",
        "render attempt-sampled-residual",
        "plan",
        "render attempt-full",
        "residual [0] 1",
        "render attempt-residual",
    ]


def test_reading_ahead_needs_the_residual_collaborator(tmp_path: Path) -> None:
    calls: list[str] = []
    frames: list[object] = [{"classification": "unknown", "region": BOX}]
    tools = dataclasses.replace(_reading(calls, frames), residual=None)
    runner.run(tmp_path / "chunk", tools, audit_stride=1)
    assert calls == ["plan", "render attempt-sampled"]


def test_the_read_is_judged_by_the_rules_of_the_audit(tmp_path: Path) -> None:
    calls: list[str] = []
    frames: list[object] = [{"classification": "unknown", "region": BOX}]
    tools = dataclasses.replace(
        _reading(calls, frames), caption_coloured=lambda lines: [False] * len(lines)
    )
    receipt = runner.run(tmp_path / "chunk", tools, audit_stride=1)
    assert calls == ["plan", "read attempt-sampled [0]", "render attempt-sampled"]
    attempts = receipt["attempts"]
    assert isinstance(attempts, list)
    assert attempts[0]["planRead"]["lines"] == []
    assert sorted(attempts[0]["planRead"]["seconds"]) == ["captionColoured", "ocr"]
