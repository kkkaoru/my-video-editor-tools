"""The YOLO admission keeps the OCR admission's schema and rules without any planning OCR."""

from fractions import Fraction

import pytest

from replay import subtitle_sampled_admission as sampled
from replay import subtitle_yolo_admission as admission
from replay.subtitle_cleanup_plan import CaptionSearchProfile
from replay.subtitle_clock import VideoFrameClock
from replay.subtitle_profile import SubtitleBands

CLOCK = VideoFrameClock(tuple(range(0, 6 * 3000, 3000)), Fraction(1, 90000), 3000)
PROFILE = CaptionSearchProfile(
    SubtitleBands(1080, 1920, primary=(42,), secondary=()), (0, 1283, 1077, 167)
)
LINE = (200.4, 1330.2, 799.6, 1377.9)


def test_lines_are_integer_horizontal_boxes_touching_the_zone() -> None:
    zone = (0, 1283, 1077, 1450)
    boxes = [LINE, LINE, (10.0, 100.0, 300.0, 140.0), (700.0, 1300.0, 720.0, 1400.0)]
    assert admission.frame_lines(boxes, zone) == [[200, 1330, 800, 1378]]


def test_neighbourhood_lines_unite_the_frames_within_the_radius() -> None:
    per_frame = [[[1, 1, 9, 3]], [], [[2, 5, 9, 7]], [[1, 1, 9, 3]]]
    assert admission.neighbourhood_lines(per_frame, 1, 1) == [[1, 1, 9, 3], [2, 5, 9, 7]]
    assert admission.neighbourhood_lines(per_frame, 0, 0) == [[1, 1, 9, 3]]


def test_frames_near_a_caption_are_regions_and_the_rest_stay_unknown() -> None:
    boxes = [[LINE], [], [], [], [], []]
    result = admission.admission(boxes=boxes, clock=CLOCK, profile=PROFILE, radius=1)
    frames = result["frames"]
    assert isinstance(frames, list)
    assert [frame["classification"] for frame in frames] == [
        "overlay-subtitle",
        "overlay-subtitle",
        "unknown",
        "unknown",
        "unknown",
        "unknown",
    ]
    assert frames[1]["lineBoxes"] == [[200, 1330, 800, 1378]]
    assert frames[1]["sampled"] is False
    assert frames[0]["region"] == {"x": 194, "y": 1282, "width": 612, "height": 144}
    assert frames[2]["region"] == {"x": 0, "y": 1200, "width": 1077, "height": 333}
    assert frames[2]["lineBoxes"] == [[0, 1200, 1077, 1533]]
    assert result["unknownSourcePTS"] == [6000, 9000, 12000, 15000]
    assert result["policy"] == "yolo-caption-boxes-v1"
    assert result["sampling"] == {
        "version": 1,
        "kind": "yolo-caption-boxes",
        "rawOcr": False,
        "interpolated": False,
        "frames": 6,
        "detectedFrames": 6,
        "qualityApproved": False,
    }
    assert result["masks"] == [
        {"startSeconds": 0.0, "endSeconds": 1 / 30, "region": frames[0]["region"]},
        {"startSeconds": 1 / 30, "endSeconds": 2 / 30, "region": frames[0]["region"]},
    ]


def test_the_last_frame_mask_ends_at_the_chunk_end() -> None:
    boxes = [[], [], [], [], [], [LINE]]
    result = admission.admission(boxes=boxes, clock=CLOCK, profile=PROFILE, radius=0)
    masks = result["masks"]
    assert isinstance(masks, list)
    assert masks[0]["endSeconds"] == pytest.approx(0.2)


def test_boxes_must_cover_the_chunk_and_the_radius_must_not_be_negative() -> None:
    with pytest.raises(ValueError, match="do not cover"):
        admission.admission(boxes=[[LINE]], clock=CLOCK, profile=PROFILE, radius=1)
    with pytest.raises(ValueError, match="do not cover"):
        admission.admission(boxes=[[]] * 6, clock=CLOCK, profile=PROFILE, radius=-1)


def test_a_tall_caption_zone_grows_only_as_far_as_the_detector_budget() -> None:
    # Captions measured at several heights: half the zone's height each way would exceed the
    # 2,000,000-pixel budget, so the zone grows 427 rows instead of 501 and stays whole.
    tall = CaptionSearchProfile(PROFILE.bands, (0, 355, 1077, 1003))
    boxes = [[], [], [], [], [], [(200.4, 900.2, 799.6, 950.9)]]
    result = admission.admission(boxes=boxes, clock=CLOCK, profile=tall, radius=0)
    frames = result["frames"]
    assert isinstance(frames, list)
    assert frames[0]["region"] == {"x": 0, "y": 0, "width": 1077, "height": 1785}
    assert frames[5]["region"] == {"x": 194, "y": 849, "width": 612, "height": 153}


def test_zone_growth_is_half_the_zone_until_the_budget_binds() -> None:
    assert sampled.zone_growth((0, 1283, 1077, 1450)) == 83
    assert sampled.zone_growth((0, 355, 1077, 1358)) == 427
    assert sampled.zone_growth((0, 0, 1080, 1920)) == 0


def test_a_zone_beyond_the_budget_by_itself_is_still_refused() -> None:
    whole = CaptionSearchProfile(PROFILE.bands, (0, 0, 1080, 1920))
    with pytest.raises(ValueError, match="detector budget"):
        admission.admission(boxes=[[]] * 6, clock=CLOCK, profile=whole, radius=0)
