"""A delivery's verified YouTube encode becomes its master; the delivery is then removed."""

import json
import subprocess
from pathlib import Path

import pytest

from replay import pipeline_completion as completion
from replay import recording_viewer_steps as viewer
from replay import youtube_encode
from replay import youtube_master as master
from replay.core import ReplayError

FFMPEG = "ffmpeg"
FFPROBE = "ffprobe"


def movie(path: Path, *, size: str = "720x1280", seconds: float = 2.0, codec: str = "libx264",
          pattern: str = "testsrc2", rate: int = 30) -> Path:  # fmt: skip
    path.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        [FFMPEG, "-v", "error", "-f", "lavfi", "-i",
         f"{pattern}=size={size}:rate={rate}:d={seconds}", "-f", "lavfi", "-i",
         f"sine=d={seconds}", "-c:v", codec, "-crf", "12", "-c:a", "aac", "-shortest", str(path)],
        check=True,
    )  # fmt: skip
    return path


def excerpt(tmp_path: Path) -> tuple[Path, Path]:
    delivery = movie(tmp_path / "lib/clip/delivery/S_JST_clip-review.mp4")
    delivery.with_suffix(".srt").write_text("1\n00:00:00,000 --> 00:00:01,000\na\n", "utf-8")
    delivery.with_suffix(".srt.excitement.json").write_text("{}", encoding="utf-8")
    summaries = delivery.with_suffix(".srt.summaries")
    summaries.mkdir(mode=0o700)
    (summaries / "current.json").write_text("{}", encoding="utf-8")
    encode = youtube_encode.output_of(delivery).with_suffix(".mov")
    encode.parent.mkdir(parents=True)
    subprocess.run(
        [FFMPEG, "-v", "error", "-i", str(delivery), "-c:v", "libx264", "-crf", "10", "-c:a", "aac",
         str(encode)],
        check=True,
    )  # fmt: skip
    return delivery, encode


def test_an_excerpt_moves_to_its_verified_master_and_the_rest_is_deleted(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.setattr(master, "checkout_root", lambda: tmp_path)
    (tmp_path / master.DELETIONS).parent.mkdir(parents=True)
    delivery, encode = excerpt(tmp_path)
    assert master.main([str(delivery)]) == 0
    printed = Path(capsys.readouterr().out.strip())
    assert printed.parent.parent.parent.name == "revisions"
    assert printed.name == "S_JST_clip-review.mp4"
    assert printed.with_suffix(".srt").read_text(encoding="utf-8") == (
        "1\n00:00:00,000 --> 00:00:01,000\na\n"
    )
    assert printed.with_suffix(".srt.excitement.json").is_file()
    assert (printed.with_suffix(".srt.summaries") / "current.json").is_file()
    assert printed.with_suffix(".srt.summaries").stat().st_mode & 0o777 == 0o700
    assert not delivery.exists()
    assert not delivery.with_suffix(".srt").exists()
    assert not encode.exists()
    assert not encode.with_name(delivery.name).exists()
    proof = json.loads((printed.parents[1] / "metadata" / master.RECORD).read_text("utf-8"))
    assert proof["frames"] == 60
    assert proof["size"] == [720, 1280]
    assert proof["psnrMeanDb"] >= 35
    logged = (tmp_path / master.DELETIONS).read_text(encoding="utf-8")
    assert logged.count("replaced by its verified YouTube encode") == 6
    assert not delivery.with_suffix(".srt.summaries").exists()


def test_the_master_keeps_the_delivery_audio_when_the_encode_trims_its_tail(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(master, "checkout_root", lambda: tmp_path)
    (tmp_path / master.DELETIONS).parent.mkdir(parents=True)
    delivery = movie(tmp_path / "lib/clip/delivery/S_JST_clip-review.mp4")
    delivery.with_suffix(".srt").write_text("", encoding="utf-8")
    encode = youtube_encode.output_of(delivery).with_suffix(".mov")
    encode.parent.mkdir(parents=True)
    subprocess.run(
        [FFMPEG, "-v", "error", "-i", str(delivery), "-c:v", "libx264", "-crf", "10", "-af",
         "atrim=end=1.9", "-ar", "48000", "-c:a", "aac", str(encode)],
        check=True,
    )  # fmt: skip
    expected = master.probe(delivery, FFPROBE).audio
    published = master.master(delivery, ffmpeg=FFMPEG, ffprobe=FFPROBE)
    assert master.probe(published, FFPROBE).audio == expected
    proof = json.loads((published.parents[1] / "metadata" / master.RECORD).read_text("utf-8"))
    assert proof["audioFrom"] == "delivery"


def test_an_encode_of_other_frames_or_pictures_is_refused(tmp_path: Path) -> None:
    delivery = movie(tmp_path / "a/delivery/S_JST_a-review.mp4")
    short = movie(youtube_encode.output_of(delivery), seconds=1.0)
    with pytest.raises(ReplayError, match="frames, duration"):
        master.verified(delivery, short, ffmpeg=FFMPEG, ffprobe=FFPROBE)
    other = movie(tmp_path / "b/x/other.mov", pattern="smptebars")
    with pytest.raises(ReplayError, match="PSNR"):
        master.verified(delivery, other, ffmpeg=FFMPEG, ffprobe=FFPROBE)
    with pytest.raises(ReplayError, match="or its YouTube encode is missing"):
        master.master(tmp_path / "none.mp4", ffmpeg=FFMPEG, ffprobe=FFPROBE)


def test_a_movie_without_one_video_stream_or_psnr_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    sound = tmp_path / "sound.m4a"
    subprocess.run([FFMPEG, "-v", "error", "-f", "lavfi", "-i", "sine=d=1", str(sound)], check=True)
    with pytest.raises(ReplayError, match="exactly one video stream"):
        master.probe(sound, FFPROBE)
    monkeypatch.setattr(
        subprocess, "run", lambda *a, **k: subprocess.CompletedProcess([], 0, "", "no psnr here")
    )
    with pytest.raises(ReplayError, match="no PSNR"):
        master.sampled_psnr(sound, sound, FFMPEG)


def test_a_pipeline_delivery_is_published_again_and_must_pass_its_check(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(master, "checkout_root", lambda: tmp_path)
    (tmp_path / master.DELETIONS).parent.mkdir(parents=True)
    delivery = movie(tmp_path / "lib/rec/revisions/p/delivery/S_JST_rec-review.mp4")
    encode = youtube_encode.output_of(delivery)
    encode.parent.mkdir(parents=True)
    subprocess.run(
        [FFMPEG, "-v", "error", "-i", str(delivery), "-c:v", "libx264", "-crf", "10", "-c:a", "aac",
         str(encode)],
        check=True,
    )  # fmt: skip
    candidate = tmp_path / "final/candidate.mp4"
    candidate.parent.mkdir()
    candidate.hardlink_to(delivery)
    steps = tmp_path / "steps"
    steps.mkdir()
    (steps / "finish.json").write_text(
        json.dumps({"outputs": {"movie": {"path": str(candidate)}}}), encoding="utf-8"
    )
    (delivery.parents[1] / "metadata").mkdir()
    (delivery.parents[1] / "metadata" / completion.INDEX_NAME).write_text(
        json.dumps({"recording": "rec", "candidate": {"path": "/w/placement-v1.json"},
                    "steps": {"finish": {"path": str(steps / "finish.json")}}}),
        encoding="utf-8",
    )  # fmt: skip
    published = tmp_path / "lib/rec/revisions/y/delivery/S_JST_rec-review.mp4"
    delivery.with_suffix(".captions.json").write_text('{"plan": 1}', encoding="utf-8")
    delivery.with_suffix(".faces.json").write_text('{"faces": 1}', encoding="utf-8")
    calls: list[object] = []

    def publish(root: Path, **options: object) -> tuple[Path, Path]:
        calls.append((root, options["steps"], options["candidate"], str(options["revision"])[:8]))
        published.parent.mkdir(parents=True)
        published.hardlink_to(Path(str(options["movie"])))
        return published, published.parents[1] / "metadata" / completion.INDEX_NAME

    def assess(_index: Path, **_options: object) -> dict[str, str]:
        # The gate reads the caption plan beside the master, so it is there first.
        calls.append(published.with_suffix(".captions.json").read_text(encoding="utf-8"))
        return {}

    monkeypatch.setattr(completion, "publish", publish)
    monkeypatch.setattr(viewer, "release_older", lambda root, path: calls.append(path))
    monkeypatch.setattr(completion, "assess", assess)
    assert master.master(delivery, ffmpeg=FFMPEG, ffprobe=FFPROBE) == published
    assert calls == [
        (tmp_path / "edits/rec", steps, Path("/w/placement-v1.json"), "youtube-"),
        published,
        '{"plan": 1}',
    ]
    assert published.with_suffix(".faces.json").read_text(encoding="utf-8") == '{"faces": 1}'
    assert not delivery.exists()
    assert not candidate.exists()
    assert not encode.exists()
    assert published.is_file()


def test_a_master_failing_its_completion_check_keeps_the_delivery(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    delivery = movie(tmp_path / "lib/rec/revisions/p/delivery/S_JST_rec-review.mp4")
    index = delivery.parents[1] / "metadata" / completion.INDEX_NAME
    index.parent.mkdir()
    index.write_text(
        json.dumps({"recording": "rec", "candidate": {"path": "/c"},
                    "steps": {"finish": {"path": str(tmp_path / "finish.json")}}}),
        encoding="utf-8",
    )  # fmt: skip
    monkeypatch.setattr(completion, "publish", lambda root, **o: (delivery, index))
    monkeypatch.setattr(viewer, "release_older", lambda root, path: None)
    monkeypatch.setattr(completion, "assess", lambda i, **k: {"silence_cut": "frames differ"})
    with pytest.raises(ReplayError, match="fails its completion check"):
        master.republished(delivery, index, delivery, "youtube-x")
    assert delivery.exists()
