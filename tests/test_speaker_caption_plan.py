import hashlib
import json
import subprocess
from pathlib import Path

import pytest

from replay import caption_layout_file, caption_plan
from replay import speaker_caption_plan as speakers
from replay.core import ReplayError
from replay.speaker_subtitles import SpeakerCue

FIXTURES = Path(__file__).parent / "fixtures"
VIDEO = {
    "width": 1920, "height": 1080, "r_frame_rate": "30/1", "nb_frames": "300",
    "start_time": "0.000000",
}  # fmt: skip
CUES = (
    SpeakerCue(2.0, 2.74, "middle", "こん"),
    SpeakerCue(2.74, 4.3, "middle", "こんば"),
    SpeakerCue(4.3, 5.6, "middle", "こんばんは"),
    SpeakerCue(3.0, 4.0, "left", "やあ"),
    SpeakerCue(3.5, 4.5, "unassigned", "うん"),
    SpeakerCue(3.6, 4.6, "unassigned", "あ"),
    SpeakerCue(6.01, 6.02, "right", "え"),
    SpeakerCue(7.0, 8.0, "right", "おはようございますみなさん元気ですか"),
)


def ffprobe(monkeypatch: pytest.MonkeyPatch) -> None:
    def run(command: list[str], **_: object) -> subprocess.CompletedProcess[str]:
        return subprocess.CompletedProcess(command, 0, json.dumps({"streams": [VIDEO]}), "")

    monkeypatch.setattr(subprocess, "run", run)


def built(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, table: object = None
) -> tuple[dict[str, object], dict[str, Path], int]:
    ffprobe(monkeypatch)
    used = {"left", "middle", "right"}
    return speakers.build(CUES, table or speakers.default_table(used), tmp_path / "m.mp4",
                          burned=False, ffprobe="probe")  # fmt: skip


def test_the_lane_style_is_the_fixtures_scaled_style_20() -> None:
    fixture = json.loads((FIXTURES / "caption-plan-v2.json").read_text(encoding="utf-8"))
    style = speakers.lane_style(1920)
    assert {key: round(value, 6) if isinstance(value, float) else value
            for key, value in style.items()} == fixture["style"]  # fmt: skip


def test_lane_colours_mix_halfway_to_white() -> None:
    assert [speakers.lighter(c) for c in ("#C48CFF", "#7CDC8C", "#6CD4FF")] == [
        "#E2C6FF", "#BEEEC6", "#B6EAFF"
    ]  # fmt: skip


def test_the_lane_base_is_the_bottom_mode_position() -> None:
    assert speakers.lane_base(1080, 40.0) == pytest.approx(-418.0723276723277, abs=1e-9)


def test_growing_cues_reveal_one_title_per_speaker(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    plan, icons, dropped = built(tmp_path, monkeypatch)
    assert icons == {}
    assert dropped == 1  # "え" shows on no frame
    base = speakers.lane_base(1080, 40.0)
    assert plan["titles"] == [
        {"text": "こんばんは", "startFrame": 60, "endFrame": 168,
         "graphemeFrames": [60, 60, 83, 129, 129], "x": 180.0, "y": base, "speaker": "middle"},
        {"text": "やあ", "startFrame": 90, "endFrame": 120, "graphemeFrames": [90, 90],
         "x": -310.0, "y": base, "speaker": "left"},
        {"text": "うん", "startFrame": 105, "endFrame": 135, "graphemeFrames": [105, 105],
         "x": 0.0, "y": base},
        {"text": "あ", "startFrame": 108, "endFrame": 138, "graphemeFrames": [108], "x": 0.0,
         "y": base},
        {"text": "おはようございますみなさ\nん元気ですか", "startFrame": 210, "endFrame": 240,
         "graphemeFrames": [210] * 19, "x": 690.0, "y": base, "speaker": "right"},
    ]  # fmt: skip
    assert plan["speakers"] == [
        {"id": "left", "name": "左", "colorTop": "#E2C6FF", "colorBottom": "#C48CFF",
         "laneX": -310.0},
        {"id": "middle", "name": "中央", "colorTop": "#BEEEC6", "colorBottom": "#7CDC8C",
         "laneX": 180.0},
        {"id": "right", "name": "右", "colorTop": "#B6EAFF", "colorBottom": "#6CD4FF",
         "laneX": 690.0},
    ]  # fmt: skip
    resolved = caption_layout_file.resolve(plan, {"version": 1, "mode": "plan"}, None)
    assert resolved[3]["y"] == pytest.approx(base + 72.32767232767233 + 12, abs=1e-9)


def test_icons_are_pinned_beside_the_plan_with_the_derived_speakers_json(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    icon = FIXTURES / "synthetic-multi.speaker-left.png"
    table = {"speakers": [
        {"id": "left", "name": "ひだり", "color": "#112233", "icon": str(icon),
         "iconSha256": hashlib.sha256(icon.read_bytes()).hexdigest()},
        {"id": "middle", "name": "まんなか"}, {"id": "right", "name": "みぎ"},
    ]}  # fmt: skip
    plan, icons, _ = built(tmp_path, monkeypatch, table)
    assert icons == {"m.speaker-left.png": icon}
    written = speakers.write(plan, tmp_path / "m.mp4", icons)
    assert [path.name for path in written] == [
        "m.speaker-left.png", "m.captions.json", "m.srt.speakers.json"
    ]  # fmt: skip
    assert (tmp_path / "m.speaker-left.png").read_bytes() == icon.read_bytes()
    assert caption_plan.validate(json.loads((tmp_path / "m.captions.json").read_text("utf-8")))
    assert json.loads((tmp_path / "m.srt.speakers.json").read_text(encoding="utf-8")) == {
        "speakers": {"left": {"label": "ひだり", "color": "#112233"},
                     "middle": {"label": "まんなか", "color": "#7CDC8C"},
                     "right": {"label": "みぎ", "color": "#6CD4FF"}}
    }  # fmt: skip
    assert speakers.write(plan, tmp_path / "m.mp4", icons) == written


def test_an_icon_of_another_digest_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    icon = FIXTURES / "synthetic-multi.speaker-left.png"
    table = {"speakers": [{"id": "left", "name": "a", "icon": str(icon), "iconSha256": "0" * 64},
                          {"id": "middle", "name": "b"}, {"id": "right", "name": "c"}]}  # fmt: skip
    plan, icons, _ = built(tmp_path, monkeypatch, table)
    with pytest.raises(ReplayError, match="is not speaker left's icon"):
        speakers.write(plan, tmp_path / "m.mp4", icons)


def test_one_speaker_is_not_a_version_2_plan(tmp_path: Path) -> None:
    cues = (SpeakerCue(0, 1, "left", "あ"), SpeakerCue(1, 2, "unassigned", "い"))
    with pytest.raises(ReplayError, match="Fewer than two speakers"):
        speakers.build(cues, {}, tmp_path / "m.mp4", burned=False, ffprobe="probe")


def test_a_speaker_missing_from_the_table_is_refused(tmp_path: Path) -> None:
    table = {"speakers": [{"id": "left", "name": "a"}, {"id": "middle", "name": "b"}]}
    with pytest.raises(ReplayError, match=r"Speakers \['right'\] are not in the speaker table"):
        speakers.build(CUES, table, tmp_path / "m.mp4", burned=False, ffprobe="probe")


def test_a_table_out_of_lane_order_is_refused(tmp_path: Path) -> None:
    table = {"speakers": [{"id": "middle", "name": "b"}, {"id": "left", "name": "a"}]}
    with pytest.raises(ReplayError, match="each lane once, left to right"):
        speakers.speaker_table(table, {"left"})


def test_a_table_entry_of_an_unknown_lane_is_refused() -> None:
    with pytest.raises(ReplayError, match="A speaker has id"):
        speakers.speaker_table({"speakers": [{"id": "far", "name": "x"}]}, set())


def test_an_icon_without_its_digest_is_refused() -> None:
    table = {"speakers": [{"id": "left", "name": "a", "icon": "/i.png"}]}
    with pytest.raises(ReplayError, match="icon with its SHA-256"):
        speakers.speaker_table(table, set())


def test_a_relative_icon_is_refused() -> None:
    table = {"speakers": [{"id": "left", "name": "a", "icon": "i.png", "iconSha256": "0"}]}
    with pytest.raises(ReplayError, match="absolute path"):
        speakers.speaker_table(table, set())


def test_the_cli_writes_beside_the_movie(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    cues = tmp_path / "cues.csv"
    cues.write_text("start,end,speaker,text\n2.0,3.0,left,あ\n2.5,3.5,right,い\n", encoding="utf-8")
    ffprobe(monkeypatch)
    assert speakers.main(["build", str(cues), str(tmp_path / "m.mp4"), "--burned"]) == 0
    lines = capsys.readouterr().out.splitlines()
    assert lines == [str(tmp_path / "m.captions.json"), str(tmp_path / "m.srt.speakers.json"),
                     '{"titles": 2, "unshown": 0}']  # fmt: skip
    plan = json.loads((tmp_path / "m.captions.json").read_text(encoding="utf-8"))
    assert plan["burned"] is True
    assert [item["id"] for item in plan["speakers"]] == ["left", "right"]


def test_the_cli_reads_a_speaker_table(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    cues = tmp_path / "cues.csv"
    cues.write_text("start,end,speaker,text\n2.0,3.0,left,あ\n2.5,3.5,middle,い\n", "utf-8")
    table = tmp_path / "speakers.json"
    listed = {"speakers": [{"id": "left", "name": "L"}, {"id": "middle", "name": "M"}]}
    table.write_text(json.dumps(listed), "utf-8")
    ffprobe(monkeypatch)
    assert speakers.main(["build", str(cues), str(tmp_path / "m.mp4"), "--unburned",
                          "--speakers", str(table)]) == 0  # fmt: skip
    capsys.readouterr()
    plan = json.loads((tmp_path / "m.captions.json").read_text(encoding="utf-8"))
    assert [item["name"] for item in plan["speakers"]] == ["L", "M"]
