"""The left and right margins captions keep clear of, fixed per frame size.

This table is the one place they are set. ``caption_plan`` writes a delivery's margins into its
plan, the viewers and ``caption-burn`` keep each block between them, and paging wraps lines to the
width they leave. 1080x1920's 54 px each side is the 972 px line width the style-20 calibration
has always used.
"""

from fractions import Fraction

from replay.core import ReplayError

SAFE_ZONES: dict[tuple[int, int], tuple[int, int]] = {
    (720, 1280): (36, 36),
    (1080, 1920): (54, 54),
    (1280, 720): (64, 64),
    (1920, 1080): (96, 96),
    (3840, 2160): (192, 192),
}


def safe_zone(width: int, height: int) -> tuple[int, int]:
    """Margins in pixels: the table's row, or for another size the row nearest in aspect (the
    smaller on a tie), scaled to this width and rounded to whole pixels."""
    if width <= 0 or height <= 0:
        raise ReplayError("A frame has a positive size")
    row = SAFE_ZONES.get((width, height))
    if row is not None:
        return row
    aspect = Fraction(width, height)
    nearest = min(SAFE_ZONES, key=lambda size: (abs(Fraction(*size) - aspect), size))
    left, right = SAFE_ZONES[nearest]
    return (round(Fraction(left * width, nearest[0])), round(Fraction(right * width, nearest[0])))


def safe_width(width: int, height: int) -> int:
    """How wide a caption block may be between the margins."""
    left, right = safe_zone(width, height)
    return width - left - right
