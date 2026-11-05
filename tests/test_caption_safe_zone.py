import pytest

from replay.caption_safe_zone import safe_width, safe_zone
from replay.core import ReplayError


def test_each_listed_size_has_its_own_margins() -> None:
    assert safe_zone(720, 1280) == (36, 36)
    assert safe_zone(1080, 1920) == (54, 54)
    assert safe_zone(1280, 720) == (64, 64)
    assert safe_zone(1920, 1080) == (96, 96)
    assert safe_zone(3840, 2160) == (192, 192)


def test_an_unlisted_size_scales_the_row_nearest_in_aspect() -> None:
    assert safe_zone(2560, 1440) == (128, 128)
    assert safe_zone(1440, 2560) == (72, 72)
    assert safe_zone(1000, 1000) == (50, 50)


def test_the_portrait_width_is_the_calibrated_line_width() -> None:
    assert safe_width(1080, 1920) == 972


def test_a_frame_without_a_size_is_refused() -> None:
    with pytest.raises(ReplayError):
        safe_zone(0, 1920)
