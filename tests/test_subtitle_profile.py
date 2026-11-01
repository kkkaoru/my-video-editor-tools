from dataclasses import replace

import pytest

from replay.caption_io import FrameObservation
from replay.caption_masks import TextDetection, TextRectangle
from replay.subtitle_profile import (
    SubtitleBands,
    audited_interface,
    interface_text,
    interface_vocabulary,
    japanese_letters,
    learn_subtitle_bands,
    profiled_detections,
    still_label_reads,
)


@pytest.fixture
def observations() -> tuple[FrameObservation, ...]:
    japanese = (
        "春の花",
        "遠い海",
        "今日の天気",
        "朝の挨拶",
        "夕焼けの空",
        "白い雲",
        "青い鳥",
        "静かな森",
        "新しい本",
        "明日の予定",
    )
    translations = (
        "spring flowers",
        "distant sea",
        "today's weather",
        "morning greeting",
        "sunset sky",
        "white clouds",
        "blue bird",
        "quiet forest",
        "new book",
        "tomorrow's plans",
    )
    return tuple(
        map(
            lambda index: FrameObservation(
                index,
                index,
                1080,
                1920,
                (
                    TextDetection(japanese[index], 1, TextRectangle(340, 1296, 400, 32)),
                    TextDetection(translations[index], 1, TextRectangle(340, 1408, 400, 24)),
                    TextDetection("STATIC", 1, TextRectangle(400, 1200, 300, 24)),
                    TextDetection(f"shirt{index}", 1, TextRectangle(440, 1220, 200, 24)),
                ),
            ),
            range(10),
        )
    )


def test_variable_japanese_and_lower_translation_seed_bands(
    observations: tuple[FrameObservation, ...],
) -> None:
    assert learn_subtitle_bands(observations) == SubtitleBands(1080, 1920, (41,), (44,))


def test_static_and_upper_roman_text_do_not_seed_a_blanket_mask(
    observations: tuple[FrameObservation, ...],
) -> None:
    result = profiled_detections(observations[0], learn_subtitle_bands(observations))
    assert result == (
        TextDetection("春の花", 1, TextRectangle(340, 1296, 400, 32)),
        TextDetection("spring flowers", 1, TextRectangle(340, 1408, 400, 24)),
    )


def test_changing_counters_do_not_seed_primary_or_secondary_bands(
    observations: tuple[FrameObservation, ...],
) -> None:
    frames = tuple(
        map(
            lambda frame: replace(
                frame,
                detections=(
                    *frame.detections,
                    TextDetection(
                        f"参加者 {frame.actual_time} 人", 1, TextRectangle(340, 288, 400, 32)
                    ),
                    TextDetection(
                        f"count {frame.actual_time}", 1, TextRectangle(340, 1472, 400, 24)
                    ),
                ),
            ),
            observations,
        )
    )
    assert learn_subtitle_bands(frames) == SubtitleBands(1080, 1920, (41,), (44,), ("参加者人",))
    assert frames[0].detections[-2].text == "参加者 0 人"


def test_fullwidth_counter_changes_alone_refuse_a_blanket_mask() -> None:
    frames = tuple(
        map(
            lambda digit: FrameObservation(
                0,
                0,
                1080,
                1920,
                (TextDetection(f"参加者　{digit}　人", 1, TextRectangle(340, 1296, 400, 32)),),
            ),
            "０１２３４５６７８９",
        )
    )
    with pytest.raises(ValueError, match="blanket mask"):
        learn_subtitle_bands(frames)


def test_isolated_upper_object_rejected_but_supported_wrapped_line_kept() -> None:
    frame = FrameObservation(
        0,
        0,
        1080,
        1920,
        (
            TextDetection("字幕", 1, TextRectangle(340, 1296, 400, 32)),
            TextDetection("続き", 1, TextRectangle(480, 1272, 120, 32)),
            TextDetection("badge", 1, TextRectangle(760, 1272, 80, 24)),
        ),
    )
    assert profiled_detections(frame, SubtitleBands(1080, 1920, (41,), ())) == (
        TextDetection("字幕", 1, TextRectangle(340, 1296, 400, 32)),
        TextDetection("続き", 1, TextRectangle(480, 1272, 120, 32)),
    )


def test_empty_and_low_confidence_are_not_invented_detections() -> None:
    frame = FrameObservation(
        0, 0, 1080, 1920, (TextDetection("字幕", 0.1, TextRectangle(340, 1296, 400, 32)),)
    )
    assert profiled_detections(frame, SubtitleBands(1080, 1920, (41,), ())) == ()


def test_script_detection_does_not_treat_roman_or_long_vowel_mark_as_japanese() -> None:
    assert japanese_letters("語句") is True
    assert japanese_letters("KANAー") is False


def test_empty_profile_input() -> None:
    with pytest.raises(ValueError, match="budget"):
        learn_subtitle_bands(())


def test_static_japanese_alone_is_not_enough() -> None:
    frames = (
        FrameObservation(
            0, 0, 1080, 1920, (TextDetection("固定", 1, TextRectangle(340, 1296, 400, 32)),)
        ),
    ) * 10
    with pytest.raises(ValueError, match="blanket mask"):
        learn_subtitle_bands(frames)


def test_wrong_canvas() -> None:
    with pytest.raises(ValueError, match="canvas"):
        learn_subtitle_bands((FrameObservation(0, 0, 0, 1920, ()),))


def test_inconsistent_frames() -> None:
    with pytest.raises(ValueError, match="Inconsistent"):
        learn_subtitle_bands(
            (FrameObservation(0, 0, 1080, 1920, ()), FrameObservation(1, 1, 1920, 1080, ()))
        )


def test_outside_canvas_rejected() -> None:
    with pytest.raises(ValueError, match="exceeds canvas"):
        learn_subtitle_bands(
            (
                FrameObservation(
                    0,
                    0,
                    1080,
                    1920,
                    (TextDetection("字幕", 1, TextRectangle(1000, 1300, 200, 30)),),
                ),
            )
        )


def test_frame_profile_mismatch() -> None:
    with pytest.raises(ValueError, match="differs"):
        profiled_detections(
            FrameObservation(0, 0, 1920, 1080, ()), SubtitleBands(1080, 1920, (41,), ())
        )


def test_missing_bands_rejected() -> None:
    with pytest.raises(ValueError, match="Empty"):
        profiled_detections(
            FrameObservation(0, 0, 1080, 1920, ()), SubtitleBands(1080, 1920, (), ())
        )


def test_a_fixed_label_misread_by_ocr_does_not_seed_a_band(
    observations: tuple[FrameObservation, ...],
) -> None:
    readings = (
        "宝箱、ポータル禁止",
        "宝額、ポータル禁止",
        "宝組、ポータル禁止",
        "家稲、ポータル禁止",
        "品額、ポータル禁に",
        "宝箱、ポータル禁止",
        "定箱、ポータル禁止",
        "宝称、ポータル禁止",
        "宝箱、ポータル禁ル",
        "宝縮、ボータル禁止",
    )
    frames = tuple(
        replace(
            frame,
            detections=(
                *frame.detections,
                TextDetection(reading, 1, TextRectangle(388, 364, 592, 80)),
            ),
        )
        for frame, reading in zip(observations, readings, strict=True)
    )
    assert learn_subtitle_bands(frames) == SubtitleBands(
        1080,
        1920,
        (41,),
        (44,),
        (
            "定箱、ポータル禁止",
            "宝称、ポータル禁止",
            "宝箱、ポータル禁ル",
            "宝箱、ポータル禁止",
            "宝組、ポータル禁止",
            "宝縮、ボータル禁止",
            "宝額、ポータル禁止",
            "家稲、ポータル禁止",
        ),
    )


def test_a_label_alone_still_refuses_a_blanket_mask() -> None:
    readings = ("宝箱、ポータル禁止", "宝額、ポータル禁止", "宝組、ポータル禁止") * 4
    frames = tuple(
        FrameObservation(
            0, 0, 1080, 1920, (TextDetection(reading, 1, TextRectangle(388, 364, 592, 80)),)
        )
        for reading in readings
    )
    with pytest.raises(ValueError, match="blanket mask"):
        learn_subtitle_bands(frames)


CAPTIONS = (
    "春の花", "遠い海", "今日の天気", "朝の挨拶", "夕焼けの空", "白い雲", "青い鳥", "静かな森",
    "新しい本", "明日の予定", "古い寺", "冬の星", "赤い傘", "長い橋", "小さな港", "夜の街",
    "秋の風", "深い谷", "広い畑", "高い塔",
)  # fmt: skip
MENU = (
    "今日もありがとね",
    "人気あっぷ",
    "シェアハピ",
    "フォローありがとう",
    "超人気あっぷ",
    "かしすみろく",
)


def test_a_scrolling_interface_row_does_not_seed_a_band() -> None:
    # A small sound-menu row near the top scrolls (no shared cell), but its few texts return in
    # frame after frame; the captions vary. Only the caption band is learned.
    frames = tuple(
        FrameObservation(
            index,
            index,
            1080,
            1920,
            (
                TextDetection(MENU[index % 6], 1, TextRectangle(280 + 13 * index, 388, 216, 20)),
                TextDetection(CAPTIONS[index], 1, TextRectangle(340, 1256, 400, 40)),
            ),
        )
        for index in range(20)
    )
    assert learn_subtitle_bands(frames) == SubtitleBands(
        1080,
        1920,
        (39,),
        (),
        (
            "かしすみろく",
            "シェアハピ",
            "フォローありがとう",
            "人気あっぷ",
            "今日もありがとね",
            "超人気あっぷ",
        ),
    )


def test_a_caption_band_with_a_few_repeated_phrases_is_kept() -> None:
    # A quarter of the captions say the same thing: still fewer than half of the band's lines.
    texts = tuple("ありがとう" if index % 4 == 0 else CAPTIONS[index] for index in range(20))
    frames = tuple(
        FrameObservation(
            index, index, 1080, 1920, (TextDetection(text, 1, TextRectangle(340, 1256, 400, 40)),)
        )
        for index, text in enumerate(texts)
    )
    bands = learn_subtitle_bands(frames)
    assert bands == SubtitleBands(1080, 1920, (39,), (), ("ありがとう",))
    # The repeated caption entered the vocabulary, yet one line at one place stays a caption.
    assert profiled_detections(frames[0], bands) == (
        TextDetection("ありがとう", 1, TextRectangle(340, 1256, 400, 40)),
    )


def _label(text: str, x: float, y: float, height: float = 40) -> TextDetection:
    return TextDetection(text, 1, TextRectangle(x, y, 7 * height, height))


GIFT = "星降りメリー"
VOCABULARY = (GIFT, "りがとう")


def test_moving_copies_of_a_vocabulary_label_are_interface_text() -> None:
    frame = FrameObservation(
        0, 0, 1080, 1920,
        (
            _label(GIFT, 10, 1330, 36),
            _label("星降りメリ", 700, 1400, 52),  # another copy, cut by OCR, a little nearer
            _label("参降りメリー星降りメリー", 200, 1460),  # two copies read as one line
            _label("無理だったよね", 340, 1256),  # the caption
        ),
    )  # fmt: skip
    assert interface_text(frame, VOCABULARY) == {0, 1, 2}


def test_a_caption_saying_a_vocabulary_word_beside_small_buttons_stays_a_caption() -> None:
    frame = FrameObservation(
        0, 0, 1080, 1920,
        (
            _label("ありがとう", 340, 1256, 40),  # the caption
            _label("りがとう。", 216, 400, 20),  # interface buttons, half its height
            _label("りがとう", 892, 400, 20),
        ),
    )  # fmt: skip
    assert interface_text(frame, VOCABULARY) == {1, 2}


def test_a_single_stuck_vocabulary_line_is_not_interface_text() -> None:
    frame = FrameObservation(0, 0, 1080, 1920, (_label(GIFT, 340, 1256),))
    assert interface_text(frame, VOCABULARY) == set()
    assert interface_text(frame, ()) == set()


def test_moving_interface_text_is_not_a_profiled_caption() -> None:
    bands = SubtitleBands(1080, 1920, (41,), (), VOCABULARY)
    frame = FrameObservation(
        0, 0, 1080, 1920,
        (_label(GIFT, 300, 1320, 40), _label(GIFT, 600, 1330, 40), _label("無理だった", 340, 1300)),
    )  # fmt: skip
    assert profiled_detections(frame, bands) == (_label("無理だった", 340, 1300),)


def test_the_vocabulary_keeps_readings_recurring_anywhere_in_the_profile() -> None:
    frames = tuple(
        FrameObservation(
            index, index, 1080, 1920,
            (_label(GIFT, 37 * index, 1320 + 7 * index, 40), _label(CAPTIONS[index], 340, 1256)),
        )
        for index in range(20)
    )  # fmt: skip
    assert interface_vocabulary(frames) == (GIFT,)


def _audited(*frames: tuple[TextDetection, ...]) -> dict[int, FrameObservation]:
    return {
        5 * index: FrameObservation(0, 0, 1080, 1920, lines) for index, lines in enumerate(frames)
    }


def test_a_lone_vocabulary_label_that_moved_between_audited_frames_is_interface() -> None:
    observed = _audited((_label(GIFT, 300, 1300),), (_label(GIFT, 310, 1360),))
    assert audited_interface(observed, 0, 0, VOCABULARY, 5) is True
    assert audited_interface(observed, 5, 0, VOCABULARY, 5) is True


def test_a_caption_saying_a_vocabulary_word_that_stays_put_is_a_caption() -> None:
    caption = _label("ありがとう", 340, 1256)
    button = _label("りがとう", 892, 400, 20)  # the same word on a small button far away
    observed = _audited((caption, button), (caption, button))
    assert audited_interface(observed, 0, 0, VOCABULARY, 5) is False
    assert audited_interface(observed, 5, 0, VOCABULARY, 5) is False


def test_a_glyph_inside_a_moving_label_nearby_is_interface() -> None:
    glyph = TextDetection("星", 1, TextRectangle(305, 1365, 40, 40))
    observed = _audited((_label(GIFT, 300, 1300),), (_label(GIFT, 310, 1360), glyph))
    assert audited_interface(observed, 5, 1, VOCABULARY, 5) is True


def test_a_one_character_caption_remnant_without_a_moving_label_stays_a_residual() -> None:
    remnant = TextDetection("え", 1, TextRectangle(520, 1260, 40, 40))
    observed = _audited((_label(GIFT, 300, 1300), remnant), (_label(GIFT, 300, 1300), remnant))
    assert audited_interface(observed, 0, 1, VOCABULARY, 5) is False
    assert (
        audited_interface(
            _audited((TextDetection("", 1, TextRectangle(0, 0, 9, 9)),)), 0, 0, VOCABULARY, 5
        )
        is False
    )


def _read(text: str, confidence: float, x: float, width: float = 104) -> TextDetection:
    return TextDetection(text, confidence, TextRectangle(x, 1270, width, 28))


def test_a_misread_still_label_leads_to_its_vocabulary_read_through_more_misreads() -> None:
    observed = _audited(
        (_read("上分に目", 0.3, 866),),  # the label behind a crossing caption, misread
        (_read("生みとき", 0.3, 863),),
        (_read("星降りメリー", 0.5, 860),),  # read at last
    )
    assert still_label_reads(observed, 0, 0, VOCABULARY, 5) == [
        (10, _read("星降りメリー", 0.5, 860))
    ]


def test_a_confident_read_of_other_text_ends_the_chain() -> None:
    observed = _audited(
        (_read("上分に目", 0.3, 866),),
        (_read("無理だったよ", 1.0, 863),),  # a caption read clearly at that place
        (_read("星降りメリー", 1.0, 860),),
    )
    assert still_label_reads(observed, 0, 0, VOCABULARY, 5) == []
    assert still_label_reads(observed, 5, 0, VOCABULARY, 5) == []


def test_a_vocabulary_read_elsewhere_or_of_another_size_is_no_anchor() -> None:
    observed = _audited(
        (_read("上分に目", 0.3, 866),),
        (_read("星降りメリー", 1.0, 700), _read("星降りメリー", 1.0, 860, 300)),
    )
    assert still_label_reads(observed, 0, 0, VOCABULARY, 5) == []


def test_a_still_label_chain_skips_audited_frames_without_a_read_and_short_glyphs() -> None:
    observed = {
        0: FrameObservation(0, 0, 1080, 1920, (_read("星降りメリー", 0.5, 860),)),
        15: FrameObservation(0, 0, 1080, 1920, (_read("な", 1.0, 860),)),  # a glyph, read surely
        20: FrameObservation(0, 0, 1080, 1920, (_read("上分に目", 0.3, 866),)),
    }  # frames 5 and 10 read nothing at the place
    assert still_label_reads(observed, 20, 0, VOCABULARY, 5) == [
        (0, _read("星降りメリー", 0.5, 860))
    ]


def test_a_still_label_chain_stops_beyond_its_gap() -> None:
    observed = {
        0: FrameObservation(0, 0, 1080, 1920, (_read("星降りメリー", 0.5, 860),)),
        25: FrameObservation(0, 0, 1080, 1920, (_read("上分に目", 0.3, 866),)),
    }
    assert still_label_reads(observed, 25, 0, VOCABULARY, 5) == []


def test_a_label_passing_below_a_caption_fragment_is_no_anchor() -> None:
    fragment = TextDetection("こめんね", 0.3, TextRectangle(880, 1257, 104, 31))
    label = TextDetection("星降りメリー", 0.3, TextRectangle(870, 1276, 120, 28))  # 18 px lower
    observed = _audited((fragment,), (label,))
    assert still_label_reads(observed, 0, 0, VOCABULARY, 5) == []
