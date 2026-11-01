"""Unchanged caption colour inside detected boxes is residual; changed or outside is not."""

import numpy as np
import pytest

from replay import subtitle_colour_residual as residual

PINK = (180, 105, 255)


def _frame() -> np.ndarray:
    return np.full((20, 30, 3), 200, dtype=np.uint8)


def _inside() -> np.ndarray:
    allowed = np.zeros((20, 30), dtype=np.uint8)
    allowed[5:15, 5:25] = 1
    return allowed


def test_unchanged_pink_inside_the_boxes_is_counted() -> None:
    original = _frame()
    original[10, 10:13] = PINK
    assert residual.unchanged_pink(original.copy(), original, _inside()) == 3


def test_repaired_pink_is_not_counted() -> None:
    original = _frame()
    original[10, 10:13] = PINK
    cleaned = _frame()
    assert residual.unchanged_pink(cleaned, original, _inside()) == 0


def test_scene_pink_outside_the_boxes_is_not_counted() -> None:
    original = _frame()
    original[2, 2:6] = PINK
    assert residual.unchanged_pink(original.copy(), original, _inside()) == 0


def test_unchanged_pink_inside_a_solid_scene_object_is_not_counted() -> None:
    original = np.full((80, 80, 3), 200, dtype=np.uint8)
    original[10:70, 10:70] = (150, 140, 250)  # salmon mascot body overlapping the caption box
    original[40, 30:33] = PINK  # caption-hue speckle inside the body
    original[5, 5:8] = PINK  # thin caption stroke beside the body stays a residual
    allowed = np.ones((80, 80), dtype=np.uint8)
    assert residual.unchanged_pink(original.copy(), original, allowed) == 3


def test_geometry_mismatch_is_rejected() -> None:
    with pytest.raises(ValueError, match="one geometry"):
        residual.unchanged_pink(_frame(), np.zeros((10, 10, 3), np.uint8), _inside())


def test_flagged_reports_positions_at_or_above_the_gate() -> None:
    assert residual.flagged([0, 39, 40, 500]) == (2, 3)


def test_gate_must_be_positive() -> None:
    with pytest.raises(ValueError, match="positive"):
        residual.flagged([1], gate=0)


def test_a_scene_object_cut_by_caption_glyphs_and_restored_by_the_repair_is_not_counted() -> None:
    # A solid pink object under a white caption line: the glyphs cut it into strips narrower
    # than the opening in the original, and the repair restores it whole.
    cleaned = np.full((80, 80, 3), 200, dtype=np.uint8)
    cleaned[10:70, 10:70] = PINK
    original = cleaned.copy()
    original[10:70, 18:70:10] = 255  # white glyph strokes every 10 columns
    allowed = np.ones((80, 80), dtype=np.uint8)
    assert residual.unchanged_pink(cleaned, original, allowed) == 0


def test_a_glyph_stroke_left_by_the_repair_stays_a_residual() -> None:
    original = _frame()
    original[10, 5:25] = PINK  # a caption stroke the repair did not remove
    assert residual.unchanged_pink(original.copy(), original, _inside()) == 20


def test_an_outline_left_on_a_pink_object_of_another_shade_stays_a_residual() -> None:
    # The repair removed a glyph's white fill over a deep-pink object but left its brighter
    # pink outline: the object is solid in the repaired frame, the outline is not its colour.
    original = np.full((80, 80, 3), 200, dtype=np.uint8)
    original[10:70, 10:70] = (120, 60, 200)  # deep-pink object
    original[10:70, 30:34] = 255  # white glyph fill
    original[10:70, 28:30] = PINK  # its bright pink outline
    original[10:70, 34:36] = PINK
    cleaned = original.copy()
    cleaned[10:70, 30:34] = (120, 60, 200)  # the fill restored to the object, outline left
    allowed = np.ones((80, 80), dtype=np.uint8)
    # The left outline (2 x 60) counts; the right one touches the strip that was already solid
    # in the original and stays exempt as before. Without the colour anchor both were exempt.
    assert residual.unchanged_pink(cleaned, original, allowed) == 120


def test_unchanged_pink_beside_no_white_counts_only_its_outlining_pixels() -> None:
    # A sticker's pink beside the caption line: 120 unchanged pink pixels, 14 of them (12%, rows
    # 12-13 x columns 28-34) within two pixels of white, so only those 14 count.
    original = np.full((40, 40, 3), 90, dtype=np.uint8)
    original[10:14, 5:35] = PINK
    original[14, 30:35] = 255
    allowed = np.ones((40, 40), np.uint8)
    assert residual.unchanged_pink(original.copy(), original, allowed, outlined=True) == 14
    assert residual.unchanged_pink(original.copy(), original, allowed) == 120  # other styles


def test_a_caption_outline_beside_white_fill_counts_whole() -> None:
    original = np.full((40, 40, 3), 90, dtype=np.uint8)
    original[10:16, 5:35] = PINK
    original[12:14, 7:33] = 255  # the glyph's white fill inside its pink outline
    allowed = np.ones((40, 40), np.uint8)
    assert residual.unchanged_pink(original.copy(), original, allowed, outlined=True) == 128


def test_the_mask_around_the_boxes_equals_the_full_frame_mask() -> None:
    rng = np.random.default_rng(7)
    original = rng.integers(0, 256, (400, 300, 3), dtype=np.uint8)
    original[150:260, 40:200] = PINK  # a solid pink object, partly under the box
    original[200:205, 60:180] = (255, 255, 255)  # a white glyph stroke on it
    cleaned = original.copy()
    cleaned[170:180, 120:160] = (90, 90, 90)  # the repair changed part of the box
    cleaned[230:240, 50:70] = (170, 110, 250)  # a shade of pink restored beside the object
    allowed = np.zeros((400, 300), dtype=np.uint8)
    allowed[160:250, 50:190] = 1
    full = residual._residual_mask(cleaned, original, allowed)
    assert full.any()
    assert np.array_equal(residual.residual_mask(cleaned, original, allowed), full)
    assert not residual.residual_mask(cleaned, original, np.zeros_like(allowed)).any()
    beside = int((full & residual.beside_white(original)).sum())
    total = int(full.sum())
    expected = total if beside >= residual.OUTLINE_SHARE * total else beside
    assert residual.unchanged_pink(cleaned, original, allowed, outlined=True) == expected
