"""A recording may lay its captions out on its own frame and keep its music, as declared intents."""

import hashlib
import json
import shutil
import subprocess
from pathlib import Path

import pytest
from PIL import Image, ImageFont

from replay import caption_layout as layout
from replay.core import ReplayError

FFMPEG = shutil.which("ffmpeg")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_speaker(tmp_path: Path) -> dict[str, object]:
    icon, font = tmp_path / "icon.png", tmp_path / "font.ttf"
    Image.new("RGB", (40, 40), (200, 30, 30)).save(icon)
    font.write_bytes(b"font")
    return {
        "name": "speaker", "icon": str(icon), "iconSha256": digest(icon), "font": str(font),
        "fontSha256": digest(font), "iconX": 4, "iconY": 4, "iconSize": 40, "nameSize": 12,
    }  # fmt: skip


def plain_layout() -> dict[str, object]:
    return {
        "version": 1, "canvas": [192, 108], "align": "left", "originX": 60, "regionTop": 20,
        "regionBottom": 100, "regionRight": 180,
    }  # fmt: skip


@pytest.fixture
def default_font(monkeypatch: pytest.MonkeyPatch) -> None:
    """The bundled Pillow font in place of the declared one (its weight is not variable)."""

    fonts = {size: ImageFont.load_default(size) for size in (12, 40)}

    def truetype(_path: str, size: int) -> ImageFont.FreeTypeFont:
        font = fonts[size]
        assert isinstance(font, ImageFont.FreeTypeFont)
        monkeypatch.setattr(font, "set_variation_by_name", lambda _name: None)
        return font

    monkeypatch.setattr(ImageFont, "truetype", truetype)


def test_without_intents_the_settings_are_unchanged(tmp_path: Path) -> None:
    assert layout.captions_settings({"version": 1}, tmp_path) == {"version": 1}


def test_both_intents_reach_the_settings(tmp_path: Path) -> None:
    value = {**plain_layout(), "speaker": write_speaker(tmp_path)}
    (tmp_path / "caption-layout.json").write_text(json.dumps(value), "utf-8")
    audio = {"version": 1, "source": "original-mix", "intent": "music kept by request"}
    (tmp_path / "delivery-audio.json").write_text(json.dumps(audio), "utf-8")
    assert layout.captions_settings({"version": 1}, tmp_path) == {
        "version": 1, "layout": value, "deliveryAudio": audio,
    }  # fmt: skip
    assert layout.frame(value) == (192, 108)


@pytest.mark.parametrize(
    ("change", "message"),
    [
        ({"version": 2}, "must hold exactly"),
        ({"extra": 1}, "must hold exactly"),
        ({"canvas": [192]}, "canvas must be"),
        ({"canvas": [8, 108]}, "canvas must be"),
        ({"align": "right"}, "align must be one of"),
        ({"originX": 180}, "originX must be a number"),
        ({"regionRight": True}, "regionRight must be a number"),
        ({"regionTop": 100}, "regionTop must be a number"),
    ],
)
def test_malformed_layouts_are_refused(change: dict[str, object], message: str) -> None:
    with pytest.raises(ReplayError, match=message):
        layout.layout({**plain_layout(), **change})


@pytest.mark.parametrize(
    ("change", "message"),
    [
        ({"name": " "}, "needs a name"),
        ({"iconSha256": "0" * 64}, "icon is missing or changed"),
        ({"icon": "relative.png"}, "absolute path with its SHA-256"),
        ({"iconX": 160}, "iconX must be a number"),
        ({"nameSize": 41}, "nameSize must be a number"),
    ],
)
def test_malformed_speakers_are_refused(
    tmp_path: Path, change: dict[str, object], message: str
) -> None:
    with pytest.raises(ReplayError, match=message):
        layout.layout({**plain_layout(), "speaker": {**write_speaker(tmp_path), **change}})
    with pytest.raises(ReplayError, match="exactly font"):
        layout.layout({**plain_layout(), "speaker": {"name": "x"}})


@pytest.mark.parametrize(
    "value",
    [
        {"version": 1, "source": "voice", "intent": "x"},
        {"version": 1, "source": "original-mix"},
        {"version": 1, "source": "original-mix", "intent": "  "},
    ],
)
def test_the_kept_music_needs_its_reason(value: dict[str, object]) -> None:
    with pytest.raises(ReplayError, match="delivery-audio.json must be|why the music is kept"):
        layout.delivery_audio(value)


def test_the_nameplate_draws_a_round_icon_and_the_name(tmp_path: Path, default_font: None) -> None:
    plate = layout.badge_image({**plain_layout(), "speaker": write_speaker(tmp_path)})
    assert plate.size == (192, 108)
    assert plate.getpixel((24, 24)) == (200, 30, 30, 255)  # the icon's centre
    assert plate.getpixel((4, 4)) == (0, 0, 0, 0)  # outside the disc
    assert plate.getpixel((24, 6)) == (255, 255, 255, 255)  # the white ring
    name = plate.crop((0, 50, 60, 70))
    assert name.getbbox() is not None


def test_a_name_wider_than_the_canvas_is_refused(tmp_path: Path, default_font: None) -> None:
    speaker = {**write_speaker(tmp_path), "name": "a much longer speaker name", "nameSize": 40}
    with pytest.raises(ReplayError, match="name does not fit"):
        layout.badge_image({**plain_layout(), "speaker": speaker})


@pytest.mark.skipif(FFMPEG is None, reason="needs FFmpeg")
def test_the_badge_movie_is_written_once_and_reused(tmp_path: Path, default_font: None) -> None:
    assert FFMPEG is not None
    value = {**plain_layout(), "speaker": write_speaker(tmp_path)}
    movie = tmp_path / "badge" / "badge.mov"
    assert layout.badge(value, movie, ffmpeg=FFMPEG, seconds=1) == movie
    probe = subprocess.run(
        [str(Path(FFMPEG).with_name("ffprobe")), "-v", "error", "-count_frames", "-show_entries",
         "stream=width,height,pix_fmt,nb_read_frames", "-of", "json", str(movie)],
        capture_output=True, text=True, check=True,
    ).stdout  # fmt: skip
    assert json.loads(probe)["streams"] == [
        {"width": 192, "height": 108, "pix_fmt": "yuva444p12le", "nb_read_frames": "30"}
    ]
    assert layout.badge(value, movie, ffmpeg=FFMPEG, seconds=1) == movie
    with pytest.raises(ReplayError, match="exists for another nameplate"):
        layout.badge(value, movie, ffmpeg=FFMPEG, seconds=2)


def test_a_layout_without_a_speaker_draws_no_badge(tmp_path: Path) -> None:
    with pytest.raises(ReplayError, match="no speaker"):
        layout.badge(plain_layout(), tmp_path / "badge.mov", ffmpeg="ffmpeg", seconds=1)


def test_the_cli_draws_the_badge_of_captions_json(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    calls: list[tuple[dict[str, object], Path, str, int]] = []

    def badge(value: dict[str, object], output: Path, *, ffmpeg: str, seconds: int) -> Path:
        calls.append((value, output, ffmpeg, seconds))
        return output

    monkeypatch.setattr(layout, "badge", badge)
    captions = tmp_path / "captions.json"
    captions.write_text(json.dumps({"layout": plain_layout()}), "utf-8")
    output = tmp_path / "badge.mov"
    assert layout.main(["badge", str(captions), str(output), "--seconds", "121"]) == 0
    assert calls == [(plain_layout(), output, "ffmpeg", 121)]
    assert capsys.readouterr().out == f"{output}\n"
    captions.write_text(json.dumps({}), "utf-8")
    with pytest.raises(ReplayError, match="no caption layout"):
        layout.main(["badge", str(captions), str(output), "--seconds", "121"])
