"""A colour-gate residual the source shows in a caption-free frame is scene; a remnant is not."""

from collections.abc import Iterator, Sequence
from pathlib import Path

import numpy as np
import pytest
from numpy.typing import NDArray

from replay import subtitle_colour_scene as scene

PINK = (180, 105, 255)
SOURCE, CLEANED = Path("source.mp4"), Path("cleaned.mp4")
Frames = dict[Path, dict[int, NDArray[np.uint8]]]


def _grey() -> NDArray[np.uint8]:
    return np.full((60, 80, 3), 200, dtype=np.uint8)


def _rim(frame: NDArray[np.uint8], left: int = 10) -> NDArray[np.uint8]:
    frame[20:28, left : left + 50] = PINK  # an avatar rim: 8 x 50 px of caption pink, unchanged
    return frame


def _decoder(frames: Frames, asked: list[tuple[str, list[int]]]) -> object:
    def decode(
        _ffmpeg: str,
        video: Path,
        _start: float,
        indices: Sequence[int],
        _size: tuple[int, int],
    ) -> Iterator[NDArray[np.uint8]]:
        asked.append((video.name, list(indices)))
        return iter([frames[video][index] for index in indices])

    return decode


def _count(
    monkeypatch: pytest.MonkeyPatch,
    frames: Frames,
    evidence: scene.Evidence,
    counts: Sequence[int] = (400,),
) -> tuple[list[int], list[tuple[str, list[int]]]]:
    asked: list[tuple[str, list[int]]] = []
    monkeypatch.setattr(scene, "_frames", _decoder(frames, asked))
    result = scene.scene_counts(
        ffmpeg="ffmpeg", source=SOURCE, source_start=0.0, cleaned=CLEANED, indices=[0],
        counts=counts, boxes=[[(0, 15, 80, 35)]] + [[]] * 10, evidence=evidence, size=(80, 60),
    )  # fmt: skip
    return result, asked


def _no_interface(_frame: int, _index: int) -> bool:
    return False


def _chunk(lines: dict[int, list[tuple[int, int, int, int]]]) -> scene.Evidence:
    return scene.Evidence([[(0, 15, 80, 35)]] + [[]] * 10, lines, _no_interface)


def test_a_rim_the_source_shows_where_no_caption_is_is_not_counted(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    frames: Frames = {
        SOURCE: {0: _rim(_grey()), 5: _rim(_grey(), left=12)},  # moved 2 px five frames later
        CLEANED: {0: _rim(_grey())},
    }
    counts, asked = _count(monkeypatch, frames, _chunk({0: [], 5: []}))
    assert counts == [0]
    assert asked == [
        ("source.mp4", [0]),
        ("cleaned.mp4", [0]),
        ("source.mp4", [5]),
        ("source.mp4", [0]),
        ("cleaned.mp4", [0]),
    ]


def test_a_remnant_absent_where_no_caption_is_keeps_counting(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    frames: Frames = {SOURCE: {0: _rim(_grey()), 5: _grey()}, CLEANED: {0: _rim(_grey())}}
    counts, asked = _count(monkeypatch, frames, _chunk({0: [], 5: []}))
    assert counts == [400]
    assert asked == [("source.mp4", [0]), ("cleaned.mp4", [0]), ("source.mp4", [5])]


def test_a_frame_whose_audit_still_reads_a_caption_there_is_no_reference(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    # The plan missed the caption in frame 5 (no box) but the audit read it there: 10/22.
    frames: Frames = {SOURCE: {0: _rim(_grey()), 5: _rim(_grey())}, CLEANED: {0: _rim(_grey())}}
    counts, asked = _count(monkeypatch, frames, _chunk({0: [], 5: [(10, 18, 60, 30)]}))
    assert counts == [400]
    assert asked == [("source.mp4", [0]), ("cleaned.mp4", [0])]


def test_interface_text_read_beside_it_does_not_block_the_reference(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    frames: Frames = {SOURCE: {0: _rim(_grey()), 5: _rim(_grey())}, CLEANED: {0: _rim(_grey())}}
    lines = {0: [(10, 18, 60, 30)], 5: [(10, 18, 60, 30)]}
    evidence = scene.Evidence([[(0, 15, 80, 35)]] + [[]] * 10, lines, lambda _f, _i: True)
    counts, _ = _count(monkeypatch, frames, evidence)
    assert counts == [0]


def test_a_component_a_caption_line_touches_in_its_own_frame_is_never_tested(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    frames: Frames = {SOURCE: {0: _rim(_grey()), 5: _rim(_grey())}, CLEANED: {0: _rim(_grey())}}
    counts, asked = _count(monkeypatch, frames, _chunk({0: [(10, 18, 60, 30)], 5: []}))
    assert counts == [400]
    assert asked == [("source.mp4", [0]), ("cleaned.mp4", [0])]


def test_a_frame_under_the_gate_is_not_decoded(monkeypatch: pytest.MonkeyPatch) -> None:
    counts, asked = _count(monkeypatch, {}, _chunk({0: [], 5: []}), counts=(39,))
    assert counts == [39]
    assert asked == []


def test_a_caption_box_in_every_nearby_frame_leaves_no_reference() -> None:
    evidence = scene.Evidence([[(0, 0, 80, 60)]] * 11, {0: [], 5: [], 10: []}, _no_interface)
    assert scene.caption_free((10, 20, 60, 28), 0, evidence) == ()


def test_references_are_the_nearest_free_audited_frames_up_to_the_limit() -> None:
    audited: dict[int, list[tuple[int, int, int, int]]] = {index: [] for index in range(0, 400, 5)}
    evidence = scene.Evidence([[]] * 400, audited, _no_interface)
    assert scene.caption_free((10, 20, 60, 28), 100, evidence) == (
        95, 105, 90, 110, 85, 115, 80, 120,
    )  # fmt: skip


def test_a_shift_past_the_frame_edge_is_skipped() -> None:
    part = np.ones((2, 2), dtype=np.uint8)
    pixels = np.full((2, 2, 3), 200, dtype=np.uint8)
    component = scene.Component(0, (0, 0, 2, 2), part, pixels, (5,))
    reference = np.full((4, 4, 3), 200, dtype=np.uint8)
    assert scene.present(component, reference)
    assert not scene.present(component, np.zeros((4, 4, 3), dtype=np.uint8))
