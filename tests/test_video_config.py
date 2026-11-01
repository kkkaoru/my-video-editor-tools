"""One per-video config drives the pipeline's per-video settings; without it nothing changes."""

import json
from pathlib import Path

import pytest

from replay import (
    caption_layout,
    caption_palette,
    source_comment_clean,
    source_vertical,
    video_config,
)
from replay import recording_caption_steps as captions
from replay import recording_steps as steps
from replay.core import ReplayError

ICON = {"name": "speaker", "icon": "/abs/icon.png", "iconSha256": "a" * 64}
LAYOUT = {"canvas": [1920, 1080], "align": "left", "originX": 780, "regionTop": 560,
          "regionBottom": 1056, "regionRight": 1880}  # fmt: skip


def write(root: Path, value: object) -> Path:
    root.mkdir(parents=True, exist_ok=True)
    (root / "video-config.json").write_text(json.dumps(value), encoding="utf-8")
    return root


def test_without_the_file_every_reader_keeps_its_side_file_or_default(tmp_path: Path) -> None:
    assert video_config.load(tmp_path) is None
    assert video_config.value(tmp_path, "clips", True) is True
    assert caption_palette.intent(tmp_path) is None
    assert source_vertical.intent(tmp_path) is None
    assert source_comment_clean.intent(tmp_path) is False
    assert caption_layout.captions_settings({"version": 1}, tmp_path) == {"version": 1}


def test_a_full_config_is_accepted_and_printed(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    value = {
        "version": 1, "orientation": {"aspect": [9, 16], "follow": "character"},
        "speakers": {"count": 1, **ICON},
        "captions": {"palette": {"gradientTop": "#B6DF9E", "gradientBottom": "#6DBE3C"},
                     "layout": {**LAYOUT, "nameplate": {"font": "/f.ttf", "fontSha256": "b" * 64,
                                                        "iconX": 1, "iconY": 2, "iconSize": 3,
                                                        "nameSize": 3}}},
        "audio": {"delivery": "original-mix", "intent": "kept",
                  "loudness": {"targetLufs": -16, "truePeakDbtp": -1}},
        "cut": {"nonSpeech": True}, "cleanup": {"sourceCaptions": True, "commentBoxes": True},
        "clips": False, "channelLabel": "label", "asrGlossary": ["/g.json"],
    }  # fmt: skip
    root = write(tmp_path / "edit", value)
    assert video_config.main([str(root)]) == 0
    assert json.loads(capsys.readouterr().out) == value
    assert caption_palette.intent(root) == {"gradientTop": "#B6DF9E", "gradientBottom": "#6DBE3C"}
    assert source_vertical.intent(root) == (9, 16)
    assert source_comment_clean.intent(root) is True
    assert video_config.caption_layout(root) == (True, {
        "version": 1, **LAYOUT,
        "speaker": {**ICON, "font": "/f.ttf", "fontSha256": "b" * 64, "iconX": 1, "iconY": 2,
                    "iconSize": 3, "nameSize": 3},
    })  # fmt: skip
    assert video_config.delivery_audio(root) == (
        True, {"version": 1, "source": "original-mix", "intent": "kept"}
    )  # fmt: skip
    assert video_config.value(root, "clips", True) is False
    with pytest.raises(SystemExit, match="Usage"):
        video_config.main([])


def test_the_config_reaches_captions_json_and_keeps_the_voice_by_default(tmp_path: Path) -> None:
    root = write(tmp_path / "edit", {
        "version": 1, "captions": {"layout": LAYOUT},
        "audio": {"delivery": "vocals", "loudness": {"targetLufs": -16, "truePeakDbtp": -2}},
    })  # fmt: skip
    assert caption_layout.captions_settings({"version": 1}, root) == {
        "version": 1, "layout": {"version": 1, **LAYOUT},
        "voiceLoudness": {"targetLufs": -16, "truePeakDbtp": -2},
        "voiceRepair": True, "voiceDeesser": True, "voiceJudge": True,
    }  # fmt: skip
    assert (
        source_vertical.intent(write(tmp_path / "e2", {"version": 1, "orientation": "source"}))
        is None
    )


def test_a_setting_held_by_both_the_config_and_its_side_file_is_refused(tmp_path: Path) -> None:
    root = write(tmp_path / "edit", {"version": 1, "orientation": "source"})
    (root / "reframe.json").write_text("{}", encoding="utf-8")
    with pytest.raises(ReplayError, match="orientation is set by both"):
        source_vertical.intent(root)


@pytest.mark.parametrize(
    ("value", "message"),
    [
        ([], "object with version 1"),
        ({"version": 1, "colour": 1}, "unknown keys colour"),
        ({"version": 1, "orientation": "portrait"}, "orientation is"),
        ({"version": 1, "speakers": {"count": 3}}, "multi-speaker"),
        ({"version": 1, "speakers": {"icon": "/i"}}, "icon needs the speaker's name"),
        ({"version": 1, "speakers": {"bad": 1}}, "speakers takes only"),
        ({"version": 1, "captions": {"layout": {"nameplate": {}}}}, "nameplate needs"),
        ({"version": 1, "audio": {"delivery": "music"}}, "vocals or original-mix"),
        ({"version": 1, "audio": {"delivery": "original-mix"}}, "why the music is kept"),
        ({"version": 1, "audio": {"loudness": {"targetLufs": -18}}}, "numeric targetLufs"),
        ({"version": 1, "audio": {"deesser": "yes"}}, "audio.deesser must be true or false"),
        ({"version": 1, "audio": {"judge": 1}}, "audio.judge must be true or false"),
        ({"version": 1, "audio": {"repair": "on"}}, "audio.repair must be true or false"),
        ({"version": 1, "audio": {"restore": True}}, "audio takes only"),
        ({"version": 1, "cut": {"nonSpeech": False}}, "cannot turn it off"),
        ({"version": 1, "cleanup": {"sourceCaptions": False}}, "cannot turn it off"),
        ({"version": 1, "cleanup": {"commentBoxes": "yes"}}, "true or false"),
        ({"version": 1, "clips": 0}, "clips must be true or false"),
        ({"version": 1, "channelLabel": " "}, "channelLabel is a non-empty string"),
        ({"version": 1, "asrGlossary": ["relative.json"]}, "absolute paths"),
        ({"version": 1, "verticals": {"inset": [1, 2, 3]}}, "needs inset"),
        ({"version": 1, "verticals": {"inset": [0, 0, 1, 9], "ranges": [[0, 1]]}}, "needs inset"),
        ({"version": 1, "verticals": {"inset": [0, 0, 9, 9], "ranges": []}}, "start, end"),
        ({"version": 1, "verticals": {"inset": [0, 0, 9, 9], "ranges": [[5, 2]]}}, "start, end"),
    ],
)
def test_malformed_configs_name_the_offending_key(value: object, message: str) -> None:
    with pytest.raises(ReplayError, match=message):
        video_config.validated(value)


def channel_pipeline(tmp_path: Path, config: dict[str, object]) -> steps.Pipeline:
    original = tmp_path / "original"
    original.mkdir(parents=True)
    (original / "channel.json").write_text('{"channel_id": "UC1"}', encoding="utf-8")
    root = write(tmp_path / "edit", config)
    (root / "edit-config.json").write_text("{}", encoding="utf-8")
    return steps.Pipeline(tmp_path / "pipeline.json", {"editRoot": str(root)})


def test_the_channel_label_is_set_once_from_the_config(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    pipeline = channel_pipeline(tmp_path, {"version": 1, "channelLabel": "かしすみるく"})
    monkeypatch.setattr(
        captions, "load_config", lambda _root: type("C", (), {"source": tmp_path / "original/x"})
    )
    calls: list[list[str]] = []

    def request(action: list[str]) -> dict[str, object]:
        calls.append(action)
        return {"version": 1, "channels": {"UC1": {"label": "old"}}}

    assert captions.channel_label(pipeline, request=request) == {
        "channelLabel": {"channelId": "UC1", "label": "かしすみるく"}
    }
    assert calls == [["list"], ["set", "UC1", "かしすみるく"]]
    assert (
        captions.channel_label(
            channel_pipeline(tmp_path / "plain", {"version": 1}), request=request
        )
        == {}
    )


def test_the_judge_reaches_captions_json_only_when_enabled(tmp_path: Path) -> None:
    on = write(tmp_path / "on", {"version": 1, "audio": {"judge": True}})
    off = write(
        tmp_path / "off",
        {"version": 1, "audio": {"judge": False, "deesser": False, "repair": False}},
    )
    assert caption_layout.captions_settings({"version": 1}, on) == {
        "version": 1, "voiceJudge": True, "voiceDeesser": True, "voiceRepair": True,
    }  # fmt: skip
    assert caption_layout.captions_settings({"version": 1}, off) == {"version": 1}


def test_computation_only_voice_correction_is_the_config_default(tmp_path: Path) -> None:
    plain = write(tmp_path / "plain", {"version": 1})
    assert caption_layout.captions_settings({"version": 1}, plain) == {
        "version": 1, "voiceRepair": True, "voiceDeesser": True, "voiceJudge": True,
    }  # fmt: skip


def test_the_deesser_reaches_captions_json_only_when_enabled(tmp_path: Path) -> None:
    on = write(tmp_path / "on", {"version": 1, "audio": {"deesser": True}})
    off = write(
        tmp_path / "off",
        {"version": 1, "audio": {"deesser": False, "judge": False, "repair": False}},
    )
    assert caption_layout.captions_settings({"version": 1}, on) == {
        "version": 1, "voiceDeesser": True, "voiceJudge": True, "voiceRepair": True,
    }  # fmt: skip
    assert caption_layout.captions_settings({"version": 1}, off) == {"version": 1}


def test_verticals_give_the_inset_and_float_ranges() -> None:
    assert video_config.verticals({"inset": [1356, 680, 312, 324], "ranges": [[238, 840.5]]}) == (
        (1356, 680, 312, 324),
        [(238.0, 840.5)],
    )
