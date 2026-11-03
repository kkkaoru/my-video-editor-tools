import json
import subprocess
import sys
from pathlib import Path
from typing import Any

SCRIPT = Path(__file__).resolve().parents[1] / "web-viewer" / "tools" / "build_catalog.py"
KASUMI = "UCEmlNV5-edGhNWYDMeGDgWw"
SRT = "1\n00:00:00,000 --> 00:00:01,000\nこんにちは\n"


def _youtube(library: Path, originals: Path, video_id: str, channel: str | None) -> None:
    stamp = "2026-10-22_00-00-20_JST_"
    delivery = library / f"youtube-{video_id}" / "delivery"
    delivery.mkdir(parents=True)
    (delivery / f"{stamp}youtube-{video_id}-review.mp4").write_bytes(b"m")
    (delivery / f"{stamp}youtube-{video_id}-review.srt").write_text(SRT, encoding="utf-8")
    original = originals / f"{stamp}youtube-{video_id}"
    original.mkdir(parents=True)
    (original / f"{stamp}youtube-{video_id}.mp4").write_bytes(b"o")
    if channel is not None:
        (original / "channel.json").write_text(channel, encoding="utf-8")


def test_youtube_videos_carry_their_channel_id(tmp_path: Path) -> None:
    library, originals, out = tmp_path / "lib", tmp_path / "yt", tmp_path / "out"
    _youtube(library, originals, "aaaaaaaaaaa", f'{{"channel_id":"{KASUMI}","channel":"A"}}')
    _youtube(library, originals, "bbbbbbbbbbb", '{"channel_id":"UCshort"}')
    _youtube(library, originals, "ccccccccccc", "not json")
    _youtube(library, originals, "ddddddddddd", None)
    tiktok = library / "replay-1" / "delivery"
    tiktok.mkdir(parents=True)
    (tiktok / "2026-10-01_00-00-00_JST_replay-1-review.mp4").write_bytes(b"m")
    (tiktok / "2026-10-01_00-00-00_JST_replay-1-review.srt").write_text(SRT, encoding="utf-8")
    subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--root",
            str(library),
            "--out",
            str(out),
            "--no-duration",
            "--streamer",
            "既定",
            "--youtube-originals",
            str(originals),
        ],
        check=True,
        capture_output=True,
    )
    catalog = json.loads((out / "catalog.json").read_text(encoding="utf-8"))
    channels = {
        video["title"]: (video["streamer"], video.get("channelId")) for video in catalog["videos"]
    }
    assert channels == {
        "youtube-aaaaaaaaaaa": ("既定", "UCEmlNV5-edGhNWYDMeGDgWw"),
        "youtube-bbbbbbbbbbb": ("既定", None),
        "youtube-ccccccccccc": ("既定", None),
        "youtube-ddddddddddd": ("既定", None),
        "replay-1": ("既定", None),
    }
    assert catalog["issues"] == [
        "no channel: youtube-bbbbbbbbbbb (default streamer)",
        "no channel: youtube-ccccccccccc (default streamer)",
        "no channel: youtube-ddddddddddd (default streamer)",
    ]


def test_kept_youtube_videos_gain_their_channel_id(tmp_path: Path) -> None:
    library, keep, out = tmp_path / "lib", tmp_path / "keep", tmp_path / "out"
    library.mkdir()
    keep.mkdir()
    original = tmp_path / "yt" / "o"
    original.mkdir(parents=True)
    (original / "channel.json").write_text(
        '{"channel_id":"UCEmlNV5-edGhNWYDMeGDgWw"}', encoding="utf-8"
    )
    published = {
        "videos": [
            {
                "id": "k",
                "title": "youtube-kkkkkkkkkkk",
                "movie": "k.mp4",
                "original": {"localPath": str(original / "o.mp4")},
                "broadcastStartJST": None,
                "size": 1,
            }
        ]
    }
    (keep / "catalog.json").write_text(json.dumps(published), encoding="utf-8")
    (keep / "k.json").write_text('{"id":"k","cues":[]}', encoding="utf-8")
    subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--root",
            str(library),
            "--out",
            str(out),
            "--no-duration",
            "--streamer",
            "既定",
            "--keep",
            str(keep),
        ],
        check=True,
        capture_output=True,
    )
    catalog = json.loads((out / "catalog.json").read_text(encoding="utf-8"))
    assert catalog["videos"] == [
        {
            "id": "k",
            "title": "youtube-kkkkkkkkkkk",
            "movie": "k.mp4",
            "original": {"localPath": str(original / "o.mp4")},
            "broadcastStartJST": None,
            "size": 1,
            "kept": True,
            "channelId": "UCEmlNV5-edGhNWYDMeGDgWw",
        }
    ]


def test_a_retired_revisions_transcript_does_not_hide_the_playable_delivery(
    tmp_path: Path,
) -> None:
    library, out = tmp_path / "lib", tmp_path / "out"
    stem = "2026-10-08_00-00-00_JST_replay-2-review"
    for revision in ("old", "new"):
        delivery = library / "replay-2" / "revisions" / revision / "delivery"
        delivery.mkdir(parents=True)
        (delivery / f"{stem}.srt").write_text(SRT, encoding="utf-8")
    (library / "replay-2" / "revisions" / "new" / "delivery" / f"{stem}.mp4").write_bytes(b"m")
    subprocess.run(
        [sys.executable, str(SCRIPT), "--root", str(library), "--out", str(out),
         "--no-duration", "--streamer", "既定"],
        check=True,
        capture_output=True,
    )  # fmt: skip
    catalog = json.loads((out / "catalog.json").read_text(encoding="utf-8"))
    assert [Path(video["localPath"]).parent.parent.name for video in catalog["videos"]] == ["new"]
    assert catalog["issues"] == []


def test_an_offloaded_delivery_publishes_fresh_data_without_its_movie_path(
    tmp_path: Path,
) -> None:
    library, keep, out = tmp_path / "lib", tmp_path / "keep", tmp_path / "out"
    stem = "2026-10-01_00-00-00_JST_replay-1-review"
    delivery = library / "replay-1" / "delivery"
    delivery.mkdir(parents=True)
    (delivery / f"{stem}.srt").write_text(SRT, encoding="utf-8")
    summaries = delivery / f"{stem}.srt.summaries"
    summaries.mkdir()
    summary = {"version": 1, "overview": "新しい要約",
               "sections": [{"startSeconds": 0, "endSeconds": 1, "text": "挨拶"}]}  # fmt: skip
    (summaries / "current.json").write_text(json.dumps(summary), encoding="utf-8")
    movie = str(delivery / f"{stem}.mp4")
    record = {"version": 1, "files": [{"role": "delivery", "localPath": movie, "bytes": 9}]}
    (library / "replay-1" / "offload.json").write_text(json.dumps(record), encoding="utf-8")
    keep.mkdir()
    published = {"videos": [{"id": stem, "title": "replay-1", "movie": f"{stem}.mp4",
                             "duration": 12.5}]}  # fmt: skip
    (keep / "catalog.json").write_text(json.dumps(published), encoding="utf-8")
    (keep / f"{stem}.json").write_text(f'{{"id":"{stem}","cues":[]}}', encoding="utf-8")
    subprocess.run(
        [sys.executable, str(SCRIPT), "--root", str(library), "--out", str(out),
         "--streamer", "既定", "--keep", str(keep)],
        check=True, capture_output=True,
    )  # fmt: skip
    catalog = json.loads((out / "catalog.json").read_text(encoding="utf-8"))
    assert catalog["videos"] == [
        {
            "id": stem, "streamer": "既定", "title": "replay-1",
            "broadcastStartJST": "2026-10-01 00:00:00 JST", "movie": f"{stem}.mp4",
            "size": 9, "original": None, "duration": 12.5,
            "transcriptPath": str(delivery / f"{stem}.srt"), "cueCount": 1,
            "hasSummary": True, "localPath": None,
        }
    ]  # fmt: skip
    assert catalog["issues"] == []
    data = json.loads((out / f"{stem}.json").read_text(encoding="utf-8"))
    assert data["summary"] == {
        "overview": "新しい要約", "sections": [{"start": 0, "end": 1, "text": "挨拶"}]
    }  # fmt: skip


def test_an_unedited_youtube_original_is_published_alone(tmp_path: Path) -> None:
    library, originals, out = tmp_path / "lib", tmp_path / "yt", tmp_path / "out"
    library.mkdir()
    _youtube(library, originals, "aaaaaaaaaaa", f'{{"channel_id":"{KASUMI}"}}')
    alone = originals / "2026-11-02_23-00-00_JST_youtube-eeeeeeeeeee"
    alone.mkdir()
    (alone / "2026-11-02_23-00-00_JST_youtube-eeeeeeeeeee.mp4").write_bytes(b"orig")
    (alone / "channel.json").write_text(f'{{"channel_id":"{KASUMI}"}}', encoding="utf-8")
    empty = originals / "2026-11-01_00-00-00_JST_youtube-fffffffffff"
    empty.mkdir()
    (originals / ".2026-11-03_00-00-00_JST_youtube-ggggggggggg.partial").mkdir()
    subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--root",
            str(library),
            "--out",
            str(out),
            "--no-duration",
            "--streamer",
            "既定",
            "--youtube-originals",
            str(originals),
        ],
        check=True,
        capture_output=True,
    )
    catalog = json.loads((out / "catalog.json").read_text(encoding="utf-8"))
    assert [video["title"] for video in catalog["videos"]] == [
        "youtube-eeeeeeeeeee [未編集・字幕なし]",
        "youtube-aaaaaaaaaaa",
    ]
    assert catalog["videos"][0] == {
        "id": "2026-11-02_23-00-00_JST_youtube-eeeeeeeeeee",
        "streamer": "既定",
        "title": "youtube-eeeeeeeeeee [未編集・字幕なし]",
        "broadcastStartJST": "2026-11-02 23:00:00 JST",
        "movie": None,
        "size": 4,
        "original": {
            "movie": "2026-11-02_23-00-00_JST_youtube-eeeeeeeeeee.mp4",
            "size": 4,
            "localPath": str(alone / "2026-11-02_23-00-00_JST_youtube-eeeeeeeeeee.mp4"),
        },
        "duration": None,
        "transcriptPath": str(out / "2026-11-02_23-00-00_JST_youtube-eeeeeeeeeee.json"),
        "cueCount": 0,
        "hasSummary": False,
        "localPath": None,
        "channelId": "UCEmlNV5-edGhNWYDMeGDgWw",
    }
    assert catalog["issues"] == ["no single original: 2026-11-01_00-00-00_JST_youtube-fffffffffff"]
    assert json.loads(
        (out / "2026-11-02_23-00-00_JST_youtube-eeeeeeeeeee.json").read_text(encoding="utf-8")
    ) == {"id": "2026-11-02_23-00-00_JST_youtube-eeeeeeeeeee", "cues": [], "summary": None}


def test_a_deliverys_caption_plan_is_published_with_its_icon_inlined(tmp_path: Path) -> None:
    fixtures = Path(__file__).resolve().parent / "fixtures"
    library, out = tmp_path / "lib", tmp_path / "out"
    stems = {"1": "2026-10-01_00-00-00_JST_replay-1-review",
             "2": "2026-10-02_00-00-00_JST_replay-2-review"}  # fmt: skip
    for number, stem in stems.items():
        delivery = library / f"replay-{number}" / "delivery"
        delivery.mkdir(parents=True)
        (delivery / f"{stem}.mp4").write_bytes(b"m")
        (delivery / f"{stem}.srt").write_text(SRT, encoding="utf-8")
        plan = json.loads((fixtures / "caption-plan-v1.json").read_text(encoding="utf-8"))
        # replay-2's plan names another movie: never published for this one.
        plan["movie"] = f"{stem}.mp4" if number == "1" else "other.mp4"
        (delivery / f"{stem}.captions.json").write_text(json.dumps(plan), encoding="utf-8")
        icon = (fixtures / "synthetic-review.nameplate.png").read_bytes()
        (delivery / "synthetic-review.nameplate.png").write_bytes(icon)
        for sibling in ("caption-layout-v1.json", "caption-faces-v1.json"):
            suffix = ".caption-layout.json" if "layout" in sibling else ".faces.json"
            (delivery / f"{stem}{suffix}").write_bytes((fixtures / sibling).read_bytes())
    subprocess.run(
        [sys.executable, str(SCRIPT), "--root", str(library), "--out", str(out),
         "--no-duration", "--streamer", "既定"],
        check=True, capture_output=True,
    )  # fmt: skip
    catalog = json.loads((out / "catalog.json").read_text(encoding="utf-8"))
    captions = {video["id"]: video.get("captions") for video in catalog["videos"]}
    assert captions == {
        stems["1"]: {"key": f"captions/{stems['1']}.json", "burned": False},
        stems["2"]: None,
    }
    assert catalog["issues"] == [f"caption plan: {stems['2']} (not this delivery's plan)"]
    published = json.loads((out / f"{stems['1']}.captions.json").read_text(encoding="utf-8"))
    assert published["nameplate"]["iconData"].startswith("data:image/png;base64,iVBOR")
    assert len(published["titles"]) == 3
    assert published["layout"] == {"version": 1, "mode": "aboveFace", "offsetY": -40}
    assert published["faceTops"] == [[700.0, "face"], [700.0, "held"], [760.0, "face"]]
    assert not (out / f"{stems['2']}.captions.json").exists()


def test_precomputed_face_tops_follow_the_caption_layout_rule(tmp_path: Path) -> None:
    import importlib.util
    from fractions import Fraction

    from replay import caption_layout_file
    from replay.caption_fixed_y import UnknownPerson, person_top_with_hold

    spec = importlib.util.spec_from_file_location("build_catalog", SCRIPT)
    assert spec is not None and spec.loader is not None
    build_catalog = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(build_catalog)
    fixtures = Path(__file__).resolve().parent / "fixtures"
    plan = json.loads((fixtures / "caption-plan-v1.json").read_text(encoding="utf-8"))
    faces = json.loads((fixtures / "caption-faces-v1.json").read_text(encoding="utf-8"))
    (tmp_path / "m.faces.json").write_text(json.dumps(faces), encoding="utf-8")
    expected = json.loads(
        (fixtures / "caption-layout-expected-v1.json").read_text(encoding="utf-8")
    )
    for case in expected["cases"]:
        layout = case["layout"]
        options = caption_layout_file.settings(layout)
        series = caption_layout_file.face_tops(faces, options["minScore"])
        canonical: list[list[float | str] | None] = []
        for title in plan["titles"]:
            try:
                top = person_top_with_hold(
                    series, Fraction(title["startFrame"]), Fraction(title["endFrame"]),
                    maximum_drift=options["maxDrift"],
                )  # fmt: skip
                canonical.append([top.top, "held" if top.held else "face"])
            except UnknownPerson:
                canonical.append(None)
        assert build_catalog.caption_face_tops(str(tmp_path / "m"), plan, layout) == canonical


# The same values are BUILT_FACE_TOPS_V2 in web-viewer/src/captions.test.ts, which resolves them to
# tests/fixtures/caption-layout-expected-v2.json.
BUILT_FACE_TOPS_V2 = {
    "absent": [
        [480.0, "face"],
        [400.0, "held"],
        [300.0, "face"],
        [300.0, "face"],
        [300.0, "face"],
        None,
        [600.0, "face"],
    ],
    "fixture-layout": [
        [480.0, "face"],
        [400.0, "held"],
        [300.0, "face"],
        [300.0, "face"],
        [300.0, "face"],
        None,
        [600.0, "face"],
    ],
    "bottom-all": [
        [480.0, "face"],
        [400.0, "held"],
        [300.0, "face"],
        [300.0, "face"],
        [300.0, "face"],
        None,
        [600.0, "face"],
    ],
    "above-face-all": [
        [480.0, "face"],
        [400.0, "held"],
        [300.0, "face"],
        [300.0, "face"],
        [300.0, "face"],
        None,
        [600.0, "face"],
    ],
    "above-face-v1-faces": [
        [700.0, "face"],
        [700.0, "held"],
        [700.0, "held"],
        [760.0, "face"],
        [760.0, "face"],
        None,
        None,
    ],
    "per-speaker-offsets": [
        [480.0, "face"],
        [400.0, "held"],
        [300.0, "face"],
        [300.0, "face"],
        [300.0, "face"],
        None,
        [600.0, "face"],
    ],
}


def canonical_face_tops(
    plan: dict[str, Any], layout: dict[str, Any], faces_path: Path
) -> list[list[float | str] | None]:
    """The face top replay.caption_layout_file.resolve reads for each title (its own rule)."""
    from fractions import Fraction

    from replay import caption_layout_file
    from replay.caption_fixed_y import UnknownPerson, person_top_with_hold

    faces = json.loads(faces_path.read_text(encoding="utf-8"))
    lanes = caption_layout_file.bands(plan)
    own = layout.get("speakers", {})
    result: list[list[float | str] | None] = []
    for title in plan["titles"]:
        speaker = title.get("speaker") if lanes else None
        values = caption_layout_file.settings({**layout, **own.get(str(speaker), {})})
        band = lanes[speaker] if speaker is not None and faces["version"] == 2 else None
        series = caption_layout_file.face_tops(faces, values["minScore"], band)
        try:
            top = person_top_with_hold(
                series, Fraction(title["startFrame"]), Fraction(title["endFrame"]),
                maximum_drift=values["maxDrift"],
            )  # fmt: skip
            result.append([top.top, "held" if top.held else "face"])
        except UnknownPerson:
            result.append(None)
    return result


def test_version_2_face_tops_take_each_speakers_lane(tmp_path: Path) -> None:
    import importlib.util

    spec = importlib.util.spec_from_file_location("build_catalog", SCRIPT)
    assert spec is not None and spec.loader is not None
    build_catalog = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(build_catalog)
    fixtures = Path(__file__).resolve().parent / "fixtures"
    plan = json.loads((fixtures / "caption-plan-v2.json").read_text(encoding="utf-8"))
    expected = json.loads(
        (fixtures / "caption-layout-expected-v2.json").read_text(encoding="utf-8")
    )
    tops = {}
    for case in expected["cases"]:
        (tmp_path / "m.faces.json").write_bytes((fixtures / case["faces"]).read_bytes())
        tops[case["name"]] = build_catalog.caption_face_tops(
            str(tmp_path / "m"), plan, case["layout"]
        )
    assert tops == BUILT_FACE_TOPS_V2
    assert tops == {
        case["name"]: canonical_face_tops(plan, case["layout"], fixtures / case["faces"])
        for case in expected["cases"]
    }
    layout = json.loads((fixtures / "caption-layout-v2.json").read_text(encoding="utf-8"))
    (tmp_path / "m.caption-layout.json").write_text(json.dumps(layout), encoding="utf-8")
    assert build_catalog.caption_layout(str(tmp_path / "m"), plan) == layout
    layout["speakers"]["nobody"] = {"mode": "top"}
    (tmp_path / "m.caption-layout.json").write_text(json.dumps(layout), encoding="utf-8")
    try:
        build_catalog.caption_layout(str(tmp_path / "m"), plan)
    except ValueError as error:
        assert str(error) == "invalid caption layout"
    else:
        raise AssertionError("an unknown speaker was accepted")


def test_version_2_speaker_icons_are_inlined_once_checked(tmp_path: Path) -> None:
    import importlib.util

    spec = importlib.util.spec_from_file_location("build_catalog", SCRIPT)
    assert spec is not None and spec.loader is not None
    build_catalog = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(build_catalog)
    fixtures = Path(__file__).resolve().parent / "fixtures"
    stem = "2026-10-01_00-00-00_JST_replay-1-review"
    plan = json.loads((fixtures / "caption-plan-v2.json").read_text(encoding="utf-8"))
    plan["movie"] = f"{stem}.mp4"
    (tmp_path / f"{stem}.captions.json").write_text(json.dumps(plan), encoding="utf-8")
    for side in ("left", "right"):
        name = f"synthetic-multi.speaker-{side}.png"
        (tmp_path / name).write_bytes((fixtures / name).read_bytes())
    out, issues = tmp_path / "out", list[str]()
    out.mkdir()
    entry = build_catalog.caption_plan(
        str(tmp_path / f"{stem}.srt"), f"{stem}.mp4", stem, str(out), issues
    )
    assert (entry, issues) == ({"key": f"captions/{stem}.json", "burned": False}, [])
    published = json.loads((out / f"{stem}.captions.json").read_text(encoding="utf-8"))
    inlined = [s.get("iconData", "")[:26] for s in published["speakers"]]
    assert inlined == ["data:image/png;base64,iVBO", "", "data:image/png;base64,iVBO"]
    (tmp_path / "synthetic-multi.speaker-right.png").write_bytes(b"other")
    assert (
        build_catalog.caption_plan(
            str(tmp_path / f"{stem}.srt"), f"{stem}.mp4", stem, str(out), issues
        )
        is None
    )
    refused = "synthetic-multi.speaker-right.png does not have its recorded SHA-256"
    assert issues == [f"caption plan: {stem} ({refused})"]
