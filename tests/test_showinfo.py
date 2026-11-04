"""The showinfo PTS lines a decode is checked by."""

from replay.showinfo import SHOWN_PTS


def test_a_shown_line_gives_its_pts() -> None:
    log = b"[Parsed_showinfo_0 @ 0x1] n:   0 pts:      0 pts_time:0\nn:1 pts:-512 pts_time:0\n"
    assert SHOWN_PTS.findall(log) == [b"0", b"-512"]


def test_a_line_after_a_warning_without_a_newline_still_counts() -> None:
    log = b"[h264 @ 0x9] Late SEIn:116962 pts:59884544 pts_time:3898.733333 duration: 512\n"
    assert SHOWN_PTS.findall(log) == [b"59884544"]


def test_another_word_ending_in_n_is_not_a_shown_line() -> None:
    assert SHOWN_PTS.findall(b"min:12 pts:34 \nscan_n:5 pts:6 \nx1n:7 pts:8 \n") == []
