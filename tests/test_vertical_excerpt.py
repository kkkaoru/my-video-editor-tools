"""9:16 excerpts: the canvas geometry, the window's presence, and the sibling caption pipeline."""

import json
import subprocess
from pathlib import Path

import numpy as np
import pytest

from replay import vertical_excerpt as vertical
from replay.core import ReplayError
from replay.source_lead_in import video_pts

FFMPEG = "ffmpeg"
FFPROBE = "ffprobe"


def test_the_centre_and_window_land_on_the_vertical_canvas() -> None:
    shape = vertical.geometry((1920, 1080), (1356, 680, 312, 324))
    assert shape == vertical.Geometry(
        (608, 1080, 656), (1356, 680, 312, 324), (264, 1296, 554, 576)
    )
    assert shape.layout() == {
        "version": 1, "canvas": [1080, 1920], "align": "centre", "originX": 0,
        "regionTop": 120, "regionBottom": 1272, "regionRight": 1080,
    }  # fmt: skip


def test_a_window_outside_the_frame_or_too_large_is_refused() -> None:
    with pytest.raises(ReplayError, match="outside the source frame"):
        vertical.geometry((1920, 1080), (1800, 680, 312, 324))
    with pytest.raises(ReplayError, match="too large"):
        vertical.geometry((1920, 1080), (0, 0, 600, 600))


def test_presence_needs_a_sustained_resemblance_and_runs_are_half_open() -> None:
    flags = vertical.presence([0.9] * 12 + [0.0] + [0.9] * 3 + [0.0] * 10)
    assert flags == [True] * 15 + [False] * 11
    assert vertical.spans([False, True, True, False, True]) == [(1, 3), (4, 5)]
    assert vertical.enable([(1, 3), (4, 5)], 0.5) == "between(t,1.500,3.500)+between(t,4.500,5.500)"
    assert vertical.enable([], 0.0) == "0"


def test_the_compose_command_crops_scales_and_overlays_while_present() -> None:
    shape = vertical.geometry((1920, 1080), (1356, 680, 312, 324))
    stream: dict[str, object] = {
        "time_base": "1/15360",
        "color_space": "bt709",
        "color_range": "unknown",
    }
    command = vertical.compose_command(
        Path("/s.mp4"), Path("/o.mp4"), shape, "between(t,0.000,1.000)", stream, ffmpeg=FFMPEG
    )
    assert command[command.index("-filter_complex") + 1] == (
        "[0:v]split=2[a][b];[a]crop=608:1080:656:0,scale=1080:1920:flags=lanczos[bg];"
        "[b]crop=312:324:1356:680,scale=554:576:flags=lanczos[in];"
        "[bg][in]overlay=264:1296:enable='between(t,0.000,1.000)'[v]"
    )
    assert command[command.index("-enc_time_base") + 1] == "1/15360"
    assert command[command.index("-colorspace") + 1] == "bt709"
    assert "-color_range" not in command
    with pytest.raises(ReplayError, match="quote"):
        vertical.compose_command(Path("/s"), Path("/o"), shape, "'", stream, ffmpeg=FFMPEG)


def movie(path: Path, seconds: int, *, size: str = "320x180") -> Path:
    subprocess.run(
        [FFMPEG, "-v", "error", "-f", "lavfi", "-i", f"testsrc2=size={size}:rate=10:d={seconds}",
         "-f", "lavfi", "-i", f"sine=d={seconds}", "-c:v", "libx264", "-c:a", "aac",
         "-shortest", str(path)],
        check=True,
    )  # fmt: skip
    return path


def test_the_composed_picture_keeps_every_frame_and_is_reused(tmp_path: Path) -> None:
    source = movie(tmp_path / "source.mp4", 3)
    out, shape = vertical.composed(
        source, tmp_path / "v", (200, 100, 60, 60), ffmpeg=FFMPEG, ffprobe=FFPROBE
    )
    assert shape.crop == (102, 180, 108)
    assert video_pts(out, FFPROBE) == video_pts(source, FFPROBE)
    recorded = json.loads((tmp_path / "v" / "vertical.json").read_text(encoding="utf-8"))
    assert recorded["insetSeconds"] == [[0, 3]]
    stamp = out.stat().st_mtime_ns
    assert vertical.composed(
        source, tmp_path / "v", (200, 100, 60, 60), ffmpeg=FFMPEG, ffprobe=FFPROBE
    ) == (out, shape)
    assert out.stat().st_mtime_ns == stamp


def test_a_source_without_frames_has_no_window(tmp_path: Path) -> None:
    empty = tmp_path / "empty.mp4"
    subprocess.run([FFMPEG, "-v", "error", "-f", "lavfi", "-i", "sine=d=1", str(empty)], check=True)
    with pytest.raises((ReplayError, subprocess.CalledProcessError)):
        vertical.correlations(empty, (0, 0, 8, 8), FFMPEG)


def test_the_sibling_pipeline_reuses_the_recording_and_replaces_the_layout(
    tmp_path: Path,
) -> None:
    work, steps = tmp_path / "rec" / "work", tmp_path / "rec" / "steps"
    for name in vertical.LINKED:
        (work / name).parent.mkdir(parents=True, exist_ok=True)
    (work / "cut-plan-v1").mkdir(parents=True, exist_ok=True)
    (work / "cut-plan-v1" / "supported-cuts.json").write_text("{}", encoding="utf-8")
    (work / "paging-v1/prepared/paging").mkdir(parents=True, exist_ok=True)
    (work / "paging-v1/prepared/paging/reflow.json").write_text("{}", encoding="utf-8")
    for name in vertical.LINKED[2:]:
        (work / name).write_text(name, encoding="utf-8")
    steps.mkdir(parents=True)
    for step in vertical.UPSTREAM:
        (steps / f"{step}.json").write_text(json.dumps({"outputs": {"a": 1}}), encoding="utf-8")
    pipeline = tmp_path / "rec" / "pipeline.json"
    pipeline.write_text(json.dumps({"work": str(work), "steps": str(steps)}), encoding="utf-8")
    (tmp_path / "rec" / "captions.json").write_text(
        json.dumps({"mediaRoot": "/m/recordings", "voiceLoudness": {}, "layout": {}}),
        encoding="utf-8",
    )
    made = vertical.sibling(pipeline, Path("/pic.mp4"), {"version": 1})
    root = tmp_path / "rec" / "vertical-v1"
    assert made == root / "pipeline.json"
    assert json.loads(made.read_text(encoding="utf-8")) == {
        "work": str(root / "work"),
        "steps": str(root / "steps"),
    }
    assert json.loads((root / "captions.json").read_text(encoding="utf-8")) == {
        "mediaRoot": "/m/verticals",
        "layout": {"version": 1},
    }
    assert json.loads((root / "steps" / "cleanup.json").read_text(encoding="utf-8")) == {
        "outputs": {"a": 1, "cleanSource": "/pic.mp4"}
    }
    assert (root / "work" / "video-clock.json").stat().st_ino == (
        work / "video-clock.json"
    ).stat().st_ino
    assert (root / "work" / "cut-plan-v1" / "supported-cuts.json").read_text("utf-8") == "{}"
    assert vertical.sibling(pipeline, Path("/pic.mp4"), {"version": 1}) == made


def test_an_excerpt_takes_the_vertical_picture_and_the_delivery_audio(tmp_path: Path) -> None:
    picture = movie(tmp_path / "v.mp4", 4, size="108x192")
    delivery = movie(tmp_path / "d.mp4", 4)
    output = tmp_path / "x.mp4"
    vertical.excerpt(picture, delivery, (1.0, 3.0), output, ffmpeg=FFMPEG)
    probe = subprocess.run(
        [FFPROBE, "-v", "error", "-show_entries", "stream=codec_type,width,height",
         "-of", "csv=p=0", str(output)],
        capture_output=True, text=True, check=True,
    ).stdout.split()  # fmt: skip
    assert probe == ["video,108,192", "audio"]
    assert vertical.clock(238) == "0358"
    assert vertical.clock(2382) == "3942"


def test_correlations_rank_the_median_window_highest(tmp_path: Path) -> None:
    source = movie(tmp_path / "s.mp4", 2)
    values = vertical.correlations(source, (0, 0, 64, 64), FFMPEG)
    assert len(values) == 2
    assert np.all(np.asarray(values) > 0.5)


def test_run_refuses_a_recording_without_verticals(tmp_path: Path) -> None:
    edit = tmp_path / "edit"
    edit.mkdir()
    (edit / "video-config.json").write_text('{"version": 1}', encoding="utf-8")
    pipeline = tmp_path / "pipeline.json"
    pipeline.write_text(
        json.dumps({"editRoot": str(edit), "python": "p", "ffmpeg": "f", "ffprobe": "g"}),
        encoding="utf-8",
    )
    with pytest.raises(ReplayError, match="asks for no verticals"):
        vertical.run(pipeline)
    with pytest.raises(SystemExit, match="Usage"):
        vertical.main([])


def test_run_places_the_captions_once_and_cuts_each_range(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    edit, steps = tmp_path / "edit", tmp_path / "rec" / "steps"
    edit.mkdir()
    (edit / "video-config.json").write_text(
        json.dumps({"version": 1, "verticals": {"inset": [8, 8, 8, 8], "ranges": [[238, 840]]}}),
        encoding="utf-8",
    )
    steps.mkdir(parents=True)
    delivery = tmp_path / "d" / "rec" / "revisions" / "p" / "delivery" / "S_JST_rec-review.mp4"
    delivery.parent.mkdir(parents=True)
    delivery.with_suffix(".srt").write_text(
        "1\n00:03:50,000 --> 00:04:00,000\nfirst\n\n2\n00:20:00,000 --> 00:20:01,000\nlate\n",
        encoding="utf-8",
    )
    (steps / "publish.json").write_text(
        json.dumps({"outputs": {"delivery": str(delivery)}}), encoding="utf-8"
    )
    pipeline = tmp_path / "rec" / "pipeline.json"
    pipeline.write_text(
        json.dumps({"editRoot": str(edit), "python": "py", "ffmpeg": "f", "ffprobe": "g",
                    "steps": str(steps), "source": "/s.mp4"}),
        encoding="utf-8",
    )  # fmt: skip
    (tmp_path / "rec" / "captions.json").write_text('{"mediaRoot": "/m/r"}', encoding="utf-8")
    sibling_root = tmp_path / "rec" / "vertical-v1"
    (sibling_root / "steps").mkdir(parents=True)
    (sibling_root / "steps" / "placement.json").write_text("{}", encoding="utf-8")
    (sibling_root / "steps" / "finish.json").write_text(
        json.dumps({"outputs": {"movie": {"path": "/final.mp4"}}}), encoding="utf-8"
    )
    shape = vertical.geometry((1920, 1080), (1356, 680, 312, 324))
    calls: list[object] = []
    monkeypatch.setattr(vertical, "composed", lambda *a, **k: (Path("/pic.mp4"), shape))
    monkeypatch.setattr(vertical, "sibling", lambda *a: sibling_root / "pipeline.json")
    monkeypatch.setattr(subprocess, "run", lambda command, **k: calls.append(command[4]))
    monkeypatch.setattr(vertical, "excerpt", lambda *a, **k: calls.append(a[2]))
    assert vertical.main([str(pipeline)]) == 0
    assert calls == ["caption-parts", "compose", (238.0, 840.0)]
    made = tmp_path / "d" / "rec-vertical-0358-1400" / "delivery"
    assert capsys.readouterr().out == f"{made / 'S_JST_rec-vertical-0358-1400-review.mp4'}\n"
    assert (made / "S_JST_rec-vertical-0358-1400-review.srt").read_text(encoding="utf-8") == (
        "1\n00:00:00,000 --> 00:00:02,000\nfirst\n"
    )


def test_a_transcript_range_keeps_clipped_cues_timed_from_the_range_start() -> None:
    text = (
        "1\n00:00:03,266 --> 00:00:03,866\nhello\n\n"
        "2\n00:00:04,866 --> 00:00:08,533\nline one\nline two\n\n"
        "3\n01:00:00,000 --> 01:00:01,000\nlater\n"
    )
    assert vertical.transcript_range(text, (3.5, 6.0)) == (
        "1\n00:00:00,000 --> 00:00:00,366\nhello\n\n"
        "2\n00:00:01,366 --> 00:00:02,500\nline one\nline two\n"
    )
    assert vertical.srt_stamp(3723.4567) == "01:02:03,457"
    with pytest.raises(ReplayError, match="malformed cue"):
        vertical.transcript_range("1\nno timing\ntext\n", (0.0, 1.0))
    with pytest.raises(ReplayError, match="Malformed transcript time"):
        vertical.transcript_range("1\nx --> y\ntext\n", (0.0, 1.0))
