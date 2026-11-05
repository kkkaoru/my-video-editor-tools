import hashlib
import json
import subprocess
from pathlib import Path

import pytest

from replay import caption_faces, caption_position
from replay.caption_plan import rows
from replay.core import ReplayError
from replay.recordings import record

FIXTURE = Path(__file__).parent / "fixtures" / "caption-faces-v1.json"
VIDEO = {
    "width": 1080, "height": 1920, "r_frame_rate": "30/1", "nb_frames": "10",
    "start_time": "0.000000",
}  # fmt: skip


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def put(path: Path, value: object) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value), encoding="utf-8")
    return path


def pin(root: Path, step: str, key: str, path: Path) -> None:
    put(root / "steps" / f"{step}.json",
        {"outputs": {key: {"path": str(path), "sha256": digest(path)}}})  # fmt: skip


def recording(tmp_path: Path) -> tuple[Path, Path]:
    """12 source frames at 30 fps (pts in 1/30 s); cuts keep source 0-3 and 6-11 (10 frames);
    the track (540x960, scaled 2x onto the 1080x1920 delivery) samples every second frame."""
    root, work = tmp_path / "rec", tmp_path / "rec" / "rec"
    movie = tmp_path / "delivery" / "d-review.mp4"
    movie.parent.mkdir(parents=True)
    movie.write_bytes(b"movie")
    put(root / "pipeline.json", {"ffprobe": "probe"})
    put(root / "steps" / "publish.json", {"outputs": {"delivery": str(movie)}})
    work.mkdir(parents=True)
    (work / "source.sha256").write_text(f"{'a' * 64}  /source.mp4\n", encoding="utf-8")
    put(work / "video-clock.json", {
        "streams": [{"time_base": "1/30"}],
        "frames": [{"pts": index, "duration": 1} for index in range(12)],
    })  # fmt: skip
    rows = [
        {"frame": 0, "pts": 0, "face": [10, 20, 110, 220, 0.91234], "faceCount": 1},
        {"frame": 2, "pts": 2, "face": None, "faceCount": 0},
        {"frame": 4, "pts": 4, "face": [10, 20, 110, 220, 0.9], "faceCount": 1},
        {"frame": 6, "pts": 6, "face": [30, 40, 90, 100, 0.6], "faceCount": 2},
        {"frame": 10, "pts": 10, "face": [1.234, 2.345, 3.456, 4.567, 0.5], "faceCount": 1},
    ]  # fmt: skip
    track = put(work / "yolo-track-v2.json", {
        "protocol": "yolo-face-caption-track-v2", "provenance": "yolo",
        "weights": {"path": "/models/face.pt", "sha256": "b" * 64},
        "source": {"path": "/source.mp4", "sha256": "a" * 64},
        "width": 540, "height": 960, "timeBase": "1/30", "frames": rows,
    })  # fmt: skip
    pin(root, "yolo-track", "track", track)
    cuts = put(work / "cut-plan-v1" / "supported-cuts.json", {"fps": 30, "cuts": [
        {"source_start_frame": 0, "source_end_frame": 4, "output_start_frame": 0},
        {"source_start_frame": 6, "source_end_frame": 12, "output_start_frame": 4},
    ]})  # fmt: skip
    pin(root, "cut-plan", "cuts", cuts)
    return work, movie


def ffprobe(monkeypatch: pytest.MonkeyPatch, stream: dict[str, object]) -> None:
    def run(command: list[str], **_: object) -> subprocess.CompletedProcess[str]:
        return subprocess.CompletedProcess(command, 0, json.dumps({"streams": [stream]}), "")

    monkeypatch.setattr(subprocess, "run", run)


def test_fixture_is_a_valid_face_track() -> None:
    faces = caption_faces.validate(json.loads(FIXTURE.read_text(encoding="utf-8")))
    assert faces["width"] == 1920


def test_build_puts_every_shown_sample_on_the_delivery_clock(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path)
    ffprobe(monkeypatch, VIDEO)
    faces = caption_faces.build(work, movie)
    assert faces == {
        "version": 1, "clock": "delivery", "rate": 30, "width": 1080, "height": 1920,
        "source": {"model": "face.pt", "modelSha256": "b" * 64, "file": "yolo-track-v2.json",
                   "sha256": digest(work / "yolo-track-v2.json")},
        "samples": [
            {"frame": 0, "faceCount": 1,
             "box": {"x": 20.0, "y": 40.0, "w": 200.0, "h": 400.0, "score": 0.9123}},
            {"frame": 2, "faceCount": 0, "box": None},
            {"frame": 4, "faceCount": 2,
             "box": {"x": 60.0, "y": 80.0, "w": 120.0, "h": 120.0, "score": 0.6}},
            {"frame": 8, "faceCount": 1,
             "box": {"x": 2.47, "y": 4.69, "w": 4.44, "h": 4.44, "score": 0.5}},
        ],
    }  # fmt: skip


def test_write_publishes_once_and_read_returns_it(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path)
    ffprobe(monkeypatch, VIDEO)
    faces = caption_faces.build(work, movie)
    assert caption_faces.read(movie) is None
    target = caption_faces.write(faces, movie)
    assert target == movie.with_name("d-review.faces.json")
    assert caption_faces.write(faces, movie) == target
    assert caption_faces.read(movie) == faces
    assert target.stat().st_mode & 0o777 == 0o600


def test_build_refuses_a_changed_track(tmp_path: Path) -> None:
    work, movie = recording(tmp_path)
    (work / "yolo-track-v2.json").write_text("{}", encoding="utf-8")
    with pytest.raises(ReplayError, match="differs from the yolo-track receipt's pin"):
        caption_faces.build(work, movie)


def test_build_refuses_an_untimed_track(tmp_path: Path) -> None:
    work, movie = recording(tmp_path)
    track = put(work / "yolo-track-v2.json", {"protocol": "yolo-face-caption-track-v1"})
    pin(work.parent, "yolo-track", "track", track)
    with pytest.raises(ReplayError, match="Not a timestamped"):
        caption_faces.build(work, movie)


def test_build_refuses_a_track_of_another_source(tmp_path: Path) -> None:
    work, movie = recording(tmp_path)
    (work / "source.sha256").write_text(f"{'c' * 64}  /source.mp4\n", encoding="utf-8")
    with pytest.raises(ReplayError, match="of another source"):
        caption_faces.build(work, movie)


def test_build_refuses_another_time_base(tmp_path: Path) -> None:
    work, movie = recording(tmp_path)
    put(work / "video-clock.json", {"streams": [{"time_base": "1/90000"}],
                                    "frames": [{"pts": 0, "duration": 3000}]})  # fmt: skip
    with pytest.raises(ReplayError, match="other time bases"):
        caption_faces.build(work, movie)


def test_build_refuses_another_length(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    work, movie = recording(tmp_path)
    ffprobe(monkeypatch, {**VIDEO, "nb_frames": "11"})
    with pytest.raises(ReplayError, match="not the cut plan's length and rate"):
        caption_faces.build(work, movie)


def test_build_refuses_another_shape(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    work, movie = recording(tmp_path)
    ffprobe(monkeypatch, {**VIDEO, "height": 1080})
    with pytest.raises(ReplayError, match="not the delivery canvas's shape"):
        caption_faces.build(work, movie)


def test_an_even_crop_width_fits_the_canvas_as_composition_does(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path)
    track = json.loads((work / "yolo-track-v2.json").read_text(encoding="utf-8"))
    pin(work.parent, "yolo-track", "track",
        put(work / "yolo-track-v2.json", {**track, "width": 608, "height": 1080}))  # fmt: skip
    ffprobe(monkeypatch, VIDEO)
    faces = caption_faces.build(work, movie)
    assert record(rows(faces["samples"], "samples")[0]["box"]) == {
        "x": 17.76, "y": 36.32, "w": 177.63, "h": 355.26, "score": 0.9123,
    }  # fmt: skip


def test_a_shape_two_source_pixels_off_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path)
    track = json.loads((work / "yolo-track-v2.json").read_text(encoding="utf-8"))
    pin(work.parent, "yolo-track", "track",
        put(work / "yolo-track-v2.json", {**track, "width": 610, "height": 1080}))  # fmt: skip
    ffprobe(monkeypatch, VIDEO)
    with pytest.raises(ReplayError, match="not the delivery canvas's shape"):
        caption_faces.build(work, movie)


def test_build_refuses_a_frame_off_the_clock(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path)
    track = json.loads((work / "yolo-track-v2.json").read_text(encoding="utf-8"))
    track["frames"] = [{"frame": 2, "pts": 3, "face": None, "faceCount": 0}]
    pin(work.parent, "yolo-track", "track", put(work / "yolo-track-v2.json", track))
    ffprobe(monkeypatch, VIDEO)
    with pytest.raises(ReplayError, match="YOLO frame 2 is not the frame clock's"):
        caption_faces.build(work, movie)


def test_build_refuses_a_malformed_face(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    work, movie = recording(tmp_path)
    track = json.loads((work / "yolo-track-v2.json").read_text(encoding="utf-8"))
    track["frames"] = [{"frame": 0, "pts": 0, "face": [1, 2, 3], "faceCount": 1}]
    pin(work.parent, "yolo-track", "track", put(work / "yolo-track-v2.json", track))
    ffprobe(monkeypatch, VIDEO)
    with pytest.raises(ReplayError, match=r"\[x1, y1, x2, y2, score\]"):
        caption_faces.build(work, movie)


def test_build_refuses_gapped_cuts(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    work, movie = recording(tmp_path)
    cuts = put(work / "cut-plan-v1" / "supported-cuts.json", {"fps": 30, "cuts": [
        {"source_start_frame": 0, "source_end_frame": 4, "output_start_frame": 1},
    ]})  # fmt: skip
    pin(work.parent, "cut-plan", "cuts", cuts)
    with pytest.raises(ReplayError, match="output frames are not contiguous"):
        caption_faces.build(work, movie)


def test_validate_refuses_another_clock() -> None:
    faces = json.loads(FIXTURE.read_text(encoding="utf-8"))
    with pytest.raises(ReplayError, match="version 1"):
        caption_faces.validate({**faces, "clock": "source"})


def test_validate_refuses_a_zero_rate() -> None:
    faces = json.loads(FIXTURE.read_text(encoding="utf-8"))
    with pytest.raises(ReplayError, match="rate must be positive"):
        caption_faces.validate({**faces, "rate": 0})


def test_validate_refuses_an_incomplete_source() -> None:
    faces = json.loads(FIXTURE.read_text(encoding="utf-8"))
    with pytest.raises(ReplayError, match="source has exactly"):
        caption_faces.validate({**faces, "source": {}})


def test_validate_refuses_unordered_samples() -> None:
    faces = json.loads(FIXTURE.read_text(encoding="utf-8"))
    samples = faces["samples"]
    with pytest.raises(ReplayError, match="frame 0 is out of order or inconsistent"):
        caption_faces.validate({**faces, "samples": [samples[1], samples[0]]})


def test_validate_refuses_a_face_count_without_a_box() -> None:
    faces = json.loads(FIXTURE.read_text(encoding="utf-8"))
    with pytest.raises(ReplayError, match="frame 3 is out of order or inconsistent"):
        caption_faces.validate({**faces, "samples": [{"frame": 3, "faceCount": 1, "box": None}]})


def test_validate_refuses_an_extra_sample_key() -> None:
    faces = json.loads(FIXTURE.read_text(encoding="utf-8"))
    with pytest.raises(ReplayError, match="A face sample has exactly"):
        caption_faces.validate({**faces, "samples": [{"frame": 3}]})


def test_validate_refuses_an_incomplete_box() -> None:
    faces = json.loads(FIXTURE.read_text(encoding="utf-8"))
    with pytest.raises(ReplayError, match="A face box has exactly"):
        caption_faces.validate(
            {**faces, "samples": [{"frame": 3, "faceCount": 1, "box": {"x": 1}}]}
        )


def test_validate_refuses_an_empty_box() -> None:
    faces = json.loads(FIXTURE.read_text(encoding="utf-8"))
    empty = {"x": 1, "y": 1, "w": 0, "h": 1, "score": 0.5}
    with pytest.raises(ReplayError, match="positive size"):
        caption_faces.validate({**faces, "samples": [{"frame": 3, "faceCount": 1, "box": empty}]})


def test_cli_faces_build_writes_beside_the_movie(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    work, movie = recording(tmp_path)
    ffprobe(monkeypatch, VIDEO)
    assert caption_position.main(["faces", "build", str(work), str(movie)]) == 0
    assert capsys.readouterr().out == f"{movie.with_name('d-review.faces.json')}\n"


V2 = Path(__file__).parent / "fixtures" / "caption-faces-v2.json"


def test_v2_fixture_is_a_valid_face_track() -> None:
    faces = caption_faces.validate(json.loads(V2.read_text(encoding="utf-8")))
    assert faces["version"] == 2


def test_v2_validate_refuses_version_1_samples() -> None:
    faces = json.loads(V2.read_text(encoding="utf-8"))
    with pytest.raises(ReplayError, match="version-2 face sample has exactly"):
        caption_faces.validate({**faces, "samples": [{"frame": 3, "faceCount": 0, "box": None}]})


def test_v2_validate_refuses_unordered_samples() -> None:
    faces = json.loads(V2.read_text(encoding="utf-8"))
    samples = [{"frame": 5, "boxes": []}, {"frame": 5, "boxes": []}]
    with pytest.raises(ReplayError, match="frame 5 is out of order"):
        caption_faces.validate({**faces, "samples": samples})


def test_v2_validate_refuses_an_empty_box() -> None:
    faces = json.loads(V2.read_text(encoding="utf-8"))
    empty = {"x": 1, "y": 1, "w": 1, "h": 0, "score": 0.5}
    with pytest.raises(ReplayError, match="positive size"):
        caption_faces.validate({**faces, "samples": [{"frame": 1, "boxes": [empty]}]})


def test_a_track_with_every_face_builds_a_version_2_face_track(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path)
    track = json.loads((work / "yolo-track-v2.json").read_text(encoding="utf-8"))
    track["protocol"] = "yolo-face-caption-track-v3"
    track["frames"] = [
        {"frame": 0, "pts": 0, "face": [10, 20, 110, 220, 0.9], "faceCount": 2,
         "faces": [[10, 20, 110, 220, 0.9], [200, 20, 300, 220, 0.4]]},
        {"frame": 4, "pts": 4, "face": None, "faceCount": 0, "faces": []},
        {"frame": 8, "pts": 8, "face": None, "faceCount": 0, "faces": []},
    ]  # fmt: skip
    pin(work.parent, "yolo-track", "track", put(work / "yolo-track-v2.json", track))
    ffprobe(monkeypatch, VIDEO)
    faces = caption_faces.build(work, movie)
    assert faces["version"] == 2
    assert faces["samples"] == [
        {"frame": 0, "boxes": [
            {"x": 20.0, "y": 40.0, "w": 200.0, "h": 400.0, "score": 0.9},
            {"x": 400.0, "y": 40.0, "w": 200.0, "h": 400.0, "score": 0.4},
        ]},
        {"frame": 6, "boxes": []},
    ]  # fmt: skip


def test_a_track_with_malformed_faces_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    work, movie = recording(tmp_path)
    track = json.loads((work / "yolo-track-v2.json").read_text(encoding="utf-8"))
    track["protocol"] = "yolo-face-caption-track-v3"
    track["frames"] = [{"frame": 0, "pts": 0, "face": None, "faceCount": 0, "faces": None}]
    pin(work.parent, "yolo-track", "track", put(work / "yolo-track-v2.json", track))
    ffprobe(monkeypatch, VIDEO)
    with pytest.raises(ReplayError, match="faces must be a list"):
        caption_faces.build(work, movie)
