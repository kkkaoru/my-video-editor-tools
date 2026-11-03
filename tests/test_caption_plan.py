import hashlib
import json
import subprocess
from collections.abc import Sequence
from pathlib import Path

import pytest

from replay import caption_plan, caption_render_native
from replay.core import ReplayError

FIXTURE = Path(__file__).parent / "fixtures" / "caption-plan-v1.json"
STYLE = {
    "fontName": "NotoSansJP-Thin_Bold", "fontSize": 82.0, "faceGrow": 0.3, "keyline": 1.36,
    "whiteBorder": 9.5, "rim": 12.3, "shadowOpacity": 0.7, "shadowOffsetY": 15.0,
    "shadowSigma": 5.3, "gradientTop": "#97D8F7", "gradientBottom": "#2FB0EE",
    "topLocation": 0.2688, "bottomLocation": 0.3944, "outputGamma": 1.961,
}  # fmt: skip


def fixture() -> dict[str, object]:
    return caption_plan.validate(json.loads(FIXTURE.read_text(encoding="utf-8")))


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def put(path: Path, value: object) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False), encoding="utf-8")
    return path


def recording(tmp_path: Path, *, layers: bool = True, speaker: bool = False) -> tuple[Path, Path]:
    """A synthetic recording: two parts (frames 2-12 and 20-30) of a 40-frame delivery."""
    root, work = tmp_path / "rec", tmp_path / "rec" / "rec"
    movie = tmp_path / "delivery" / "d-review.mp4"
    movie.parent.mkdir(parents=True)
    movie.write_bytes(b"movie")
    binary = tmp_path / "renderer"
    binary.write_bytes(b"binary")
    captions: dict[str, object] = {
        "nativeRender": {"binary": str(binary), "binarySha256": digest(binary)}
    }
    if speaker:
        icon, font = tmp_path / "icon.PNG", tmp_path / "font.ttf"
        icon.write_bytes(b"icon")
        font.write_bytes(b"font")
        captions["layout"] = {
            "version": 1, "canvas": [1080, 1920], "align": "left", "originX": 100,
            "regionTop": 1000, "regionBottom": 1800, "regionRight": 1000,
            "speaker": {
                "name": "テスト", "icon": str(icon), "iconSha256": digest(icon),
                "font": str(font), "fontSha256": digest(font), "iconX": 20.4, "iconY": 30,
                "iconSize": 150, "nameSize": 36.7,
            },
        }  # fmt: skip
    put(root / "captions.json", captions)
    put(root / "pipeline.json", {"ffprobe": "probe"})
    put(root / "steps" / "publish.json", {"outputs": {"delivery": str(movie)}})
    parts = [
        {"name": "p0", "offsetFrame": 2, "frames": 10, "moti": str(work / "p0.moti"),
         "renderDir": str(tmp_path / "alpha" / "p0")},
        {"name": "p1", "offsetFrame": 20, "frames": 10, "moti": str(work / "p1.moti"),
         "renderDir": str(tmp_path / "alpha" / "p1")},
    ]  # fmt: skip
    pinned = put(work / "caption-parts-v1" / "parts.json", parts)
    (work / "p0.moti").write_bytes(b"p0")
    (work / "p1.moti").write_bytes(b"p1")
    put(root / "steps" / "caption-parts.json",
        {"outputs": {"parts": {"path": str(pinned), "sha256": digest(pinned)}}})  # fmt: skip
    put(root / "steps" / "caption-render.json", {"startedAt": 1e12})
    groups = put(work / "compose-v1" / "requests" / "groups.json",
                 [{"name": "g000", "frames": 15}, {"name": "g001", "frames": 25}])  # fmt: skip
    first = [
        {"sourcePath": str(tmp_path / "alpha" / "p0" / "a.mov"),
         "selection": {"startSeconds": 0.0}, "offsetSeconds": 2 / 30},
        {"sourcePath": str(tmp_path / "badge.mov"), "selection": {"startSeconds": 0.0},
         "offsetSeconds": 0.0},
    ]  # fmt: skip
    second = [
        {"sourcePath": str(tmp_path / "alpha" / "p1" / "a.mov"),
         "selection": {"startSeconds": 0.0}, "offsetSeconds": 5 / 30},
    ]  # fmt: skip
    put(work / "compose-v1" / "requests" / "g000.json",
        {"recipe": {"additionalVideo": first if layers else []}})  # fmt: skip
    put(work / "compose-v1" / "requests" / "g001.json",
        {"recipe": {"additionalVideo": second} if layers else {}})  # fmt: skip
    compose: dict[str, object] = {"groups": {"path": str(groups), "sha256": digest(groups)}}
    if speaker:
        compose["badge"] = {"path": "badge.mov", "sha256": "0" * 64}
    put(root / "steps" / "compose.json", {"outputs": compose})
    return work, movie


def repin(work: Path, parts: object) -> None:
    """Replace the synthetic parts and the caption-parts receipt's pin of them."""
    pinned = put(work / "caption-parts-v1" / "parts.json", parts)
    put(work.parent / "steps" / "caption-parts.json",
        {"outputs": {"parts": {"path": str(pinned), "sha256": digest(pinned)}}})  # fmt: skip


def inspect(command: Sequence[str]) -> str:
    """The native renderer's ``--inspect`` of the synthetic parts (p1 has its own style)."""
    titles = {
        "p0.moti": [{"text": "あ\nい", "startFrame": 1, "endFrame": 6, "x": 0, "y": 700.5,
                     "graphemeFrames": [1, 1, 3]}],
        "p1.moti": [{"text": "う", "startFrame": 0, "endFrame": 10, "x": -400, "y": 700.5,
                     "graphemeFrames": [4], "alignment": "left"}],
    }  # fmt: skip
    name = Path(command[-1]).name
    style = {**STYLE, "fontSize": 90.0} if name == "p1.moti" else STYLE
    return json.dumps({"frames": 10, "width": 1080, "height": 1920, "rate": 30,
                       "style": style, "titles": titles[name]})  # fmt: skip


def ffprobe(monkeypatch: pytest.MonkeyPatch, stream: dict[str, object]) -> list[list[str]]:
    calls: list[list[str]] = []

    def run(command: list[str], **_: object) -> subprocess.CompletedProcess[str]:
        calls.append(command)
        return subprocess.CompletedProcess(command, 0, json.dumps({"streams": [stream]}), "")

    monkeypatch.setattr(subprocess, "run", run)
    return calls


VIDEO = {
    "width": 1080, "height": 1920, "r_frame_rate": "30/1", "nb_frames": "40",
    "start_time": "0.000000",
}  # fmt: skip


def test_fixture_is_a_valid_plan() -> None:
    assert caption_plan.validate(fixture())["frames"] == 300


def test_graphemes_count_line_breaks_and_clusters() -> None:
    assert caption_plan.graphemes("あ\nい") == 3
    assert caption_plan.graphemes("é👍🏽") == 2


def test_build_puts_every_title_on_the_delivery_clock(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path)
    calls = ffprobe(monkeypatch, VIDEO)
    plan, icon = caption_plan.build(work, movie, run=inspect)
    assert icon is None
    assert calls[0][0] == "probe"
    assert plan == {
        "version": 1, "clock": "delivery", "movie": "d-review.mp4", "burned": True,
        "width": 1080, "height": 1920, "rate": 30, "frames": 40, "style": STYLE,
        "titles": [
            {"text": "あ\nい", "startFrame": 3, "endFrame": 8, "x": 0, "y": 700.5,
             "graphemeFrames": [3, 3, 5]},
            {"text": "う", "startFrame": 20, "endFrame": 30, "x": -400, "y": 700.5,
             "graphemeFrames": [24], "alignment": "left",
             "style": {**STYLE, "fontSize": 90.0}},
        ],
        "nameplate": None,
    }  # fmt: skip


def test_build_without_composed_alpha_is_not_burned(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path, layers=False)
    ffprobe(monkeypatch, VIDEO)
    assert caption_plan.build(work, movie, run=inspect)[0]["burned"] is False


def test_build_draws_the_layout_nameplate(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    work, movie = recording(tmp_path, speaker=True)
    ffprobe(monkeypatch, VIDEO)
    plan, icon = caption_plan.build(work, movie, run=inspect)
    assert icon == tmp_path / "icon.PNG"
    assert plan["nameplate"] == {
        "burned": True, "icon": "d-review.nameplate.png",
        "iconSha256": hashlib.sha256(b"icon").hexdigest(), "iconX": 20, "iconY": 30,
        "iconSize": 150, "ringWidth": 4, "ringColour": "#FFFFFF", "name": "テスト",
        "nameFont": "font.ttf", "nameFontSha256": hashlib.sha256(b"font").hexdigest(),
        "nameFontVariation": "Bold", "nameSize": 36, "nameColour": "#FFFFFF",
        "nameStrokeWidth": 5, "nameStrokeColour": "#000000", "nameX": 95.0, "nameY": 186,
        "nameAnchor": "mt",
    }  # fmt: skip


def test_build_refuses_an_unrecorded_movie(tmp_path: Path) -> None:
    work, _ = recording(tmp_path)
    with pytest.raises(ReplayError, match="is not a delivery recorded"):
        caption_plan.build(work, tmp_path / "other.mp4", run=inspect)


def test_build_accepts_the_youtube_master(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    work, movie = recording(tmp_path)
    master = movie.with_name("master.mp4")
    master.write_bytes(b"master")
    put(work.parent / "steps" / "youtube-master.json",
        {"outputs": {"master": {"path": str(master), "sha256": "0" * 64}}})  # fmt: skip
    ffprobe(monkeypatch, VIDEO)
    assert caption_plan.build(work, master, run=inspect)[0]["movie"] == "master.mp4"


def test_build_refuses_a_movie_of_another_length(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path)
    ffprobe(monkeypatch, {**VIDEO, "nb_frames": "41"})
    with pytest.raises(ReplayError, match="is not the compose plan's length"):
        caption_plan.build(work, movie, run=inspect)


def test_build_refuses_a_fractional_rate(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    work, movie = recording(tmp_path)
    ffprobe(monkeypatch, {**VIDEO, "r_frame_rate": "30000/1001"})
    with pytest.raises(ReplayError, match="integer frame rate"):
        caption_plan.build(work, movie, run=inspect)


def test_build_refuses_another_canvas(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    work, movie = recording(tmp_path)
    ffprobe(monkeypatch, {**VIDEO, "width": 1920, "height": 1080})
    with pytest.raises(ReplayError, match="p0 is not on the delivery's canvas"):
        caption_plan.build(work, movie, run=inspect)


def test_build_refuses_overlapping_parts(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    work, movie = recording(tmp_path)
    parts = json.loads((work / "caption-parts-v1" / "parts.json").read_text(encoding="utf-8"))
    repin(work, [parts[0], {**parts[1], "offsetFrame": 11}])
    ffprobe(monkeypatch, VIDEO)
    with pytest.raises(ReplayError, match="p1 starts before the previous part ends"):
        caption_plan.build(work, movie, run=inspect)


def test_build_refuses_parts_past_the_delivery(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path)
    parts = json.loads((work / "caption-parts-v1" / "parts.json").read_text(encoding="utf-8"))
    repin(work, [parts[0], {**parts[1], "offsetFrame": 35}])
    ffprobe(monkeypatch, VIDEO)
    with pytest.raises(ReplayError, match="run past the delivery"):
        caption_plan.build(work, movie, run=inspect)


def test_build_refuses_no_parts(tmp_path: Path) -> None:
    work, movie = recording(tmp_path)
    repin(work, [])
    with pytest.raises(ReplayError, match="no caption parts"):
        caption_plan.build(work, movie, run=inspect)


def test_build_refuses_a_moti_of_another_length(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path)
    ffprobe(monkeypatch, VIDEO)
    with pytest.raises(ReplayError, match="p0: its .moti is not the part's length"):
        caption_plan.build(
            work, movie, run=lambda command: inspect(command).replace(": 10,", ": 9,")
        )


def test_build_refuses_a_title_past_its_part(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path)
    parts = json.loads((work / "caption-parts-v1" / "parts.json").read_text(encoding="utf-8"))
    repin(work, [{**parts[0], "frames": 5}, parts[1]])
    ffprobe(monkeypatch, VIDEO)
    with pytest.raises(ReplayError, match="p0: a title runs past its part"):
        caption_plan.build(
            work, movie, run=lambda command: inspect(command).replace('"frames": 10', '"frames": 5')
            if command[-1].endswith("p0.moti") else inspect(command),
        )  # fmt: skip


def test_build_refuses_an_alpha_composed_off_its_offset(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path)
    request = work / "compose-v1" / "requests" / "g001.json"
    put(request, {"recipe": {"additionalVideo": [
        {"sourcePath": str(tmp_path / "alpha" / "p1" / "a.mov"),
         "selection": {"startSeconds": 0.0}, "offsetSeconds": 6 / 30}]}})  # fmt: skip
    ffprobe(monkeypatch, VIDEO)
    with pytest.raises(ReplayError, match="p1 was composed at frame 21, not its offset"):
        caption_plan.build(work, movie, run=inspect)


def test_build_refuses_some_parts_composed(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    work, movie = recording(tmp_path)
    put(work / "compose-v1" / "requests" / "g001.json", {"recipe": {}})
    ffprobe(monkeypatch, VIDEO)
    with pytest.raises(ReplayError, match="Only some caption parts were composed"):
        caption_plan.build(work, movie, run=inspect)


def test_build_refuses_changed_compose_groups(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path)
    put(work / "compose-v1" / "requests" / "groups.json", [{"name": "g000", "frames": 40}])
    ffprobe(monkeypatch, VIDEO)
    with pytest.raises(ReplayError, match="differs from the compose receipt's pin"):
        caption_plan.build(work, movie, run=inspect)


def test_validate_refuses_overlapping_titles() -> None:
    plan = fixture()
    titles = plan["titles"]
    assert isinstance(titles, list)
    plan["titles"] = [titles[1], {**titles[2], "startFrame": 119}]
    with pytest.raises(ReplayError, match="overlaps another or the end"):
        caption_plan.validate(plan)


def test_validate_refuses_a_title_past_the_end() -> None:
    plan = {**fixture(), "frames": 150}
    with pytest.raises(ReplayError, match="'来てくれてありがとう\\\\nございます' overlaps"):
        caption_plan.validate(plan)


def test_validate_refuses_missing_onsets() -> None:
    plan = fixture()
    titles = plan["titles"]
    assert isinstance(titles, list)
    plan["titles"] = [{**titles[0], "graphemeFrames": [14]}]
    with pytest.raises(ReplayError, match="needs one onset frame per grapheme"):
        caption_plan.validate(plan)


def test_validate_refuses_onsets_out_of_order() -> None:
    plan = fixture()
    titles = plan["titles"]
    assert isinstance(titles, list)
    plan["titles"] = [{**titles[0], "graphemeFrames": [18, 14]}]
    with pytest.raises(ReplayError, match="outside its span or out of order"):
        caption_plan.validate(plan)


def test_validate_refuses_an_empty_span() -> None:
    plan = fixture()
    titles = plan["titles"]
    assert isinstance(titles, list)
    plan["titles"] = [{**titles[0], "endFrame": 14}]
    with pytest.raises(ReplayError, match="empty or negative span"):
        caption_plan.validate(plan)


def test_validate_refuses_empty_text() -> None:
    plan = fixture()
    titles = plan["titles"]
    assert isinstance(titles, list)
    plan["titles"] = [{**titles[0], "text": ""}]
    with pytest.raises(ReplayError, match="needs text"):
        caption_plan.validate(plan)


def test_validate_refuses_unknown_title_keys() -> None:
    plan = fixture()
    titles = plan["titles"]
    assert isinstance(titles, list)
    plan["titles"] = [{**titles[0], "colour": "red"}]
    with pytest.raises(ReplayError, match="A caption title has"):
        caption_plan.validate(plan)


def test_validate_refuses_a_centre_alignment_value() -> None:
    plan = fixture()
    titles = plan["titles"]
    assert isinstance(titles, list)
    plan["titles"] = [{**titles[0], "alignment": "centre"}]
    with pytest.raises(ReplayError, match="alignment is left or absent"):
        caption_plan.validate(plan)


def test_validate_refuses_a_numeric_speaker() -> None:
    plan = fixture()
    titles = plan["titles"]
    assert isinstance(titles, list)
    plan["titles"] = [{**titles[0], "speaker": 1}]
    with pytest.raises(ReplayError, match="speaker is a name or null"):
        caption_plan.validate(plan)


def test_validate_refuses_an_infinite_position() -> None:
    plan = fixture()
    titles = plan["titles"]
    assert isinstance(titles, list)
    plan["titles"] = [{**titles[0], "x": float("inf")}]
    with pytest.raises(ReplayError, match="x must be a finite number"):
        caption_plan.validate(plan)


def test_validate_refuses_a_fractional_frame() -> None:
    plan = fixture()
    titles = plan["titles"]
    assert isinstance(titles, list)
    plan["titles"] = [{**titles[0], "startFrame": 14.0}]
    with pytest.raises(ReplayError, match="startFrame must be an integer"):
        caption_plan.validate(plan)


def test_validate_refuses_non_list_onsets() -> None:
    plan = fixture()
    titles = plan["titles"]
    assert isinstance(titles, list)
    plan["titles"] = [{**titles[0], "graphemeFrames": "14"}]
    with pytest.raises(ReplayError, match="graphemeFrames must be a list"):
        caption_plan.validate(plan)


def test_validate_refuses_a_title_style_missing_a_key() -> None:
    plan = fixture()
    titles = plan["titles"]
    assert isinstance(titles, list)
    plan["titles"] = [{**titles[0], "style": {"fontName": "x"}}]
    with pytest.raises(ReplayError, match="A caption style has exactly"):
        caption_plan.validate(plan)


def test_validate_refuses_an_empty_font_name() -> None:
    plan = {**fixture(), "style": {**STYLE, "fontName": ""}}
    with pytest.raises(ReplayError, match="fontName must be a string"):
        caption_plan.validate(plan)


def test_validate_refuses_another_version() -> None:
    with pytest.raises(ReplayError, match="version 1"):
        caption_plan.validate({**fixture(), "version": 2})


def test_validate_refuses_a_movie_path() -> None:
    with pytest.raises(ReplayError, match="movie is the delivery's file name"):
        caption_plan.validate({**fixture(), "movie": "a/b.mp4"})


def test_validate_refuses_an_unknown_burned_value() -> None:
    with pytest.raises(ReplayError, match="burned must be true or false"):
        caption_plan.validate({**fixture(), "burned": "yes"})


def test_validate_refuses_a_zero_rate() -> None:
    with pytest.raises(ReplayError, match="rate must be positive"):
        caption_plan.validate({**fixture(), "rate": 0})


def test_validate_refuses_non_list_titles() -> None:
    with pytest.raises(ReplayError, match="titles must be a list"):
        caption_plan.validate({**fixture(), "titles": {}})


def test_validate_refuses_an_incomplete_nameplate() -> None:
    with pytest.raises(ReplayError, match="A nameplate has exactly"):
        caption_plan.validate({**fixture(), "nameplate": {"icon": "a.png"}})


def test_validate_refuses_a_nameplate_icon_path() -> None:
    plate = fixture()["nameplate"]
    assert isinstance(plate, dict)
    with pytest.raises(ReplayError, match="icon is a file name beside the plan"):
        caption_plan.validate({**fixture(), "nameplate": {**plate, "icon": "../a.png"}})


def test_validate_refuses_a_fractional_icon_size() -> None:
    plate = fixture()["nameplate"]
    assert isinstance(plate, dict)
    with pytest.raises(ReplayError, match="iconSize must be an integer"):
        caption_plan.validate({**fixture(), "nameplate": {**plate, "iconSize": 1.5}})


def test_write_publishes_the_plan_and_its_icon_once(tmp_path: Path) -> None:
    icon = tmp_path / "icon.png"
    icon.write_bytes(b"icon")
    plate = fixture()["nameplate"]
    assert isinstance(plate, dict)
    plan = {**fixture(), "nameplate": {**plate, "iconSha256": digest(icon)}}
    target = caption_plan.write(plan, tmp_path, icon)
    assert target == tmp_path / "synthetic-review.captions.json"
    assert json.loads(target.read_text(encoding="utf-8")) == plan
    assert (tmp_path / "synthetic-review.nameplate.png").read_bytes() == b"icon"
    assert target.stat().st_mode & 0o777 == 0o600
    assert caption_plan.write(plan, tmp_path, icon) == target
    assert sorted(path.name for path in tmp_path.iterdir()) == [
        "icon.png", "synthetic-review.captions.json", "synthetic-review.nameplate.png"
    ]  # fmt: skip


def test_write_refuses_to_replace_another_plan(tmp_path: Path) -> None:
    plan = {**fixture(), "nameplate": None}
    caption_plan.write(plan, tmp_path)
    with pytest.raises(ReplayError, match="exists with other content"):
        caption_plan.write({**plan, "burned": True}, tmp_path)


def test_write_refuses_another_icon(tmp_path: Path) -> None:
    icon = tmp_path / "icon.png"
    icon.write_bytes(b"other")
    with pytest.raises(ReplayError, match="is not the nameplate's icon"):
        caption_plan.write(fixture(), tmp_path, icon)


def test_write_refuses_a_nameplate_without_its_icon(tmp_path: Path) -> None:
    with pytest.raises(ReplayError, match="needs its icon, and only it"):
        caption_plan.write(fixture(), tmp_path)


def test_main_builds_and_writes_beside_the_movie(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    work, movie = recording(tmp_path)
    ffprobe(monkeypatch, VIDEO)
    monkeypatch.setattr(caption_render_native, "run_checked", inspect)
    assert caption_plan.main(["build", str(work), str(movie)]) == 0
    assert capsys.readouterr().out == f"{movie.parent / 'd-review.captions.json'}\n"


def test_build_refuses_parts_other_than_the_pinned(tmp_path: Path) -> None:
    work, movie = recording(tmp_path)
    put(work / "caption-parts-v1" / "parts.json", [])
    with pytest.raises(ReplayError, match="differs from the caption-parts receipt's pin"):
        caption_plan.build(work, movie, run=inspect)


def test_build_refuses_a_moti_staged_after_the_render(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path)
    put(work.parent / "steps" / "caption-render.json", {"startedAt": 1.0})
    ffprobe(monkeypatch, VIDEO)
    with pytest.raises(ReplayError, match="p0: its .moti changed after the caption render"):
        caption_plan.build(work, movie, run=inspect)


def test_build_refuses_a_movie_not_starting_at_zero(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path)
    ffprobe(monkeypatch, {**VIDEO, "start_time": "0.033333"})
    with pytest.raises(ReplayError, match="does not start at time zero"):
        caption_plan.build(work, movie, run=inspect)


def test_build_refuses_a_nameplate_icon_that_is_not_png(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path, speaker=True)
    captions = json.loads((work.parent / "captions.json").read_text(encoding="utf-8"))
    icon = tmp_path / "icon.jpg"
    icon.write_bytes(b"icon")
    captions["layout"]["speaker"]["icon"] = str(icon)
    put(work.parent / "captions.json", captions)
    ffprobe(monkeypatch, VIDEO)
    with pytest.raises(ReplayError, match="is not a PNG"):
        caption_plan.build(work, movie, run=inspect)


def test_fixture_icon_has_the_fixture_sha() -> None:
    plate = fixture()["nameplate"]
    assert isinstance(plate, dict)
    assert digest(FIXTURE.with_name("synthetic-review.nameplate.png")) == plate["iconSha256"]


def test_build_accepts_a_verified_master_of_the_published_delivery(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path)
    master = tmp_path / "youtube" / "delivery" / "d-review.mp4"
    master.parent.mkdir(parents=True)
    master.write_bytes(b"master")
    put(tmp_path / "youtube" / "metadata" / "reencode-verification.json",
        {"master": {"path": str(master)}, "delivery": {"path": str(movie)}})  # fmt: skip
    ffprobe(monkeypatch, VIDEO)
    assert caption_plan.build(work, master, run=inspect)[0]["frames"] == 40


def test_build_refuses_a_master_of_another_delivery(tmp_path: Path) -> None:
    work, movie = recording(tmp_path)
    master = tmp_path / "youtube" / "delivery" / "d-review.mp4"
    master.parent.mkdir(parents=True)
    master.write_bytes(b"master")
    put(tmp_path / "youtube" / "metadata" / "reencode-verification.json",
        {"master": {"path": str(master)},
         "delivery": {"path": str(tmp_path / "x.mp4")}})  # fmt: skip
    with pytest.raises(ReplayError, match="is not a delivery recorded"):
        caption_plan.build(work, master, run=inspect)


def test_build_needs_a_reader_without_native_render(tmp_path: Path) -> None:
    work, movie = recording(tmp_path)
    put(work.parent / "captions.json", {})
    with pytest.raises(ReplayError, match="names no native .moti reader"):
        caption_plan.build(work, movie, run=inspect)


def test_main_uses_an_explicit_reader(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    work, movie = recording(tmp_path)
    put(work.parent / "captions.json", {})
    ffprobe(monkeypatch, VIDEO)
    monkeypatch.setattr(caption_render_native, "run_checked", inspect)
    binary = tmp_path / "renderer"
    assert caption_plan.main(["build", str(work), str(movie), "--binary", str(binary),
                              "--binary-sha256", digest(binary)]) == 0  # fmt: skip
    assert capsys.readouterr().out == f"{movie.parent / 'd-review.captions.json'}\n"


def test_chain_failure_accepts_every_chain_shown_in_order() -> None:
    chains = [("ふぅ", 13, 30), ("なんかミスっ", 40, 80), ("なんかミスっ\nてないかなぁ", 81, 119),
              ("来てくれてありがとう\nございます", 121, 157)]  # fmt: skip
    assert caption_plan.chain_failure(fixture(), chains) is None


def test_chain_failure_names_a_caption_no_title_shows() -> None:
    chains = [("ふぅ", 14, 30), ("違う", 50, 60)]
    assert caption_plan.chain_failure(fixture(), chains) == (
        "no title shows the caption '違う' at frame 50"
    )


def test_chain_failure_refuses_a_chain_far_from_its_title() -> None:
    assert caption_plan.chain_failure(fixture(), [("ふぅ", 20, 30)]) == (
        "no title shows the caption 'ふぅ' at frame 20"
    )


def test_chain_failure_counts_titles_without_a_chain() -> None:
    chains = [("ふぅ", 14, 30), ("なんかミスっ\nてないかなぁ", 40, 119)]
    assert caption_plan.chain_failure(fixture(), chains) == "1 titles show no displayed caption"


def test_build_without_burned_parts_ignores_a_stale_render(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path, layers=False)
    put(work.parent / "steps" / "caption-render.json", {"startedAt": 1.0})
    ffprobe(monkeypatch, VIDEO)
    assert caption_plan.build(work, movie, run=inspect)[0]["burned"] is False


def test_build_of_burned_parts_without_a_render_receipt(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path)
    (work.parent / "steps" / "caption-render.json").unlink()
    ffprobe(monkeypatch, VIDEO)
    assert caption_plan.build(work, movie, run=inspect)[0]["burned"] is True


V2 = FIXTURE.with_name("caption-plan-v2.json")


def v2() -> dict[str, object]:
    data: dict[str, object] = json.loads(V2.read_text(encoding="utf-8"))
    return data


def v2_speakers() -> list[dict[str, object]]:
    table = v2()["speakers"]
    assert isinstance(table, list)
    return table


def test_v2_fixture_is_a_valid_plan() -> None:
    assert caption_plan.validate(v2())["version"] == 2


def test_v2_speaker_icon_files_have_their_sha() -> None:
    left = v2_speakers()[0]
    assert digest(V2.with_name(str(left["icon"]))) == left["iconSha256"]


def test_v2_needs_two_speakers() -> None:
    with pytest.raises(ReplayError, match="at least two speakers with unique ids"):
        caption_plan.validate({**v2(), "speakers": v2_speakers()[:1]})


def test_v2_speaker_ids_are_unique() -> None:
    first = v2_speakers()[1]
    with pytest.raises(ReplayError, match="at least two speakers with unique ids"):
        caption_plan.validate({**v2(), "speakers": [first, first]})


def test_v2_reserves_the_unassigned_id() -> None:
    reserved = {**v2_speakers()[1], "id": "unassigned"}
    with pytest.raises(ReplayError, match="a reserved id or an icon path"):
        caption_plan.validate({**v2(), "speakers": [v2_speakers()[0], reserved]})


def test_v2_icon_and_its_sha_come_together() -> None:
    lone = {**v2_speakers()[1], "icon": "a.png"}
    with pytest.raises(ReplayError, match="A speaker has"):
        caption_plan.validate({**v2(), "speakers": [v2_speakers()[0], lone]})


def test_v2_speaker_names_are_strings() -> None:
    unnamed = {**v2_speakers()[1], "name": " "}
    with pytest.raises(ReplayError, match="name must be a non-empty string"):
        caption_plan.validate({**v2(), "speakers": [v2_speakers()[0], unnamed]})


def test_v2_speaker_colours_are_hex() -> None:
    pale = {**v2_speakers()[1], "colorTop": "white"}
    with pytest.raises(ReplayError, match="colours are #RRGGBB"):
        caption_plan.validate({**v2(), "speakers": [v2_speakers()[0], pale]})


def test_v2_titles_name_known_speakers() -> None:
    titles = v2()["titles"]
    assert isinstance(titles, list)
    with pytest.raises(ReplayError, match="names an unknown speaker"):
        caption_plan.validate({**v2(), "titles": [{**titles[0], "speaker": "nobody"}]})


def test_v2_one_speakers_titles_never_overlap() -> None:
    titles = v2()["titles"]
    assert isinstance(titles, list)
    again = {**titles[0], "startFrame": 20, "graphemeFrames": [20, 21, 22, 23, 24]}
    with pytest.raises(ReplayError, match="overlaps another or the end"):
        caption_plan.validate({**v2(), "titles": [titles[0], again]})


def test_v2_unassigned_titles_may_overlap() -> None:
    titles = v2()["titles"]
    assert isinstance(titles, list)
    unassigned = [item for item in titles if item.get("speaker") is None]
    assert caption_plan.validate({**v2(), "titles": unassigned[:2]})["version"] == 2


def test_a_version_1_plan_cannot_carry_speakers() -> None:
    with pytest.raises(ReplayError, match="version 1 or 2"):
        caption_plan.validate({**fixture(), "speakers": v2_speakers()})
