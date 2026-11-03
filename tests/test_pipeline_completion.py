"""Pipeline deliveries: publication, SRT of displayed captions and the fast five-stage gate."""

import json
import subprocess
from pathlib import Path
from types import SimpleNamespace

import pytest

from replay import movie_decode
from replay import pipeline_completion as completion
from replay.core import ReplayError
from replay.edit_completion import REQUIRED_STAGES

SOURCE = "2026-10-14_23-01-14_JST_replay-1234567890123456789.mp4"
STEM = "2026-10-14_23-01-14_JST_replay-1234567890123456789-review"
STYLE = {
    "fontName": "NotoSansJP-Thin_Bold", "fontSize": 82.0, "faceGrow": 0.3, "keyline": 1.36,
    "whiteBorder": 9.5, "rim": 12.3, "shadowOpacity": 0.7, "shadowOffsetY": 15.0,
    "shadowSigma": 5.3, "gradientTop": "#97D8F7", "gradientBottom": "#2FB0EE",
    "topLocation": 0.2688, "bottomLocation": 0.3944, "outputGamma": 1.961,
}  # fmt: skip


def cue(text: str, start: float, end: float, y: float = 100.0) -> dict[str, object]:
    return {"cue": {"text": text, "start": start, "end": end}, "x": 540.0, "y": y}


def title(text: str, start: int, end: int) -> dict[str, object]:
    graphemes = [start] * len(text)
    return {"text": text, "startFrame": start, "endFrame": end, "x": 0.0, "y": 700.0,
            "graphemeFrames": graphemes}  # fmt: skip


def write_plan(
    storage: Path,
    titles: list[dict[str, object]],
    *,
    frames: int = 90,
    revision: str = "pipeline-abc",
) -> Path:
    """The caption plan beside the delivery ``publish`` puts in ``revision``."""
    plan = {
        "version": 1, "clock": "delivery", "movie": f"{STEM}.mp4", "burned": False,
        "width": 1080, "height": 1920, "rate": 30, "frames": frames, "style": STYLE,
        "titles": titles, "nameplate": None,
    }  # fmt: skip
    path = storage / "revisions" / revision / "delivery" / f"{STEM}.captions.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(plan, ensure_ascii=False), encoding="utf-8")
    return path


def recording(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> dict[str, Path]:
    """A published-ready recording: edit root, steps, candidate, movie and cleanup state."""
    root = tmp_path / "edits/replay-1234567890123456789"
    root.mkdir(parents=True)
    (root / "edit-config.json").write_text("{}", encoding="utf-8")
    storage = tmp_path / "storage/replay-1234567890123456789"
    storage.mkdir(parents=True)
    source = tmp_path / SOURCE
    monkeypatch.setattr(
        completion, "load_config", lambda _r: SimpleNamespace(storage_root=storage, source=source)
    )
    movie = tmp_path / "candidate.mp4"
    movie.write_bytes(b"movie")
    candidate = tmp_path / "placement.json"
    rows = [cue("あ", 0, 0.5), cue("あい", 0.5, 1), cue("う", 2, 3)]
    candidate.write_text(json.dumps({"candidates": rows}), encoding="utf-8")
    work = tmp_path / "clean"
    (work / "configs").mkdir(parents=True)
    (work / "state/chunk-00000").mkdir(parents=True)
    (work / "configs/chunk-00000.json").write_text("{}", encoding="utf-8")
    (work / "state/chunk-00000/chunk-receipt.json").write_text('{"accepted": "a"}', "utf-8")
    description = tmp_path / "description.json"
    description.write_text(json.dumps({"work": str(work)}), encoding="utf-8")
    cuts = tmp_path / "cuts.json"
    cuts.write_text(
        json.dumps({"cuts": [{"source_start_frame": 10, "source_end_frame": 100}]}), "utf-8"
    )
    steps = tmp_path / "steps"
    steps.mkdir()
    outputs = {
        "separation": {"vocals": {"path": "/v.wav"}},
        "cut-plan": {"cuts": {"path": str(cuts)}},
        "cleanup": {"description": {"path": str(description)}},
        "placement": {"failedRuns": 0},
    }
    for name in completion.STEP_NAMES:
        body = {"step": name, "outputs": outputs.get(name, {})}
        (steps / f"{name}.json").write_text(json.dumps(body), encoding="utf-8")
    write_plan(storage, [title("あい", 0, 30), title("う", 60, 90)])
    return {"root": root, "movie": movie, "steps": steps, "candidate": candidate, "work": work,
            "storage": storage}  # fmt: skip


def good_movie(monkeypatch: pytest.MonkeyPatch, *, frames: int = 90) -> None:
    streams = [
        {"codec_type": "video", "nb_read_packets": str(frames), "duration": "3.0"},
        {"codec_type": "audio", "nb_read_packets": "130", "duration": "3.0"},
    ]
    monkeypatch.setattr(completion, "probe", lambda _m, _p: {"streams": streams})
    monkeypatch.setattr(movie_decode, "decodes", lambda _m, **_k: True)


def published(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> tuple[dict[str, Path], Path]:
    paths = recording(tmp_path, monkeypatch)
    delivery, index = completion.publish(
        paths["root"], movie=paths["movie"], steps=paths["steps"],
        candidate=paths["candidate"], revision="pipeline-abc",
    )  # fmt: skip
    return paths | {"delivery": delivery}, index


def test_growing_cues_become_one_displayed_caption() -> None:
    assert completion.srt([("あい", 0, 30), ("う", 60, 90)]) == (
        "1\n00:00:00,000 --> 00:00:01,000\nあい\n\n2\n00:00:02,000 --> 00:00:03,000\nう\n"
    )


def test_a_moved_or_reset_cue_starts_a_new_caption(tmp_path: Path) -> None:
    path = tmp_path / "c.json"
    rows = [cue("あ", 0, 1), cue("あい", 1, 2, y=50), cue("x", 2, 3)]
    path.write_text(json.dumps({"candidates": rows}), encoding="utf-8")
    assert completion.displayed(path) == [("あ", 0, 30), ("あい", 30, 60), ("x", 60, 90)]


def test_an_empty_candidate_is_refused(tmp_path: Path) -> None:
    (tmp_path / "c.json").write_text('{"candidates": []}', encoding="utf-8")
    with pytest.raises(ReplayError, match="no cues"):
        completion.displayed(tmp_path / "c.json")


def test_publication_links_the_movie_and_the_gate_passes(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths, index = published(tmp_path, monkeypatch)
    delivery = paths["delivery"]
    assert delivery.name == "2026-10-14_23-01-14_JST_replay-1234567890123456789-review.mp4"
    assert delivery.samefile(paths["movie"])
    assert delivery.with_suffix(".srt").read_text(encoding="utf-8").startswith("1\n00:00:00,000")
    good_movie(monkeypatch)
    assert completion.assess(index, delivery=delivery, root=paths["root"]) == {}


def carrying(text: str, start: float, end: float, carried: str) -> dict[str, object]:
    return {"cue": {"text": text, "start": start, "end": end, "carried": carried},
            "x": 540.0, "y": 100.0}  # fmt: skip


def test_the_srt_holds_only_newly_spoken_words_while_the_display_keeps_the_carry(
    tmp_path: Path,
) -> None:
    path = tmp_path / "c.json"
    rows = [
        cue("今日は", 0, 1), cue("今日は\nいい天気", 1, 2),
        carrying("いい天気\nですね", 2, 3, "いい天気"),
        carrying("いい天気\nですねそう", 3, 4, "いい天気"),
        # A page whose carry took all its words extends the chain it carries from.
        cue("はい", 5, 6), carrying("はいはい", 6, 7, "はい"),
    ]  # fmt: skip
    path.write_text(json.dumps({"candidates": rows}), encoding="utf-8")
    assert completion.displayed(path) == [
        ("今日は\nいい天気", 0, 60), ("いい天気\nですねそう", 60, 120), ("はいはい", 150, 210),
    ]  # fmt: skip
    assert completion.transcript(path) == [
        ("今日は\nいい天気", 0, 60), ("ですねそう", 60, 120), ("はいはい", 150, 210),
    ]  # fmt: skip
    assert completion.spoken("いい\n天気です", "いい天気") == "です"
    assert completion.spoken("動画でもしてる \n定期的にね", "動画でもしてる") == "定期的にね"


@pytest.mark.parametrize("carried", ["天気", "いい天気です"])
def test_carried_words_that_do_not_lead_their_caption_are_refused(carried: str) -> None:
    with pytest.raises(ReplayError, match="carried words"):
        completion.spoken("いい天気\nです", carried)


def test_a_carrying_delivery_publishes_and_passes_with_the_spoken_srt(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = recording(tmp_path, monkeypatch)
    rows = [cue("あい", 0, 1), carrying("い\nう", 1, 2, "い")]
    paths["candidate"].write_text(json.dumps({"candidates": rows}), encoding="utf-8")
    write_plan(paths["storage"], [title("あい", 0, 30), title("い\nう", 30, 60)])
    delivery, index = completion.publish(
        paths["root"], movie=paths["movie"], steps=paths["steps"],
        candidate=paths["candidate"], revision="pipeline-abc",
    )  # fmt: skip
    assert delivery.with_suffix(".srt").read_text(encoding="utf-8") == (
        "1\n00:00:00,000 --> 00:00:01,000\nあい\n\n2\n00:00:01,000 --> 00:00:02,000\nう\n"
    )
    good_movie(monkeypatch)
    assert completion.assess(index, delivery=delivery, root=paths["root"]) == {}


def test_publishing_again_is_idempotent(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    paths, _ = published(tmp_path, monkeypatch)
    again = completion.publish(
        paths["root"], movie=paths["movie"], steps=paths["steps"],
        candidate=paths["candidate"], revision="pipeline-abc",
    )  # fmt: skip
    assert again[0] == paths["delivery"]


def test_another_file_at_the_delivery_path_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths, _ = published(tmp_path, monkeypatch)
    other = tmp_path / "other.mp4"
    other.write_bytes(b"other")
    with pytest.raises(ReplayError, match="not the pipeline's movie"):
        completion.publish(
            paths["root"], movie=other, steps=paths["steps"],
            candidate=paths["candidate"], revision="pipeline-abc",
        )  # fmt: skip


def test_an_unbound_recording_cannot_publish(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = recording(tmp_path, monkeypatch)
    monkeypatch.setattr(completion, "load_config", lambda _r: SimpleNamespace(storage_root=None))
    with pytest.raises(ReplayError, match="storage root"):
        completion.publish(
            paths["root"], movie=paths["movie"], steps=paths["steps"],
            candidate=paths["candidate"], revision="r",
        )  # fmt: skip


def test_each_stage_fails_on_its_own_evidence(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths, index = published(tmp_path, monkeypatch)
    good_movie(monkeypatch, frames=89)
    write_plan(paths["storage"], [title("あい", 0, 30), title("う", 60, 89)], frames=89)
    monkeypatch.setattr(movie_decode, "decodes", lambda _m, **_k: False)
    (paths["work"] / "configs/chunk-00001.json").write_text("{}", encoding="utf-8")
    failures = completion.stage_failures(
        json.loads(index.read_text(encoding="utf-8")), ffmpeg="f", ffprobe="p"
    )
    assert failures == {
        "silence_cut": "movie frames differ from the 90 planned frames",
        "source_caption_mask": "1 of 2 cleanup chunks accepted",
        "dynamic_captions": "the movie does not decode cleanly",
    }


def test_audio_and_caption_failures_are_reported(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths, index = published(tmp_path, monkeypatch)
    streams = [{"codec_type": "video", "nb_read_packets": "90", "duration": "3.0"},
               {"codec_type": "audio", "duration": "2.5"}]  # fmt: skip
    monkeypatch.setattr(completion, "probe", lambda _m, _p: {"streams": streams})
    data = json.loads(index.read_text(encoding="utf-8"))
    paths["delivery"].with_suffix(".srt").write_text("other", encoding="utf-8")
    placement = Path(data["steps"]["placement"]["path"])
    placement.write_text(json.dumps({"outputs": {"failedRuns": 2}}), encoding="utf-8")
    failures = completion.stage_failures(data, ffmpeg="f", ffprobe="p")
    assert failures == {
        "bgm_removal": "audio and video lengths differ",
        "transcription": "the SRT sidecar differs from the displayed captions",
        "dynamic_captions": "some captions were not placed",
    }
    placement.write_text(json.dumps({"outputs": {"failedRuns": 0}}), encoding="utf-8")
    paths["delivery"].with_name(f"{STEM}.captions.json").unlink()
    monkeypatch.setattr(completion, "probe", lambda _m, _p: {"streams": streams[:1]})
    failures = completion.stage_failures(data, ffmpeg="f", ffprobe="p")
    assert failures["dynamic_captions"] == "no caption plan beside the movie"
    assert failures["bgm_removal"].startswith("no pinned separated voice")


def test_a_plan_must_be_valid_this_movies_and_show_every_caption(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths, index = published(tmp_path, monkeypatch)
    good_movie(monkeypatch)
    data = json.loads(index.read_text(encoding="utf-8"))
    plan = paths["delivery"].with_name(f"{STEM}.captions.json")
    plan.write_text("{}", encoding="utf-8")
    assert completion.stage_failures(data, ffmpeg="f", ffprobe="p")["dynamic_captions"].startswith(
        "the caption plan is invalid: A caption plan has exactly"
    )
    write_plan(paths["storage"], [title("あい", 0, 30)])
    assert completion.stage_failures(data, ffmpeg="f", ffprobe="p") == {
        "dynamic_captions": "no title shows the caption 'う' at frame 60"
    }
    good_movie(monkeypatch, frames=91)
    assert completion.stage_failures(data, ffmpeg="f", ffprobe="p")["dynamic_captions"] == (
        "the caption plan is not this movie's"
    )


def test_a_delivery_burned_before_the_plan_passes_with_its_rendered_parts(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = recording(tmp_path, monkeypatch)
    render = paths["steps"] / "caption-render.json"
    render.write_text(json.dumps({"outputs": {"parts": 3}}), encoding="utf-8")
    delivery, index = completion.publish(
        paths["root"], movie=paths["movie"], steps=paths["steps"],
        candidate=paths["candidate"], revision="pipeline-abc",
    )  # fmt: skip
    delivery.with_name(f"{STEM}.captions.json").unlink()
    good_movie(monkeypatch)
    data = json.loads(index.read_text(encoding="utf-8"))
    assert sorted(data["steps"]) == sorted([*completion.STEP_NAMES, "caption-render"])
    assert completion.stage_failures(data, ffmpeg="f", ffprobe="p") == {}
    render.write_text(json.dumps({"outputs": {}}), encoding="utf-8")
    assert completion.stage_failures(data, ffmpeg="f", ffprobe="p") == {
        "dynamic_captions": "no caption plan beside the movie"
    }


def test_changed_evidence_fails_every_stage(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths, index = published(tmp_path, monkeypatch)
    (paths["steps"] / "finish.json").write_text("{}", encoding="utf-8")
    failures = completion.assess(index, delivery=paths["delivery"], root=paths["root"])
    assert set(failures) == set(REQUIRED_STAGES)
    assert "changed after publication" in failures["silence_cut"]


@pytest.mark.parametrize(
    ("change", "message"),
    [
        ({"backend": "x"}, "Unsupported"),
        ({"recording": "replay-1"}, "another recording"),
        ({"configurationSHA256": "0"}, "edit configuration changed"),
    ],
)
def test_foreign_evidence_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, change: dict[str, object], message: str
) -> None:
    paths, index = published(tmp_path, monkeypatch)
    data = json.loads(index.read_text(encoding="utf-8"))
    index.write_text(json.dumps({**data, **change}), encoding="utf-8")
    failures = completion.assess(index, delivery=paths["delivery"], root=paths["root"])
    assert message in failures["bgm_removal"]


def test_a_changed_or_other_movie_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths, index = published(tmp_path, monkeypatch)
    other = tmp_path / "other.mp4"
    other.write_bytes(b"x")
    assert (
        "another movie"
        in completion.assess(index, delivery=other, root=paths["root"])["transcription"]
    )
    with paths["movie"].open("ab") as stream:
        stream.write(b"more")
    failures = completion.assess(index, delivery=paths["delivery"], root=paths["root"])
    assert "changed after publication" in failures["transcription"]


def test_the_cli_publishes(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    paths = recording(tmp_path, monkeypatch)
    arguments = [str(paths[k]) for k in ("root", "movie", "steps", "candidate")]
    assert completion.main(["publish", *arguments, "--revision", "r1"]) == 0
    assert json.loads(capsys.readouterr().out)["delivery"].endswith("-review.mp4")


def test_probe_and_decode_run_the_tools(tmp_path: Path) -> None:
    movie = tmp_path / "m.mp4"
    movie.write_bytes(b"not a movie")
    with pytest.raises(subprocess.CalledProcessError):
        completion.probe(movie, "ffprobe")


def test_a_caption_free_source_passes_only_its_verified_survey(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    verdicts = iter((None, "the caption absence summary is missing or changed"))
    monkeypatch.setattr("replay.caption_absence.verify", lambda _pinned: next(verdicts))
    cleanup: dict[str, object] = {"captionFree": {"path": "/s.json", "sha256": "a"}}
    assert completion.mask_failure(cleanup) is None
    assert completion.mask_failure(cleanup) == "the caption absence summary is missing or changed"


def keep_music(steps: Path, *, finish_intent: str = "kept by request") -> None:
    """Compose and finish receipts that record delivering the original mix."""
    declared = {"source": "original-mix", "intent": "kept by request", "tempo": {"path": "/m"}}
    (steps / "compose.json").write_text(
        json.dumps({"step": "compose", "outputs": {"deliveryAudio": declared}}), "utf-8"
    )
    finished = {**declared, "intent": finish_intent}
    (steps / "finish.json").write_text(
        json.dumps({"step": "finish", "outputs": {"deliveryAudio": finished}}), "utf-8"
    )


def test_kept_music_passes_only_with_its_declared_intent_named_in_the_index(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = recording(tmp_path, monkeypatch)
    keep_music(paths["steps"])
    _, index = completion.publish(
        paths["root"], movie=paths["movie"], steps=paths["steps"],
        candidate=paths["candidate"], revision="pipeline-abc",
    )  # fmt: skip
    good_movie(monkeypatch)
    data = json.loads(index.read_text(encoding="utf-8"))
    assert data["declaredIntents"] == {
        "bgm_removal": {"source": "original-mix", "intent": "kept by request"}
    }
    assert completion.stage_failures(data, ffmpeg="f", ffprobe="p") == {}
    del data["declaredIntents"]
    assert completion.stage_failures(data, ffmpeg="f", ffprobe="p") == {
        "bgm_removal": "the index does not name the declared music intent"
    }


def test_kept_music_whose_receipts_disagree_fails(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths, index = published(tmp_path, monkeypatch)
    keep_music(paths["steps"], finish_intent=" ")
    good_movie(monkeypatch)
    failures = completion.stage_failures(
        json.loads(index.read_text(encoding="utf-8")), ffmpeg="f", ffprobe="p"
    )
    assert failures == {
        "bgm_removal": "compose and finish disagree on the declared delivered audio"
    }
    with pytest.raises(ReplayError, match="disagree on the declared delivered audio"):
        completion.publish(
            paths["root"], movie=paths["movie"], steps=paths["steps"],
            candidate=paths["candidate"], revision="pipeline-def",
        )  # fmt: skip
