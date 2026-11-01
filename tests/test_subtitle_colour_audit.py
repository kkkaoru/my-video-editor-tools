"""Audited frames of real (lavfi) videos give exact unchanged-caption-colour counts."""

import subprocess
from collections.abc import Iterator, Sequence
from pathlib import Path

import pytest

from replay import subtitle_colour_audit as audit
from replay.subtitle_inpaint import Image

SIZE = (160, 120)
BOX = (20, 40, 50, 50)


def _video(path: Path, *, pink: bool) -> Path:
    graph = "color=c=gray:s=160x120:r=30:d=1"
    if pink:
        # A caption glyph: white fill outlined by two pixels of caption pink (144 pink pixels).
        graph += (
            ",drawbox=x=20:y=40:w=30:h=10:color=0xFF69B4:t=fill"
            ",drawbox=x=22:y=42:w=26:h=6:color=white:t=fill"
        )
    subprocess.run(
        ["ffmpeg", "-nostdin", "-v", "error", "-f", "lavfi", "-i", graph, "-pix_fmt", "yuv444p",
         "-c:v", "libx264", "-qp", "0", "-y", str(path)],
        check=True,
        timeout=60,
    )  # fmt: skip
    return path


def _boxes() -> list[list[tuple[int, int, int, int]]]:
    return [[BOX] for _ in range(30)]


def test_unchanged_caption_colour_inside_the_box_is_counted(tmp_path: Path) -> None:
    original = _video(tmp_path / "original.mp4", pink=True)
    counts = audit.colour_counts(
        ffmpeg="ffmpeg", source=original, source_start=0.0, cleaned=original,
        indices=[0, 7, 29], boxes=_boxes(), size=SIZE,
    )  # fmt: skip
    assert counts == [144, 144, 144]


def test_repaired_frames_count_zero(tmp_path: Path) -> None:
    original = _video(tmp_path / "original.mp4", pink=True)
    cleaned = _video(tmp_path / "cleaned.mp4", pink=False)
    counts = audit.colour_counts(
        ffmpeg="ffmpeg", source=original, source_start=0.0, cleaned=cleaned,
        indices=[3, 10], boxes=_boxes(), size=SIZE,
    )  # fmt: skip
    assert counts == [0, 0]


def test_source_offset_seeks_before_counting(tmp_path: Path) -> None:
    original = _video(tmp_path / "original.mp4", pink=True)
    counts = audit.colour_counts(
        ffmpeg="ffmpeg", source=original, source_start=0.5, cleaned=original,
        indices=[0], boxes=_boxes(), size=SIZE,
    )  # fmt: skip
    assert counts == [144]


def test_indices_must_be_ascending_and_unique(tmp_path: Path) -> None:
    original = _video(tmp_path / "original.mp4", pink=True)
    with pytest.raises(ValueError, match="ascending"):
        audit.colour_counts(
            ffmpeg="ffmpeg", source=original, source_start=0.0, cleaned=original,
            indices=[5, 5], boxes=_boxes(), size=SIZE,
        )  # fmt: skip


def test_truncated_decode_is_rejected(tmp_path: Path) -> None:
    original = _video(tmp_path / "original.mp4", pink=True)
    with pytest.raises(ValueError, match="before every audited frame"):
        audit.colour_counts(
            ffmpeg="ffmpeg", source=original, source_start=0.0, cleaned=original,
            indices=[0, 60], boxes=[[BOX]] * 61, size=SIZE,
        )  # fmt: skip


def test_decoder_failure_is_reported(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="before every audited frame"):
        audit.colour_counts(
            ffmpeg="ffmpeg", source=tmp_path / "missing.mp4", source_start=0.0,
            cleaned=tmp_path / "missing.mp4", indices=[0], boxes=_boxes(), size=SIZE,
        )  # fmt: skip


def test_selector_groups_arithmetic_runs_and_single_frames() -> None:
    assert audit._selector([0, 5, 10, 899]) == ("between(n\\,0\\,10)*not(mod(n-0\\,5))+eq(n\\,899)")


def test_selector_handles_a_single_index() -> None:
    assert audit._selector([7]) == "eq(n\\,7)"


def test_long_audit_runs_decode_with_a_compact_expression(tmp_path: Path) -> None:
    original = _video(tmp_path / "original.mp4", pink=True)
    counts = audit.colour_counts(
        ffmpeg="ffmpeg", source=original, source_start=0.0, cleaned=original,
        indices=list(range(0, 30, 2)), boxes=_boxes(), size=SIZE,
    )  # fmt: skip
    assert counts == [144] * 15


def test_source_pink_counts_measure_caption_colour_inside_each_line_box(tmp_path: Path) -> None:
    original = _video(tmp_path / "original.mp4", pink=True)
    counts = audit.source_pink_counts(
        ffmpeg="ffmpeg", source=original, source_start=0.0,
        lines=[
            (7, (20.0, 40.0, 30.0, 10.0)),
            (7, (100.0, 40.0, 30.0, 10.0)),
            (3, (20.0, 40.0, 5.0, 5.0)),
        ],
        size=SIZE,
    )  # fmt: skip
    assert counts == [144, 0, 20]


def test_shared_source_masks_decode_each_frame_once_and_give_the_same_counts(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    original = _video(tmp_path / "original.mp4", pink=True)
    lines = [(7, (20.0, 40.0, 30.0, 10.0)), (3, (20.0, 40.0, 5.0, 5.0))]
    plain = audit.source_pink_counts(
        ffmpeg="ffmpeg", source=original, source_start=0.0, lines=lines, size=SIZE
    )
    shares = audit.source_outlined_shares(
        ffmpeg="ffmpeg", source=original, source_start=0.0, lines=lines, size=SIZE
    )
    decoded: list[list[int]] = []
    frames = audit._frames

    def counted(
        ffmpeg: str, video: Path, start: float, indices: Sequence[int], size: tuple[int, int]
    ) -> Iterator[Image]:
        decoded.append(list(indices))
        return frames(ffmpeg, video, start, indices, size)

    monkeypatch.setattr(audit, "_frames", counted)
    masks = audit.SourceMasks("ffmpeg", original, 0.0, SIZE)
    masks.expect([3, 7, 9])
    assert plain == audit.source_pink_counts(
        ffmpeg="ffmpeg", source=original, source_start=0.0, lines=lines, size=SIZE, masks=masks
    )
    assert shares == audit.source_outlined_shares(
        ffmpeg="ffmpeg", source=original, source_start=0.0, lines=lines, size=SIZE, masks=masks
    )
    assert audit.source_pink_counts(
        ffmpeg="ffmpeg", source=original, source_start=0.0, lines=[(9, lines[0][1])], size=SIZE,
        masks=masks,
    ) == [144]  # fmt: skip
    assert decoded == [[3, 7, 9]]  # one decode, every expected frame with the first ask


def test_source_pink_counts_need_at_least_one_line(tmp_path: Path) -> None:
    original = _video(tmp_path / "original.mp4", pink=True)
    with pytest.raises(ValueError, match="No lines"):
        audit.source_pink_counts(
            ffmpeg="ffmpeg", source=original, source_start=0.0, lines=[], size=SIZE
        )


def test_band_rows_and_reused_bands_give_the_full_frame_counts(tmp_path: Path) -> None:
    original = _video(tmp_path / "original.mp4", pink=True)
    # An odd top: the crop really starts one row higher, as the band audit's decode does.
    band = (7, 76)
    bands = list(audit._frames("ffmpeg", original, 0.0, [0, 9], SIZE, band))
    counts = audit.colour_counts(
        ffmpeg="ffmpeg", source=original, source_start=0.0, cleaned=original,
        indices=[0, 9], boxes=_boxes(), size=SIZE, band=band, cleaned_bands=bands,
    )  # fmt: skip
    assert [frame.shape for frame in bands] == [(76, 160, 3), (76, 160, 3)]
    assert counts == [144, 144]


def test_source_bands_decoded_ahead_replace_the_source_decode(tmp_path: Path) -> None:
    original = _video(tmp_path / "original.mp4", pink=True)
    band = (7, 76)
    bands = list(audit._frames("ffmpeg", original, 0.0, [0, 9], SIZE, band))
    counts = audit.colour_counts(
        ffmpeg="ffmpeg", source=tmp_path / "missing.mp4", source_start=0.0,
        cleaned=tmp_path / "missing.mp4", indices=[0, 9], boxes=_boxes(), size=SIZE, band=band,
        cleaned_bands=bands, source_bands=bands,
    )  # fmt: skip
    assert counts == [144, 144]


def test_boxes_near_the_band_edge_are_counted_on_full_frames(tmp_path: Path) -> None:
    original = _video(tmp_path / "original.mp4", pink=True)
    counts = audit.colour_counts(
        ffmpeg="ffmpeg", source=original, source_start=0.0, cleaned=original,
        indices=[0], boxes=_boxes(), size=SIZE, band=(30, 40), cleaned_bands=[],
        source_bands=[],
    )  # fmt: skip
    assert counts == [144]


def test_a_crop_starts_on_an_even_row() -> None:
    assert (audit.crop_origin(7), audit.crop_origin(8)) == (6, 8)


def test_boxes_inside_the_band_leave_the_reach_on_both_sides() -> None:
    assert audit.inside((6, 76), [[(20, 38, 50, 50)]]) is True
    assert audit.inside((6, 76), [[(20, 37, 50, 50)]]) is False
    assert audit.inside((6, 76), [[(20, 40, 50, 51)]]) is False


def test_residual_boxes_bound_the_unchanged_caption_colour_of_the_given_frames(
    tmp_path: Path,
) -> None:
    original = _video(tmp_path / "original.mp4", pink=True)
    found = audit.residual_boxes(
        ffmpeg="ffmpeg", source=original, source_start=0.0, cleaned=original,
        indices=[7, 29], boxes=_boxes(), size=SIZE,
    )  # fmt: skip
    assert found == {7: [(20, 40, 50, 50)], 29: [(20, 40, 50, 50)]}


def test_repaired_frames_have_no_residual_boxes(tmp_path: Path) -> None:
    original = _video(tmp_path / "original.mp4", pink=True)
    cleaned = _video(tmp_path / "cleaned.mp4", pink=False)
    found = audit.residual_boxes(
        ffmpeg="ffmpeg", source=original, source_start=0.0, cleaned=cleaned,
        indices=[3], boxes=_boxes(), size=SIZE,
    )  # fmt: skip
    assert found == {3: []}


def test_residual_boxes_need_ascending_unique_frames(tmp_path: Path) -> None:
    original = _video(tmp_path / "original.mp4", pink=True)
    with pytest.raises(ValueError, match="ascending and unique"):
        audit.residual_boxes(
            ffmpeg="ffmpeg", source=original, source_start=0.0, cleaned=original,
            indices=[], boxes=_boxes(), size=SIZE,
        )  # fmt: skip


def test_excluded_boxes_are_not_counted(tmp_path: Path) -> None:
    original = _video(tmp_path / "original.mp4", pink=True)
    excluded: list[list[tuple[int, int, int, int]]] = [[] for _ in range(30)]
    excluded[7] = [(20, 40, 50, 50)]  # the whole pink area
    counts = audit.colour_counts(
        ffmpeg="ffmpeg", source=original, source_start=0.0, cleaned=original,
        indices=[0, 7], boxes=_boxes(), size=SIZE, excluded=excluded,
    )  # fmt: skip
    assert counts == [144, 0]


def _outlined_video(path: Path, *, outline: str) -> Path:
    graph = (
        "color=c=gray:s=160x120:r=30:d=1,"
        f"drawbox=x=20:y=40:w=40:h=14:color={outline}:t=fill,"
        "drawbox=x=23:y=43:w=34:h=8:color=white:t=fill"
    )
    subprocess.run(
        ["ffmpeg", "-nostdin", "-v", "error", "-f", "lavfi", "-i", graph, "-pix_fmt", "yuv444p",
         "-c:v", "libx264", "-qp", "0", "-y", str(path)],
        check=True,
        timeout=60,
    )  # fmt: skip
    return path


def test_caption_styled_text_has_pink_outlining_white_and_grey_outlined_text_none(
    tmp_path: Path,
) -> None:
    captioned = _outlined_video(tmp_path / "caption.mp4", outline="0xFF69B4")
    label = _outlined_video(tmp_path / "label.mp4", outline="0x808080")
    line = [(0, (20.0, 40.0, 39.0, 13.0))]
    caption_share = audit.source_outlined_shares(
        ffmpeg="ffmpeg", source=captioned, source_start=0.0, lines=line, size=SIZE
    )
    label_share = audit.source_outlined_shares(
        ffmpeg="ffmpeg", source=label, source_start=0.0, lines=line, size=SIZE
    )
    assert caption_share[0] >= audit.MINIMUM_OUTLINED_SHARE
    assert label_share == [0.0]
    with pytest.raises(ValueError, match="No lines"):
        audit.source_outlined_shares(
            ffmpeg="ffmpeg", source=label, source_start=0.0, lines=[], size=SIZE
        )


def test_line_shares_read_each_time_once_and_style_needs_outlined_lines(tmp_path: Path) -> None:
    captioned = _outlined_video(tmp_path / "caption.mp4", outline="0xFF69B4")
    grey = _outlined_video(tmp_path / "grey.mp4", outline="0x808080")
    lines = [(t / 30, (20.0, 40.0, 39.0, 13.0)) for t in range(12)]
    pink_shares = audit.line_shares(ffmpeg="ffmpeg", source=captioned, lines=lines, size=SIZE)
    grey_shares = audit.line_shares(ffmpeg="ffmpeg", source=grey, lines=lines, size=SIZE)
    assert len(pink_shares) == 12
    assert audit.outlined_style(pink_shares) is True
    # White captions in a grey outline: no caption pink, so the outline rules stay off.
    assert grey_shares == [0.0] * 12
    assert audit.outlined_style(grey_shares) is False
    assert audit.outlined_style(pink_shares[:9]) is False  # too few lines decide nothing
