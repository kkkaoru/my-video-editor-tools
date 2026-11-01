"""A displayed caption stays long enough to read, never stale, never past the next one."""

from fractions import Fraction

from replay import caption_display_hold as hold

F = Fraction
FAR = F(1000)


def test_a_short_reply_is_held_to_the_minimum_display() -> None:
    # "え、" spoken in 0.25 s: half a second past its speech is still under 5/6 s on screen.
    end = hold.held_end("え、", F("54.14"), F("54.39"), following=FAR, rate=30)
    assert end == F("54.14") + F(5, 6)


def test_slow_speech_lingers_half_a_second() -> None:
    # 8 characters over 4 s (2 cps): the 4 cps norm is already met, so only the linger is added.
    assert hold.held_end("ゆっくり話してる", F(10), F(14), following=FAR, rate=30) == F("14.5")


def test_fast_speech_is_read_at_the_fast_rate_within_the_linger() -> None:
    # 18 characters in 1.2 s (15 cps): read at 12 cps needs 1.5 s, which the linger covers.
    assert hold.held_end("あ" * 18, F(20), F("21.2"), following=FAR, rate=30) == F("21.7")


def test_a_long_page_spoken_fast_never_passes_the_linger_bound() -> None:
    # 36 characters in 1 s: 3 s at 12 cps, cut at 5/6 s after its speech.
    assert hold.held_end("あ" * 36, F(0), F(1), following=FAR, rate=30) == 1 + F(5, 6)


def test_long_silence_never_keeps_stale_text() -> None:
    # Already past the 7 s maximum: not held at all, however long the silence after it.
    assert hold.held_end("あ" * 40, F(0), F(8), following=F(60), rate=30) == F(8)


def test_back_to_back_captions_keep_their_ends() -> None:
    assert hold.held_end("こんにちは", F(1), F(2), following=F(2), rate=30) == F(2)


def test_the_next_caption_preempts_two_frames_before_its_onset() -> None:
    assert hold.held_end("え、", F(0), F("0.3"), following=F("0.6"), rate=30) == F(16, 30)


def test_a_short_remaining_gap_is_chained_to_two_frames() -> None:
    # Held to 2.5 s, 0.3 s before the next caption: closed to two frames instead.
    end = hold.held_end("ゆっくり話してる", F(0), F(2), following=F("2.8"), rate=30)
    assert end == F("2.8") - F(2, 30)


def test_held_frames_follow_the_next_caption_and_the_limit() -> None:
    captions = [("え、", 0, 9), ("はい", 18, 30), ("うん", 300, 315)]
    # 16: two frames before the next onset; 45: half a second past its speech; 318: two frames
    # before the limit.
    assert hold.held_frames(captions, rate=30, limit=320) == [16, 45, 318]


def test_held_frames_never_end_earlier() -> None:
    assert hold.held_frames([("あい", 0, 30), ("う", 30, 45)], rate=30, limit=45) == [30, 45]


def test_spaces_and_line_breaks_are_not_read() -> None:
    assert hold.characters("あい\nう え") == 4
