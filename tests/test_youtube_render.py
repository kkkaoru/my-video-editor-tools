import json
import subprocess
from collections.abc import Callable
from pathlib import Path

import pytest

from replay import caption_plan
from replay.core import ReplayError
from replay.youtube_render import load_config, main, render, video_filter


def write_config(tmp_path: Path, **overrides: object) -> Path:
    base = tmp_path / "edits/youtube-abcdefghijk"
    (base / "inputs").mkdir(parents=True)
    (base / "inputs/cues.csv").write_text(
        "start,end,speaker,text\n1,2,left,左です\n1.5,3,middle,中です\n", encoding="utf-8"
    )
    (base / "inputs/cut-frames.json").write_text("[[10, 20], [30, 30]]\n", encoding="utf-8")
    publication = tmp_path / "publication.json"
    publication.write_text(json.dumps({"id": "abcdefghijk", "timestamp": 1790000000}))
    (tmp_path / "out").mkdir()
    config = {
        "version": 1,
        "video_id": "abcdefghijk",
        "source_video": str(tmp_path / "original.mkv"),
        "source_audio": str(tmp_path / "kept.wav"),
        "cut_frames": "inputs/cut-frames.json",
        "cues": "inputs/cues.csv",
        "publication": str(publication),
        "output_root": str(tmp_path / "out"),
        "ffmpeg": "/bin/ffmpeg",
        "ffprobe": "/bin/ffprobe",
        **overrides,
    }
    path = base / "render-config.json"
    path.write_text(json.dumps(config), encoding="utf-8")
    return path


@pytest.fixture(autouse=True)
def delivery_canvas(monkeypatch: pytest.MonkeyPatch) -> None:
    """The rendered movie's canvas, rate and frames, as the caption plan probes them."""
    monkeypatch.setattr(caption_plan, "probed", lambda _movie, _ffprobe: (1920, 1080, 30, 90))


def fake_run(
    streams: list[str], commands: list[list[str]]
) -> Callable[..., subprocess.CompletedProcess[str]]:
    def run(command: list[str], **_kwargs: object) -> subprocess.CompletedProcess[str]:
        commands.append(command)
        if command[0] == "/bin/ffprobe":
            body = json.dumps({"streams": [{"codec_type": kind} for kind in streams]})
            return subprocess.CompletedProcess(command, 0, body, "")
        if "-filter_complex" in command:
            Path(command[-1]).write_bytes(b"movie")
        return subprocess.CompletedProcess(command, 0, "", "")

    return run


def test_several_speakers_get_a_caption_plan_instead_of_a_burn(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    commands: list[list[str]] = []
    monkeypatch.setattr(subprocess, "run", fake_run(["video", "audio"], commands))
    output = render(load_config(write_config(tmp_path)))
    stem = "2026-09-21_23-13-20_JST_youtube-abcdefghijk-review"
    assert output == tmp_path / "out" / stem
    assert (output / f"{stem}.mov").read_bytes() == b"movie"
    assert "[左] 左です" in (output / f"{stem}.srt").read_text(encoding="utf-8")
    assert not (output / f"{stem}.ass").exists()
    plan = json.loads((output / f"{stem}.captions.json").read_text(encoding="utf-8"))
    assert (plan["version"], plan["burned"], plan["movie"]) == (2, False, f"{stem}.mov")
    assert [title["text"] for title in plan["titles"]] == ["左です", "中です"]
    assert json.loads((output / f"{stem}.srt.speakers.json").read_text(encoding="utf-8")) == {
        "speakers": {"left": {"label": "左", "color": "#C48CFF"},
                     "middle": {"label": "中央", "color": "#7CDC8C"}}
    }  # fmt: skip
    encode = commands[0]
    assert "ass=" not in encode[encode.index("-filter_complex") + 1]
    assert encode[encode.index("-map") + 3] == "1:a:0"
    assert "-sn" in encode
    assert encode[encode.index("-c:a") + 1] == "copy"
    assert "mov_text" not in encode
    report = json.loads((output / "render.json").read_text(encoding="utf-8"))
    assert report["cut_spans"] == 2
    assert report["cues"] == 2
    assert report["captions"] == "a version-2 caption plan beside the movie, not burned"
    with pytest.raises(FileExistsError, match="--replace"):
        render(load_config(tmp_path / "edits/youtube-abcdefghijk/render-config.json"))
    assert sorted(path.name for path in (tmp_path / "out").iterdir()) == [stem]


def test_replace_keeps_the_old_delivery_until_the_new_render_is_verified(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    commands: list[list[str]] = []
    monkeypatch.setattr(subprocess, "run", fake_run(["video", "audio"], commands))
    config = load_config(write_config(tmp_path))
    output = render(config)
    stem = "2026-09-21_23-13-20_JST_youtube-abcdefghijk-review"
    (output / f"{stem}.mov").write_bytes(b"old movie")
    monkeypatch.setattr(subprocess, "run", fake_run(["video", "audio", "subtitle"], commands))
    with pytest.raises(ReplayError, match="exactly one video and one audio"):
        render(config, replace=True)
    assert (output / f"{stem}.mov").read_bytes() == b"old movie"
    assert sorted(path.name for path in (tmp_path / "out").iterdir()) == [
        f".{stem}.rendering",
        stem,
    ]
    (tmp_path / "out" / f".{stem}.rendering").rename(tmp_path / "failed-render")
    monkeypatch.setattr(subprocess, "run", fake_run(["video", "audio"], commands))
    assert render(config, replace=True) == output
    assert (output / f"{stem}.mov").read_bytes() == b"movie"
    assert sorted(path.name for path in (tmp_path / "out").iterdir()) == [stem]


def test_render_without_separate_audio_uses_source_audio_in_mp4(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    commands: list[list[str]] = []
    monkeypatch.setattr(subprocess, "run", fake_run(["audio", "video"], commands))
    output = render(load_config(write_config(tmp_path, source_audio=None)))
    assert (output / f"{output.name}.mp4").is_file()
    encode = commands[0]
    assert encode.count("-i") == 1
    assert "0:a:0" in encode
    assert encode[encode.index("-c:a") + 1] == "aac"


def test_render_refuses_an_embedded_subtitle_stream(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    commands: list[list[str]] = []
    monkeypatch.setattr(subprocess, "run", fake_run(["video", "audio", "subtitle"], commands))
    with pytest.raises(ReplayError, match="exactly one video and one audio"):
        render(load_config(write_config(tmp_path)))


def test_one_speaker_is_still_burned_once_from_ass(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    commands: list[list[str]] = []
    monkeypatch.setattr(subprocess, "run", fake_run(["video", "audio"], commands))
    config = write_config(tmp_path)
    (config.parent / "inputs/cues.csv").write_text(
        "start,end,speaker,text\n1,2,left,左です\n", encoding="utf-8"
    )
    output = render(load_config(config))
    assert (output / f"{output.name}.ass").is_file()
    assert not (output / f"{output.name}.captions.json").exists()
    assert "ass=filename=" in commands[0][commands[0].index("-filter_complex") + 1]
    report = json.loads((output / "render.json").read_text(encoding="utf-8"))
    assert report["captions"] == "burned once from ASS; no embedded subtitle track"


def test_video_filter_cuts_then_burns_once() -> None:
    assert video_filter((), None) == "[0:v:0]null[v]"
    assert video_filter(((1, 2),), None) == (
        "[0:v:0]select='not(between(n,1,2))',setpts=N/(30*TB)[v]"
    )
    assert video_filter((), Path("/a/b.ass")) == "[0:v:0]ass=filename='/a/b.ass'[v]"
    assert video_filter(((1, 2), (5, 9)), Path("/a/x:y.ass")) == (
        "[0:v:0]select='not(between(n,1,2)+between(n,5,9))',setpts=N/(30*TB),"
        "ass=filename='/a/x\\:y.ass'[v]"
    )


@pytest.mark.parametrize(
    ("overrides", "message"),
    [
        ({"extra": 1}, "unknown, missing"),
        ({"version": 2}, "unknown, missing"),
        ({"video_id": ""}, "video ID"),
        ({"source_video": "relative.mkv"}, "absolute path"),
        ({"source_video": "/v/youtube-edits/x-edit-1.mp4"}, "original, not an edited"),
    ],
)
def test_config_rejects_invalid_or_edited_sources(
    tmp_path: Path, overrides: dict[str, object], message: str
) -> None:
    with pytest.raises(ReplayError, match=message):
        load_config(write_config(tmp_path, **overrides))


@pytest.mark.parametrize(
    "cuts",
    ['{"a": 1}', "[[5, 3]]", "[[1, 2], [2, 3]]", '[["1", 2]]', "[[1]]"],
)
def test_cut_frames_must_be_ordered_disjoint_pairs(tmp_path: Path, cuts: str) -> None:
    path = write_config(tmp_path)
    (path.parent / "inputs/cut-frames.json").write_text(cuts, encoding="utf-8")
    with pytest.raises(ReplayError, match="Cut frames"):
        load_config(path)


def test_cli_prints_the_new_directory(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    commands: list[list[str]] = []
    monkeypatch.setattr(subprocess, "run", fake_run(["video", "audio"], commands))
    path = str(write_config(tmp_path))
    assert main([path]) == 0
    assert capsys.readouterr().out.strip().endswith("_JST_youtube-abcdefghijk-review")
    assert main([path, "--replace"]) == 0
    assert capsys.readouterr().out.strip().endswith("_JST_youtube-abcdefghijk-review")


def test_a_configured_speaker_table_names_the_lanes(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    commands: list[list[str]] = []
    monkeypatch.setattr(subprocess, "run", fake_run(["video", "audio"], commands))
    table = [{"id": "left", "name": "あい", "color": "#112233"}, {"id": "middle", "name": "うえ"}]
    output = render(load_config(write_config(tmp_path, speakers=table)))
    names = json.loads((output / f"{output.name}.srt.speakers.json").read_text(encoding="utf-8"))
    assert names == {"speakers": {"left": {"label": "あい", "color": "#112233"},
                                  "middle": {"label": "うえ", "color": "#7CDC8C"}}}  # fmt: skip


def test_a_bad_speaker_table_is_refused_before_rendering(tmp_path: Path) -> None:
    with pytest.raises(ReplayError, match="A speaker has id"):
        load_config(write_config(tmp_path, speakers=[{"id": "far", "name": "x"}]))
