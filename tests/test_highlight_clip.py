"""A highlight clip joins chosen ranges of a delivery without cutting a caption mid-line."""

import json
import subprocess
from pathlib import Path

import pytest

from replay import highlight_clip as clip
from replay.core import ReplayError

FFMPEG = "ffmpeg"
FFPROBE = "ffprobe"
CUES = [(1.0, 3.0, ("a",)), (4.0, 10.0, ("b",)), (12.0, 13.0, ("c",))]


def test_times_and_ranges_read_minutes_and_seconds() -> None:
    assert clip.seconds("1:02") == 62.0
    assert clip.seconds("7.5") == 7.5
    assert clip.span("0:51-1:02") == (51.0, 62.0)
    with pytest.raises(ReplayError, match="Unreadable time"):
        clip.seconds("x:10")
    with pytest.raises(ReplayError, match="START before END"):
        clip.span("1:02-0:51")
    with pytest.raises(ReplayError, match="START before END"):
        clip.span("12")


def test_edges_inside_a_caption_move_out_to_keep_it_whole() -> None:
    assert clip.snapped(2.0, CUES, start=True) == 1.0
    assert clip.snapped(2.0, CUES, start=False) == 3.0
    assert clip.snapped(3.5, CUES, start=True) == 3.5
    assert clip.snapped(8.0, CUES, start=True) == 10.0
    assert clip.snapped(5.0, CUES, start=False) == 4.0
    assert clip.adjusted([(2.0, 3.5), (12.5, 20.0)], CUES, 15.0) == [(1.0, 3.5), (12.0, 15.0)]
    with pytest.raises(ReplayError, match="overlap"):
        clip.adjusted([(2.0, 8.0), (5.0, 11.0)], CUES, 15.0)


def test_joined_cues_follow_each_range_in_order() -> None:
    assert clip.joined_cues(CUES, [(1.0, 3.5), (12.0, 15.0)]) == [
        (0.0, 2.0, ("a",)),
        (2.5, 3.5, ("c",)),
    ]


def test_the_command_trims_fades_and_concatenates() -> None:
    command = clip.command(Path("/m.mp4"), [(1.0, 2.0), (5.0, 7.0)], Path("/o.mp4"), ffmpeg=FFMPEG)
    assert command[command.index("-filter_complex") + 1] == (
        "[0:v]trim=1.000:2.000,setpts=PTS-STARTPTS[v0];"
        "[0:a]atrim=1.000:2.000,asetpts=PTS-STARTPTS,"
        "afade=t=in:d=0.015,afade=t=out:st=0.985:d=0.015[a0];"
        "[0:v]trim=5.000:7.000,setpts=PTS-STARTPTS[v1];"
        "[0:a]atrim=5.000:7.000,asetpts=PTS-STARTPTS,"
        "afade=t=in:d=0.015,afade=t=out:st=1.985:d=0.015[a1];"
        "[v0][a0][v1][a1]concat=n=2:v=1:a=1[v][a]"
    )


def delivery(tmp_path: Path) -> Path:
    movie = tmp_path / "lib" / "rec" / "delivery" / "S_JST_rec-review.mp4"
    movie.parent.mkdir(parents=True)
    subprocess.run(
        [FFMPEG, "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=108x192:rate=10:d=15",
         "-f", "lavfi", "-i", "sine=d=15", "-c:v", "libx264", "-c:a", "aac", "-shortest",
         str(movie)],
        check=True,
    )  # fmt: skip
    movie.with_suffix(".srt").write_text(
        "1\n00:00:01,000 --> 00:00:03,000\na\n\n2\n00:00:12,000 --> 00:00:13,000\nc\n",
        encoding="utf-8",
    )
    return movie


def test_a_clip_is_its_own_recording_folder_with_transcript_and_record(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    movie = delivery(tmp_path)
    assert clip.main([str(movie), "clip1", "0:02-0:03.5", "12.5-14"]) == 0
    folder = tmp_path / "lib" / "rec-clip1"
    output = folder / "delivery" / "S_JST_rec-clip1-review.mp4"
    assert capsys.readouterr().out == f"{output}\n"
    assert clip.duration(output, FFPROBE) == pytest.approx(4.5, abs=0.1)
    assert output.with_suffix(".srt").read_text(encoding="utf-8") == (
        "1\n00:00:00,000 --> 00:00:02,000\na\n\n2\n00:00:02,500 --> 00:00:03,500\nc\n"
    )
    assert json.loads((folder / "highlight.json").read_text(encoding="utf-8")) == {
        "version": 1,
        "source": str(movie),
        "requested": [[2.0, 3.5], [12.5, 14.0]],
        "adjusted": [[1.0, 3.5], [12.0, 14.0]],
    }
    with pytest.raises(ReplayError, match="choose another name"):
        clip.make(movie, "clip1", [(2.0, 3.0)], ffmpeg=FFMPEG, ffprobe=FFPROBE)


def test_a_movie_outside_a_delivery_folder_or_a_bad_name_is_refused(tmp_path: Path) -> None:
    movie = delivery(tmp_path)
    with pytest.raises(ReplayError, match="delivery movie"):
        clip.make(movie, "a/b", [(2.0, 3.0)], ffmpeg=FFMPEG, ffprobe=FFPROBE)
    with pytest.raises(ReplayError, match="delivery movie"):
        clip.make(tmp_path / "x.mp4", "c", [(2.0, 3.0)], ffmpeg=FFMPEG, ffprobe=FFPROBE)
    with pytest.raises(SystemExit, match="Usage"):
        clip.main(["only"])
