"""Vision lines become caption labels by the cleanup's profile rule; uncertain frames drop."""

import json
from collections.abc import Iterator
from fractions import Fraction
from pathlib import Path
from types import SimpleNamespace

import cv2
import numpy as np
import pytest

from replay import yolo_vision_dataset as vision
from replay.caption_io import FrameObservation
from replay.caption_masks import TextDetection, TextRectangle
from replay.core import ReplayError
from replay.subtitle_cleanup_plan import CaptionSearchProfile
from replay.subtitle_clock import VideoFrameClock
from replay.subtitle_profile import SubtitleBands
from replay.yolo_joint_eval import Prediction

PROFILE = CaptionSearchProfile(SubtitleBands(1080, 1920, (39,), (40,)), (0, 1240, 1080, 100))
WORDS = (
    "おはよう",
    "ありがとう",
    "こんばんは",
    "さようなら",
    "たのしいね",
    "すごいなあ",
    "まってね",
    "いくよ",
    "かわいい",
    "なんでだ",
    "ほんとうに",
    "おやすみ",
)
CLOCK = {"pts": [0, 3000, 6000, 9000], "timeBase": "1/90000", "lastDuration": 3000}


def line(
    text: str,
    y: float,
    *,
    x: float = 400,
    width: float = 280,
    confidence: float = 1.0,
    height: float = 32,
) -> TextDetection:
    return TextDetection(text, confidence, TextRectangle(x, y, width, height))


def frame(*lines: TextDetection, time: float = 0.0) -> FrameObservation:
    return FrameObservation(time, time, 1080, 1920, lines)


def raw(frame_time: float, lines: list[tuple[str, float]]) -> dict[str, object]:
    return {
        "width": 1080,
        "height": 1920,
        "requestedTimeSeconds": frame_time,
        "actualTimeSeconds": frame_time,
        "lines": [
            {
                "text": text,
                "confidence": 1.0,
                "region": {
                    "x": 400 + 7 * len(text),
                    "y": y,
                    "width": 200 + 9 * len(text),
                    "height": 32,
                },
            }
            for text, y in lines
        ],
    }


def write_profile(directory: Path) -> None:
    directory.mkdir(parents=True)
    frames = [raw(index, [(word, 1256.0)]) for index, word in enumerate(WORDS)]
    (directory / "0000.result.json").write_text(json.dumps(frames[:6]), encoding="utf-8")
    (directory / "0001.result.json").write_text(json.dumps(frames[6:]), encoding="utf-8")


def write_work(root: Path) -> Path:
    work = root / "clean"
    write_profile(root / "profile")
    (work / "configs").mkdir(parents=True)
    config = {
        "clock": CLOCK,
        "width": 1080,
        "height": 1920,
        "native": str(root / "native"),
        "source": str(root / "source.mp4"),
        "checkoutRoot": str(root),
        "profileDirectory": str(root / "profile"),
        "ffmpeg": "ffmpeg",
    }
    (work / "configs" / "chunk-00000.json").write_text(json.dumps(config), encoding="utf-8")
    native = work / "state" / "chunk-00000" / "attempt-full" / "ocr" / "child-000" / "native"
    native.mkdir(parents=True)
    (native / "complete.json").write_text("{}", encoding="utf-8")
    block = [raw(0.0, [("ありがとう", 1256.0), ("フォローしてね", 300.0)])]
    (native / "block-0.json").write_text(json.dumps(block), encoding="utf-8")
    (native / "block-0-clocks.json").write_text("[]", encoding="utf-8")
    return work


def stub_ocr(*args: object, **options: object) -> dict[int, FrameObservation]:
    return {2: frame(line("こんばんは", 1256), time=6000 / 90000)}


def stub_frames(source: Path, clock: VideoFrameClock, **options: object) -> Iterator[np.ndarray]:
    return iter([np.zeros((1138, 640, 3), dtype=np.uint8)] * len(clock.pts))


def stub_detect(images: list[np.ndarray]) -> list[Prediction]:
    return [Prediction((((100.0, 100.0, 200.0, 220.0), 0.9),), ())] * len(images)


def fake_result(
    classes: list[float], scores: list[float], boxes: list[list[float]]
) -> SimpleNamespace:
    return SimpleNamespace(
        boxes=SimpleNamespace(
            cls=SimpleNamespace(tolist=lambda: classes),
            conf=SimpleNamespace(tolist=lambda: scores),
            xyxy=SimpleNamespace(tolist=lambda: boxes),
        )
    )


def test_profiled_lines_are_captions_and_the_rest_interface() -> None:
    labels = vision.frame_labels(
        frame(
            line("ありがとう", 1256),
            line("Thank you", 1292, height=24),
            line("フォローしてね", 300),
        ),
        PROFILE,
    )
    assert labels == vision.FrameLabels(
        ((400.0, 1256.0, 680.0, 1288.0), (400.0, 1292.0, 680.0, 1316.0)),
        ((400.0, 300.0, 680.0, 332.0),),
        False,
    )


def test_faint_text_inside_the_caption_bands_is_unreadable() -> None:
    labels = vision.frame_labels(frame(line("ありがとう", 1256, confidence=0.2)), PROFILE)
    assert labels.unreadable is True


def test_symbols_inside_the_caption_bands_stay_readable() -> None:
    labels = vision.frame_labels(frame(line("<<", 1256, confidence=0.2)), PROFILE)
    assert labels.unreadable is False


def test_a_caption_holding_a_gift_fragment_is_marked() -> None:
    bands = SubtitleBands(1080, 1920, (39,), (40,), ("星降りメリー",))
    profile = CaptionSearchProfile(bands, (0, 1240, 1080, 100))
    labels = vision.frame_labels(
        frame(line("ほんとうにりメリー", 1256), line("星降りメリー", 1500)), profile
    )
    assert labels.interface_fragment is True


def test_a_small_button_with_the_same_words_keeps_the_caption() -> None:
    bands = SubtitleBands(1080, 1920, (39,), (40,), ("りがとう",))
    profile = CaptionSearchProfile(bands, (0, 1240, 1080, 100))
    labels = vision.frame_labels(
        frame(line("本当にありがとう", 1256), line("りがとう", 400, height=20)), profile
    )
    assert labels.interface_fragment is False


def test_a_vocabulary_word_without_its_interface_copy_stays_a_caption() -> None:
    bands = SubtitleBands(1080, 1920, (39,), (40,), ("ありがとう",))
    profile = CaptionSearchProfile(bands, (0, 1240, 1080, 100))
    labels = vision.frame_labels(frame(line("本当にありがとう", 1256)), profile)
    assert labels.interface_fragment is False


def test_a_caption_sharing_two_characters_is_not_a_fragment() -> None:
    assert vision.holds_fragment("メリークリスマス", ("星降りメリー",)) is True
    assert vision.holds_fragment("ありがとう", ("星降りメリー", "フォローしてね")) is False
    assert vision.holds_fragment("りメ", ("星降りメリー",)) is False


def test_a_gift_fragment_caption_drops_the_frame() -> None:
    labels = vision.FrameLabels((), (), False, True)
    reason = vision.drop_reason(labels, Prediction((), ()), scale=1.0)
    assert reason == "interfaceFragmentCaption"


def test_a_middling_face_drops_the_frame() -> None:
    labels = vision.FrameLabels((), (), False)
    prediction = Prediction((((0.0, 0.0, 10.0, 10.0), 0.3),), ())
    assert vision.drop_reason(labels, prediction, scale=1.0) == "uncertainFace"


def test_unreadable_band_text_drops_the_frame() -> None:
    labels = vision.FrameLabels((), (), True)
    assert vision.drop_reason(labels, Prediction((), ()), scale=1.0) == "unreadableBandText"


def test_a_detector_caption_box_without_any_vision_line_drops_the_frame() -> None:
    labels = vision.FrameLabels(((0.0, 0.0, 10.0, 10.0),), (), False)
    prediction = Prediction((), ((100.0, 100.0, 120.0, 110.0),))
    assert vision.drop_reason(labels, prediction, scale=1.0) == "unreadCaptionBox"


def test_a_detector_box_on_interface_text_keeps_the_frame() -> None:
    labels = vision.FrameLabels((), ((200.0, 200.0, 240.0, 220.0),), False)
    prediction = Prediction((((0.0, 0.0, 10.0, 10.0), 0.9),), ((100.0, 100.0, 120.0, 110.0),))
    assert vision.drop_reason(labels, prediction, scale=2.0) is None


def test_label_rows_keep_sure_faces_and_scale_captions_into_the_image() -> None:
    labels = vision.FrameLabels(
        ((200.0, 1000.0, 400.0, 1040.0), (900.0, 50.0, 950.0, 60.0)), (), False
    )
    prediction = Prediction((((10.0, 20.0, 110.0, 120.0), 0.9), ((0.0, 0.0, 5.0, 5.0), 0.3)), ())
    rows = vision.label_rows(labels, prediction, scale=2.0, size=(400, 800))
    assert rows == [
        "0 0.150000 0.087500 0.250000 0.125000",
        "1 0.375000 0.637500 0.250000 0.025000",
    ]


def test_existing_observations_map_stored_ocr_onto_the_chunk_clock(tmp_path: Path) -> None:
    work = write_work(tmp_path)
    clock = VideoFrameClock((0, 3000, 6000, 9000), Fraction(1, 90000), 3000)
    found = vision.existing_observations(work / "state/chunk-00000/attempt-full/ocr", clock)
    assert list(found) == [0]
    assert [item.text for item in found[0].detections] == ["ありがとう", "フォローしてね"]


def test_existing_observations_skip_an_incomplete_child(tmp_path: Path) -> None:
    work = write_work(tmp_path)
    (work / "state/chunk-00000/attempt-full/ocr/child-000/native/complete.json").unlink()
    clock = VideoFrameClock((0, 3000), Fraction(1, 90000), 3000)
    assert vision.existing_observations(work / "state/chunk-00000/attempt-full/ocr", clock) == {}


def test_existing_observations_refuse_a_frame_off_the_clock(tmp_path: Path) -> None:
    work = write_work(tmp_path)
    clock = VideoFrameClock((4500, 9000), Fraction(1, 90000), 3000)
    with pytest.raises(ReplayError, match="chunk clock"):
        vision.existing_observations(work / "state/chunk-00000/attempt-full/ocr", clock)


def test_the_manifest_is_parsed(tmp_path: Path) -> None:
    path = tmp_path / "m.json"
    item = {"name": "a", "cleanWork": "w", "role": "train", "chunks": [1, 3], "frameStep": 2}
    path.write_text(json.dumps({"output": "o", "recordings": [item]}), encoding="utf-8")
    recordings, data = vision.parse_manifest(path)
    assert recordings == [vision.Recording("a", Path("w"), "train", (1, 3), 2)]
    assert data["output"] == "o"


@pytest.mark.parametrize(
    ("document", "message"),
    [
        ({}, "recordings list"),
        ({"recordings": [1]}, "must be an object"),
        ({"recordings": [{"name": "a-b", "role": "train"}]}, "plain-identifier"),
        ({"recordings": [{"name": "a", "role": "train", "frameStep": 0}]}, "frameStep"),
        (
            {
                "recordings": [
                    {"name": "a", "role": "test", "cleanWork": "w", "frameStep": 1, "chunks": []}
                ]
            },
            "chunk numbers",
        ),
        (
            {
                "recordings": [
                    {"name": "a", "role": "val", "cleanWork": "w", "frameStep": 1, "chunks": [0]}
                ]
                * 2
            },
            "unique",
        ),
    ],
)
def test_the_manifest_refuses_invalid_entries(
    tmp_path: Path, document: dict[str, object], message: str
) -> None:
    path = tmp_path / "m.json"
    path.write_text(json.dumps(document), encoding="utf-8")
    with pytest.raises(ReplayError, match=message):
        vision.parse_manifest(path)


def test_the_builder_writes_labelled_frames_from_stored_and_new_ocr(tmp_path: Path) -> None:
    work = write_work(tmp_path)
    output = tmp_path / "dataset"
    builder = vision.Builder(output, stub_detect, ocr=stub_ocr, frames=stub_frames)
    counts = builder.recording(vision.Recording("rec", work, "train", (0,), 2))
    assert counts == {"frames": 2, "captionLines": 2, "framesWithCaption": 2, "newOcrFrames": 1}
    assert (output / "labels/rec/rec_00000_002.txt").read_text(encoding="utf-8") == (
        "0 0.234375 0.140598 0.156250 0.105448\n1 0.500000 0.662371 0.259259 0.016663\n"
    )
    assert json.loads((output / "vision/rec/rec_00000_000.json").read_text(encoding="utf-8")) == {
        "canvasScale": 1.6875,
        "captions": [[435.0, 1256.0, 680.0, 1288.0]],
        "interface": [[449.0, 300.0, 712.0, 332.0]],
    }


def test_the_builder_reuses_an_earlier_builds_ocr(tmp_path: Path) -> None:
    work = write_work(tmp_path)
    earlier = tmp_path / "earlier" / "rec" / "chunk-00000" / "child-000" / "native"
    earlier.mkdir(parents=True)
    (earlier / "complete.json").write_text("{}", encoding="utf-8")
    block = [raw(6000 / 90000, [("こんばんは", 1256.0)])]
    (earlier / "block-0.json").write_text(json.dumps(block), encoding="utf-8")
    builder = vision.Builder(
        tmp_path / "dataset",
        stub_detect,
        ocr=stub_ocr,
        frames=stub_frames,
        reuse=tmp_path / "earlier",
    )
    counts = builder.recording(vision.Recording("rec", work, "train", (0,), 2))
    assert counts == {"frames": 2, "captionLines": 2, "framesWithCaption": 2, "newOcrFrames": 0}


def test_the_builder_reads_ocr_of_a_moved_rerun_chunk(tmp_path: Path) -> None:
    work = write_work(tmp_path)
    moved = work / "interrupted" / "chunk-00000-rerun-1" / "attempt-full" / "ocr" / "child-000"
    (moved / "native").mkdir(parents=True)
    (moved / "native" / "complete.json").write_text("{}", encoding="utf-8")
    block = [raw(6000 / 90000, [("こんばんは", 1256.0)])]
    (moved / "native" / "block-0.json").write_text(json.dumps(block), encoding="utf-8")
    builder = vision.Builder(tmp_path / "dataset", stub_detect, ocr=stub_ocr, frames=stub_frames)
    counts = builder.recording(vision.Recording("rec", work, "train", (0,), 2))
    assert counts == {"frames": 2, "captionLines": 2, "framesWithCaption": 2, "newOcrFrames": 0}


def test_main_refuses_a_non_text_reuse(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    manifest = tmp_path / "m.json"
    document = {"output": str(tmp_path / "out"), "recordings": [], "reuseOcr": 3}
    manifest.write_text(json.dumps(document), encoding="utf-8")
    monkeypatch.setattr(vision, "detector", lambda *a, **k: stub_detect)
    with pytest.raises(ReplayError, match="reuseOcr"):
        vision.main(
            [
                "build",
                str(manifest),
                "--weights",
                "w",
                "--weights-sha256",
                "x",
                "--config-directory",
                "c",
                "--work",
                "w",
            ]
        )


def test_the_builder_counts_dropped_frames(tmp_path: Path) -> None:
    work = write_work(tmp_path)
    builder = vision.Builder(
        tmp_path / "dataset",
        lambda images: [Prediction((((0.0, 0.0, 9.0, 9.0), 0.3),), ())] * len(images),
        ocr=stub_ocr,
        frames=stub_frames,
    )
    counts = builder.recording(vision.Recording("rec", work, "train", (0,), 2))
    assert counts == {
        "frames": 0,
        "captionLines": 0,
        "framesWithCaption": 0,
        "newOcrFrames": 1,
        "uncertainFace": 2,
    }


def test_the_builder_refuses_a_short_decode(tmp_path: Path) -> None:
    work = write_work(tmp_path)
    builder = vision.Builder(
        tmp_path / "dataset", stub_detect, ocr=stub_ocr, frames=lambda *a, **k: iter([])
    )
    with pytest.raises(ReplayError, match="every sampled frame"):
        builder.recording(vision.Recording("rec", work, "train", (0,), 2))


def test_the_builder_needs_chunk_configurations(tmp_path: Path) -> None:
    builder = vision.Builder(tmp_path / "dataset", stub_detect)
    with pytest.raises(ReplayError, match="no chunk configurations"):
        builder.recording(vision.Recording("rec", tmp_path, "train", (0,), 2))


def test_data_yaml_lists_roles_and_included_lists(tmp_path: Path) -> None:
    (tmp_path / "images" / "a").mkdir(parents=True)
    (tmp_path / "images" / "a" / "a_1.jpg").write_bytes(b"")
    (tmp_path / "old.txt").write_text("/x/old.jpg\n", encoding="utf-8")
    recordings = [vision.Recording("a", tmp_path, "train", (0,), 1)]
    text = vision.data_yaml(tmp_path, recordings, {"train": [str(tmp_path / "old.txt")]})
    assert text == (
        f"path: {tmp_path}\ntrain: {tmp_path / 'train.txt'}\nnames:\n"
        "  0: avatar_face\n  1: source_caption\n"
    )
    assert (tmp_path / "train.txt").read_text(encoding="utf-8") == (
        f"{tmp_path / 'images/a/a_1.jpg'}\n/x/old.jpg\n"
    )


def test_data_yaml_refuses_a_non_list_include(tmp_path: Path) -> None:
    with pytest.raises(ReplayError, match="include"):
        vision.data_yaml(tmp_path, [], {"train": "x"})


def test_a_line_half_inside_predicted_boxes_is_covered() -> None:
    assert vision.covered((0.0, 0.0, 10.0, 2.0), [(0.0, 0.0, 5.0, 2.0)]) is True
    assert (
        vision.covered((0.0, 0.0, 10.0, 2.0), [(0.0, 0.0, 4.0, 2.0), (50.0, 0.0, 60.0, 2.0)])
        is False
    )


def test_score_counts_found_and_missed_lines_and_interface_boxes() -> None:
    tally = dict.fromkeys(
        (
            "frames",
            "lines",
            "linesFound",
            "framesWithCaption",
            "framesAllMissed",
            "boxes",
            "interfaceFalseBoxes",
            "linesRepaired",
            "framesAllUnrepaired",
        ),
        0,
    )
    stored = {
        "canvasScale": 2.0,
        "captions": [[100, 1000, 300, 1030], [100, 1040, 300, 1070]],
        "interface": [[10, 10, 50, 30]],
    }
    prediction = Prediction((), ((50.0, 500.0, 150.0, 515.0), (0.0, 0.0, 30.0, 20.0)))
    grown = Prediction((), ((50.0, 520.0, 150.0, 535.0),))
    vision.score(stored, prediction, grown, tally)
    assert tally == {
        "frames": 1,
        "lines": 2,
        "linesFound": 1,
        "framesWithCaption": 1,
        "framesAllMissed": 0,
        "boxes": 2,
        "interfaceFalseBoxes": 1,
        "linesRepaired": 1,
        "framesAllUnrepaired": 0,
    }


def test_a_stored_box_must_be_four_numbers() -> None:
    with pytest.raises(ReplayError, match="four numbers"):
        vision.box_of([1, 2, 3])


def test_evaluate_scores_every_image_of_a_recording(tmp_path: Path) -> None:
    (tmp_path / "images/rec").mkdir(parents=True)
    (tmp_path / "vision/rec").mkdir(parents=True)
    cv2.imwrite(str(tmp_path / "images/rec/rec_1.jpg"), np.zeros((16, 16, 3), dtype=np.uint8))
    stored = {"canvasScale": 1.0, "captions": [[0, 0, 8, 4]], "interface": []}
    (tmp_path / "vision/rec/rec_1.json").write_text(json.dumps(stored), encoding="utf-8")
    result = vision.evaluate(lambda images: [Prediction((), ())] * len(images), tmp_path, "rec")
    assert result == {
        "frames": 1,
        "lines": 1,
        "linesFound": 0,
        "framesWithCaption": 1,
        "framesAllMissed": 1,
        "boxes": 0,
        "interfaceFalseBoxes": 0,
        "linesRepaired": 0,
        "framesAllUnrepaired": 1,
        "lineRecall": 0.0,
        "missedLines": 1,
        "productionRecall": 0.0,
        "productionMissedLines": 1,
    }


def test_evaluate_needs_images(tmp_path: Path) -> None:
    with pytest.raises(ReplayError, match="No evaluation images"):
        vision.evaluate(lambda images: [], tmp_path, "rec")


def test_sure_captions_keep_caption_boxes_at_the_floor() -> None:
    result = fake_result(
        [1.0, 1.0, 0.0], [0.5, 0.4, 0.9], [[1, 2, 3, 4], [5, 6, 7, 8], [0, 0, 1, 1]]
    )
    prediction = Prediction(
        (((0.0, 0.0, 1.0, 1.0), 0.9),), ((1.0, 2.0, 3.0, 4.0), (5.0, 6.0, 7.0, 8.0))
    )
    assert vision.sure_captions(prediction, result, 0.5) == Prediction(
        (((0.0, 0.0, 1.0, 1.0), 0.9),), ((1.0, 2.0, 3.0, 4.0),)
    )


def settings(tmp_path: Path) -> Path:
    config = tmp_path / "config"
    (config / "Ultralytics").mkdir(parents=True)
    services = ("sync", "clearml", "comet", "dvc", "mlflow", "raytune", "wandb", "tensorboard")
    values: dict[str, object] = dict.fromkeys(services, False)
    values |= {"datasets_dir": str(tmp_path / "work"), "runs_dir": str(tmp_path / "work")}
    (config / "Ultralytics" / "settings.json").write_text(json.dumps(values), encoding="utf-8")
    return config


class FakeModel:
    def __init__(self, weights: str) -> None:
        self.weights = weights

    def predict(self, images: list[np.ndarray], **options: object) -> list[SimpleNamespace]:
        return [fake_result([0.0, 1.0], [0.9, 0.3], [[1, 1, 5, 5], [0, 0, 8, 4]])] * len(images)


def test_main_evaluates_with_the_checked_weights(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config = settings(tmp_path)
    weights = tmp_path / "w.pt"
    weights.write_bytes(b"w")
    dataset = tmp_path / "work" / "dataset"
    (dataset / "images/rec").mkdir(parents=True)
    (dataset / "vision/rec").mkdir(parents=True)
    cv2.imwrite(str(dataset / "images/rec/rec_1.jpg"), np.zeros((16, 16, 3), dtype=np.uint8))
    stored = {"canvasScale": 1.0, "captions": [[0, 0, 8, 4]], "interface": []}
    (dataset / "vision/rec/rec_1.json").write_text(json.dumps(stored), encoding="utf-8")
    monkeypatch.setattr("ultralytics.models.yolo.model.YOLO", FakeModel)
    output = tmp_path / "work" / "eval.json"
    digest = "0b6a1c1b4b58fdb4e6ca0bf3b8f8a1a7e6e9db7e9b0f9a3b1d7f6f2b1e9c1a6e"
    monkeypatch.setattr(vision, "check_checkpoint", lambda path, expected: None)
    code = vision.main(
        [
            "evaluate",
            str(dataset),
            str(output),
            "--recording",
            "rec",
            "--weights",
            str(weights),
            "--weights-sha256",
            digest,
            "--config-directory",
            str(config),
            "--work",
            str(tmp_path / "work"),
            "--device",
            "cpu",
        ]
    )
    assert code == 0
    document = json.loads(output.read_text(encoding="utf-8"))
    assert document["recordings"]["rec"]["lineRecall"] == 1.0
    assert document["recordings"]["rec"]["productionRecall"] == 1.0


def test_main_refuses_an_existing_evaluation(tmp_path: Path) -> None:
    (tmp_path / "out.json").write_text("{}", encoding="utf-8")
    with pytest.raises(ReplayError, match="overwrite"):
        vision.main(
            [
                "evaluate",
                str(tmp_path),
                str(tmp_path / "out.json"),
                "--recording",
                "a",
                "--weights",
                "w",
                "--weights-sha256",
                "x",
                "--config-directory",
                "c",
                "--work",
                "w",
            ]
        )


def test_main_builds_a_dataset(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    work = write_work(tmp_path)
    output = tmp_path / "dataset"
    item = {"name": "rec", "cleanWork": str(work), "role": "train", "chunks": [0], "frameStep": 2}
    manifest = tmp_path / "m.json"
    manifest.write_text(json.dumps({"output": str(output), "recordings": [item]}), encoding="utf-8")
    monkeypatch.setattr(vision, "detector", lambda *a, **k: stub_detect)
    monkeypatch.setattr(vision, "session_ocr", stub_ocr)
    monkeypatch.setattr(
        vision.Builder.__init__, "__kwdefaults__", {"ocr": stub_ocr, "frames": stub_frames}
    )
    code = vision.main(
        [
            "build",
            str(manifest),
            "--weights",
            "w",
            "--weights-sha256",
            "x",
            "--config-directory",
            "c",
            "--work",
            "w",
        ]
    )
    assert code == 0
    assert json.loads((output / "summary.json").read_text(encoding="utf-8")) == {
        "rec": {
            "role": "train",
            "frames": 2,
            "captionLines": 2,
            "framesWithCaption": 2,
            "newOcrFrames": 1,
        }
    }


def test_main_refuses_an_existing_dataset(tmp_path: Path) -> None:
    manifest = tmp_path / "m.json"
    manifest.write_text(json.dumps({"output": str(tmp_path), "recordings": []}), encoding="utf-8")
    with pytest.raises(ReplayError, match="overwrite"):
        vision.main(
            [
                "build",
                str(manifest),
                "--weights",
                "w",
                "--weights-sha256",
                "x",
                "--config-directory",
                "c",
                "--work",
                "w",
            ]
        )
