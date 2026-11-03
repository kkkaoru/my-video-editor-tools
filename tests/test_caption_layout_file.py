import json
import shutil
from pathlib import Path

import pytest

from replay import caption_layout_file, caption_position
from replay.core import ReplayError

FIXTURES = Path(__file__).parent / "fixtures"


def load(name: str) -> dict[str, object]:
    data: dict[str, object] = json.loads((FIXTURES / name).read_text(encoding="utf-8"))
    return data


def resolved(case: str) -> list[dict[str, object]]:
    """The fixture plan and faces under one expected case's layout."""
    cases = load("caption-layout-expected-v1.json")["cases"]
    assert isinstance(cases, list)
    layout = next(item["layout"] for item in cases if item["name"] == case)
    return caption_layout_file.resolve(
        load("caption-plan-v1.json"), layout, load("caption-faces-v1.json")
    )


def expected(case: str) -> object:
    cases = load("caption-layout-expected-v1.json")["cases"]
    assert isinstance(cases, list)
    return next(item["titles"] for item in cases if item["name"] == case)


def test_layout_fixture_is_valid() -> None:
    assert caption_layout_file.validate(load("caption-layout-v1.json")) == {
        "version": 1, "mode": "aboveFace", "offsetY": -40
    }  # fmt: skip


def test_above_face_places_the_box_above_the_face_or_holds_it() -> None:
    assert resolved("above-face") == [
        {"x": -460.0, "y": pytest.approx(-70.22873, abs=1e-5), "basis": "face"},
        {"x": -460.0, "y": pytest.approx(49.89127, abs=1e-5), "basis": "held"},
        {"x": -460.0, "y": pytest.approx(-10.10873, abs=1e-5), "basis": "face"},
    ]


def test_above_face_falls_back_to_the_plan_beyond_the_drift() -> None:
    assert [title["basis"] for title in resolved("above-face-no-drift")] == [
        "face", "plan", "face"
    ]  # fmt: skip
    assert resolved("above-face-no-drift")[1]["y"] == -322.0


def test_above_face_falls_back_to_the_plan_on_one_side() -> None:
    assert [title["basis"] for title in resolved("above-face-one-sided")] == [
        "plan", "plan", "face"
    ]  # fmt: skip


def test_above_face_keeps_the_margin() -> None:
    assert resolved("above-face-high")[0]["y"] == pytest.approx(299.77127, abs=1e-5)


def test_top_reproduces_the_region_top_origin() -> None:
    assert resolved("top")[0]["y"] == pytest.approx(379.77127, abs=1e-5)


def test_bottom_offset_clamps_into_the_canvas() -> None:
    assert resolved("bottom-offset-clamped")[0] == {
        "x": 960.0, "y": pytest.approx(-510.22873, abs=1e-5), "basis": "mode"
    }  # fmt: skip


def test_every_expected_case_matches() -> None:
    assert resolved("absent") == expected("absent")
    assert resolved("plan-offset") == expected("plan-offset")
    assert resolved("bottom") == expected("bottom")
    assert resolved("center") == expected("center")


def test_a_title_style_sets_its_own_box() -> None:
    plan = load("caption-plan-v1.json")
    titles = plan["titles"]
    style = plan["style"]
    assert isinstance(titles, list) and isinstance(style, dict)
    plan["titles"] = [{**titles[0], "style": {**style, "fontSize": 100.0}}]
    assert caption_layout_file.resolve(plan, {"version": 1, "mode": "top"}, None) == [
        {"x": -460.0, "y": pytest.approx(280.0, abs=1e-9), "basis": "mode"}
    ]


def test_above_face_without_faces_keeps_the_plan() -> None:
    assert caption_layout_file.resolve(
        load("caption-plan-v1.json"), {"version": 1, "mode": "aboveFace"}, None
    )[0] == {"x": -460.0, "y": -404.0, "basis": "plan"}


def test_resolve_refuses_faces_of_another_canvas() -> None:
    faces = {**load("caption-faces-v1.json"), "width": 1080, "height": 1920}
    with pytest.raises(ReplayError, match="not on the plan's canvas"):
        caption_layout_file.resolve(load("caption-plan-v1.json"), {"version": 1, "mode": "top"},
                                    faces)  # fmt: skip


def test_validate_refuses_an_unknown_mode() -> None:
    with pytest.raises(ReplayError, match="mode is one of"):
        caption_layout_file.validate({"version": 1, "mode": "left"})


def test_validate_refuses_an_unknown_key() -> None:
    with pytest.raises(ReplayError, match="A caption layout has"):
        caption_layout_file.validate({"version": 1, "mode": "top", "x": 1})


def test_validate_refuses_a_negative_margin() -> None:
    with pytest.raises(ReplayError, match="margin is out of range"):
        caption_layout_file.validate({"version": 1, "mode": "top", "margin": -1})


def test_validate_refuses_a_score_above_one() -> None:
    with pytest.raises(ReplayError, match="minScore is out of range"):
        caption_layout_file.validate({"version": 1, "mode": "top", "minScore": 2})


def test_validate_refuses_a_text_offset() -> None:
    with pytest.raises(ReplayError, match="offsetY must be a finite number"):
        caption_layout_file.validate({"version": 1, "mode": "top", "offsetY": "-40"})


def delivery(tmp_path: Path) -> Path:
    """A delivery directory holding the fixture plan and face track for ``s.mp4``."""
    movie = tmp_path / "s.mp4"
    shutil.copy(FIXTURES / "caption-plan-v1.json", tmp_path / "s.captions.json")
    shutil.copy(FIXTURES / "caption-faces-v1.json", tmp_path / "s.faces.json")
    return movie


def test_cli_show_without_a_file_is_the_plan(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    assert caption_position.main(["show", str(delivery(tmp_path))]) == 0
    assert json.loads(capsys.readouterr().out) == {
        "version": 1, "mode": "plan", "offsetX": 0.0, "offsetY": 0.0, "margin": 120.0,
        "faceGap": 60.0, "minScore": 0.5, "maxDrift": 150.0,
    }  # fmt: skip


def test_cli_set_keeps_the_options_it_is_not_given(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    movie = delivery(tmp_path)
    assert caption_position.main(["set", str(movie), "--mode", "aboveFace", "--margin", "80"]) == 0
    assert caption_position.main(["set", str(movie), "--offset-y", "-40"]) == 0
    assert capsys.readouterr().out == f"{tmp_path / 's.caption-layout.json'}\n" * 2
    assert json.loads((tmp_path / "s.caption-layout.json").read_text(encoding="utf-8")) == {
        "version": 1, "mode": "aboveFace", "margin": 80.0, "offsetY": -40.0
    }  # fmt: skip
    assert (tmp_path / "s.caption-layout.json").stat().st_mode & 0o777 == 0o600


def test_cli_resolve_counts_each_basis(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    movie = delivery(tmp_path)
    caption_layout_file.write({"version": 1, "mode": "aboveFace", "maxDrift": 10}, movie)
    assert caption_position.main(["resolve", str(movie)]) == 0
    assert json.loads(capsys.readouterr().out) == {
        "titles": 3, "basis": {"face": 2, "plan": 1}
    }  # fmt: skip


def test_cli_resolve_lists_titles(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    assert caption_position.main(["resolve", str(delivery(tmp_path)), "--titles"]) == 0
    assert json.loads(capsys.readouterr().out)["resolved"][0] == {
        "x": -460.0, "y": -404.0, "basis": "plan"
    }  # fmt: skip


def resolved_v2(case: str) -> tuple[list[dict[str, object]], object]:
    """The version-2 fixture plan under one expected case's layout and face track."""
    cases = load("caption-layout-expected-v2.json")["cases"]
    assert isinstance(cases, list)
    item = next(entry for entry in cases if entry["name"] == case)
    titles = caption_layout_file.resolve(
        load("caption-plan-v2.json"), item["layout"], load(str(item["faces"]))
    )
    return titles, item["titles"]


def close(left: list[dict[str, object]], right: object) -> bool:
    """Every title's basis equal and its position within the fixture's 1e-6."""
    assert isinstance(right, list)
    return len(left) == len(right) and all(
        a["basis"] == b["basis"]
        and abs(float(str(a["x"])) - b["x"]) <= 1e-6
        and abs(float(str(a["y"])) - b["y"]) <= 1e-6
        for a, b in zip(left, right, strict=True)
    )


def test_v2_absent_layout_still_stacks_unassigned_titles() -> None:
    assert close(*resolved_v2("absent"))


def test_v2_fixture_layout_gives_speakers_their_own_options() -> None:
    assert close(*resolved_v2("fixture-layout"))


def test_v2_bottom_for_every_title() -> None:
    assert close(*resolved_v2("bottom-all"))


def test_v2_above_face_assigns_faces_by_lane_band() -> None:
    assert close(*resolved_v2("above-face-all"))


def test_v2_plan_with_version_1_faces_uses_the_one_face_rule() -> None:
    assert close(*resolved_v2("above-face-v1-faces"))


def test_v2_per_speaker_offsets() -> None:
    assert close(*resolved_v2("per-speaker-offsets"))


def test_v2_speaker_icons_sit_on_their_lanes_at_the_bottom() -> None:
    assert caption_layout_file.speaker_icons(load("caption-plan-v2.json")) == [
        {"id": "left", "x": 607, "y": 972, "size": 86, "ring": 2},
        {"id": "right", "x": 1607, "y": 972, "size": 86, "ring": 2},
    ]


def test_a_layout_naming_a_speaker_the_plan_lacks_is_refused() -> None:
    layout = {"version": 2, "mode": "plan", "speakers": {"nobody": {"mode": "top"}}}
    with pytest.raises(ReplayError, match="speaker the plan does not have"):
        caption_layout_file.resolve(load("caption-plan-v2.json"), layout, None)


def test_a_version_1_layout_cannot_name_speakers() -> None:
    with pytest.raises(ReplayError, match="A caption layout has"):
        caption_layout_file.validate({"version": 1, "mode": "top", "speakers": {}})


def test_a_layout_of_another_version_is_refused() -> None:
    with pytest.raises(ReplayError, match="version 1 or 2"):
        caption_layout_file.validate({"version": 3, "mode": "top"})


def test_a_speakers_unknown_option_is_refused() -> None:
    with pytest.raises(ReplayError, match="speaker 'left' has mode"):
        caption_layout_file.validate(
            {"version": 2, "mode": "top", "speakers": {"left": {"colour": 1}}}
        )


def test_a_speakers_out_of_range_option_is_refused() -> None:
    with pytest.raises(ReplayError, match="maxDrift is out of range"):
        caption_layout_file.validate(
            {"version": 2, "mode": "top", "speakers": {"left": {"maxDrift": -1}}}
        )
