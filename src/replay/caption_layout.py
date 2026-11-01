"""A recording's own caption layout and delivered audio, as explicit editing intents.

Both are opt-in files in the recording's edit root; without them nothing changes.

``caption-layout.json`` places the captions on a frame of its own ``canvas`` (a landscape
recording keeps its 16:9 frame): ``align`` ``left`` starts every line of every caption at the
same ``originX`` (Motion paragraph alignment left), each caption's run is anchored as low as it
fits in ``[regionTop, regionBottom]`` and no line may cross ``regionRight``; the person track is
not consulted. An optional ``speaker`` draws a nameplate (the round ``icon`` and ``name`` below
it, in ``font``) that stays on screen for the whole delivery, as one full-frame alpha layer of
every compose group (``badge``).

``delivery-audio.json`` (``{"version": 1, "source": "original-mix", "intent": WHY}``) delivers
the recording's original mix, background music included, instead of the separated voice. The cut
plan, the reveal onsets and the voice level are still measured on the voice; the completion gate
accepts the kept music only with this recorded intent.

``recording_pipeline`` copies both into the recording's ``captions.json`` (``layout``,
``deliveryAudio``), where the caption steps read them.

    python -m replay.caption_layout badge CAPTIONS.json OUTPUT.mov --ffmpeg FFMPEG
"""

import argparse
import hashlib
import json
import math
import os
import re
import subprocess
from collections.abc import Sequence
from pathlib import Path
from typing import TYPE_CHECKING

from replay import video_config
from replay.core import ReplayError
from replay.recordings import record

if TYPE_CHECKING:
    from PIL import Image

LAYOUT_INTENT = "caption-layout.json"
AUDIO_INTENT = "delivery-audio.json"
LAYOUT_KEYS = frozenset(
    {"version", "canvas", "align", "originX", "regionTop", "regionBottom", "regionRight"}
)
SPEAKER_KEYS = frozenset(
    {"name", "icon", "iconSha256", "font", "fontSha256", "iconX", "iconY", "iconSize", "nameSize"}
)
AUDIO_KEYS = frozenset({"version", "source", "intent"})
ALIGNMENTS = frozenset({"left", "centre"})
ORIGINAL_MIX = "original-mix"
SHA256 = re.compile(r"[0-9a-f]{64}")
MAX_SIDE = 8192
FONT_WEIGHT = b"Bold"  # the caption style's weight
NAME_GAP = 6  # pixels between the icon and the name below it
RING = 4  # the icon's white ring, in pixels
SUPERSAMPLE = 4  # the round mask is drawn this much larger, then reduced (anti-aliasing)
BADGE_RATE = 30
BADGE_ARGUMENTS = (
    "-c:v", "prores_ks", "-profile:v", "4", "-pix_fmt", "yuva444p10le", "-vendor", "apl0",
    "-color_primaries", "bt709", "-color_trc", "bt709", "-colorspace", "bt709",
)  # fmt: skip


def number(value: object, name: str, *, low: float, high: float) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not low <= value <= high:
        raise ReplayError(f"Caption layout {name} must be a number in [{low}, {high}]")
    return float(value)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def pinned(value: dict[str, object], key: str) -> Path:
    """An absolute file whose content still has its recorded digest."""
    path, digest = Path(str(value[key])), value[f"{key}Sha256"]
    if not path.is_absolute() or not isinstance(digest, str) or not SHA256.fullmatch(digest):
        raise ReplayError(f"Caption layout {key} must be an absolute path with its SHA-256")
    if not path.is_file() or sha256(path) != digest:
        raise ReplayError(f"Caption layout {key} is missing or changed: {path}")
    return path


def frame(data: dict[str, object]) -> tuple[int, int]:
    """The layout's canvas (width, height) in pixels."""
    canvas = data.get("canvas")
    if not isinstance(canvas, list) or len(canvas) != 2:
        raise ReplayError("Caption layout canvas must be [width, height] in pixels")
    width, height = canvas
    if not all(type(side) is int and 16 <= side <= MAX_SIDE for side in (width, height)):
        raise ReplayError("Caption layout canvas must be [width, height] in pixels")
    return int(width), int(height)


def speaker(value: object, canvas: tuple[int, int]) -> dict[str, object]:
    data = record(value)
    if set(data) != SPEAKER_KEYS:
        raise ReplayError(f"A caption speaker has exactly {', '.join(sorted(SPEAKER_KEYS))}")
    if not isinstance(data["name"], str) or not data["name"].strip():
        raise ReplayError("A caption speaker needs a name")
    pinned(data, "icon")
    pinned(data, "font")
    size = number(data["iconSize"], "iconSize", low=8, high=min(canvas))
    number(data["iconX"], "iconX", low=0, high=canvas[0] - size)
    number(data["iconY"], "iconY", low=0, high=canvas[1] - size)
    number(data["nameSize"], "nameSize", low=8, high=size)
    return data


def layout(value: object) -> dict[str, object]:
    """A caption layout, refused unless strictly well formed and inside its canvas."""
    data = record(value)
    if set(data) - {"speaker"} != LAYOUT_KEYS or data["version"] != 1:
        raise ReplayError(f"{LAYOUT_INTENT} must hold exactly {', '.join(sorted(LAYOUT_KEYS))}")
    width, height = frame(data)
    if data["align"] not in ALIGNMENTS:
        raise ReplayError(f"Caption layout align must be one of {', '.join(sorted(ALIGNMENTS))}")
    right = number(data["regionRight"], "regionRight", low=1, high=width)
    number(data["originX"], "originX", low=0, high=right - 1)
    bottom = number(data["regionBottom"], "regionBottom", low=1, high=height)
    number(data["regionTop"], "regionTop", low=0, high=bottom - 1)
    if "speaker" in data:
        speaker(data["speaker"], (width, height))
    return data


def delivery_audio(value: object) -> dict[str, object]:
    """A delivered-audio intent: the source and the recorded reason."""
    data = record(value)
    if set(data) != AUDIO_KEYS or data["version"] != 1 or data["source"] != ORIGINAL_MIX:
        raise ReplayError(f"{AUDIO_INTENT} must be {{version: 1, source: {ORIGINAL_MIX}, intent}}")
    if not isinstance(data["intent"], str) or not data["intent"].strip():
        raise ReplayError("The delivered-audio intent must record why the music is kept")
    return data


def captions_settings(settings: dict[str, object], edit_root: Path) -> dict[str, object]:
    """``settings`` with the recording's layout, delivered-audio and loudness intents, from its
    ``video-config.json`` or else its side files."""
    result = dict(settings)
    for name, key, check, configured in (
        (LAYOUT_INTENT, "layout", layout, video_config.caption_layout),
        (AUDIO_INTENT, "deliveryAudio", delivery_audio, video_config.delivery_audio),
    ):
        present, value = configured(edit_root)
        path = edit_root / name
        if present and value is not None:
            result[key] = check(value)
        elif not present and path.exists():
            result[key] = check(json.loads(path.read_text(encoding="utf-8")))
    audio = record(video_config.value(edit_root, "audio", {}))
    if audio.get("loudness") is not None:
        result["voiceLoudness"] = audio["loudness"]
    # Computation-only voice correction is the default for config-driven (new) recordings
    # (user, 2026-11-01); false turns it off; recordings without video-config.json keep theirs.
    default = video_config.load(edit_root) is not None
    if audio.get("repair", default) is True:
        result["voiceRepair"] = True  # replay.voice_repair: declipping and compression
    if audio.get("deesser", default) is True:
        result["voiceDeesser"] = True  # replay.voice_deesser in the voice-repair step
    if audio.get("judge", default) is True:
        result["voiceJudge"] = True  # replay.voice_judge gates; replay.voice_tempo delivers
    return result


def round_icon(path: Path, size: int) -> "Image.Image":
    """The icon scaled to ``size``, cut to a disc inside a white ring (anti-aliased edges)."""
    # Pillow is imported only to draw (the pipeline imports this module for its settings).
    from PIL import Image, ImageDraw

    large = size * SUPERSAMPLE
    with Image.open(path) as opened:
        picture = opened.convert("RGB").resize((large, large), Image.Resampling.LANCZOS)
    disc = Image.new("L", (large, large), 0)
    ImageDraw.Draw(disc).ellipse((0, 0, large - 1, large - 1), fill=255)
    ring = RING * SUPERSAMPLE
    inner = Image.new("L", (large, large), 0)
    ImageDraw.Draw(inner).ellipse((ring, ring, large - 1 - ring, large - 1 - ring), fill=255)
    white = Image.new("RGB", (large, large), (255, 255, 255))
    face = Image.composite(picture, white, inner)
    result = Image.new("RGBA", (large, large), (0, 0, 0, 0))
    result.paste(face, (0, 0), disc)
    return result.resize((size, size), Image.Resampling.LANCZOS)


def badge_image(settings: dict[str, object]) -> "Image.Image":
    """The full-frame nameplate: the round icon with the name centred below it."""
    from PIL import Image, ImageDraw, ImageFont

    data = layout(settings)
    person = record(data["speaker"])
    width, height = frame(data)
    plate = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    size = round(float(str(person["iconSize"])))
    left, top = round(float(str(person["iconX"]))), round(float(str(person["iconY"])))
    plate.alpha_composite(round_icon(pinned(person, "icon"), size), (left, top))
    font = ImageFont.truetype(str(pinned(person, "font")), int(float(str(person["nameSize"]))))
    font.set_variation_by_name(FONT_WEIGHT)
    stroke = max(2, math.ceil(font.size / 8))
    centre = left + size / 2
    ImageDraw.Draw(plate).text(
        (centre, top + size + NAME_GAP), str(person["name"]), font=font, anchor="mt",
        fill=(255, 255, 255, 255), stroke_width=stroke, stroke_fill=(0, 0, 0, 255),
    )  # fmt: skip
    drawn = ImageDraw.Draw(plate).textbbox(
        (centre, top + size + NAME_GAP), str(person["name"]), font=font, anchor="mt",
        stroke_width=stroke,
    )  # fmt: skip
    if drawn[0] < 0 or drawn[2] > width or drawn[3] > height:
        raise ReplayError("The caption nameplate's name does not fit its canvas")
    return plate


def badge(settings: dict[str, object], output: Path, *, ffmpeg: str, seconds: int) -> Path:
    """A ``seconds`` long ProRes 4444 movie of the nameplate; an existing one is reused only
    when its record (layout, digests, length) still matches."""
    data = layout(settings)
    if "speaker" not in data:
        raise ReplayError("The caption layout has no speaker to draw")
    note = output.with_name(f"{output.name}.json")
    expected = {"version": 1, "layout": data, "seconds": seconds, "rate": BADGE_RATE}
    if output.exists() and note.exists():
        if json.loads(note.read_text(encoding="utf-8")) != {**expected, "sha256": sha256(output)}:
            raise ReplayError(f"{output} exists for another nameplate; inspect it")
        return output
    output.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    still = output.with_name(f"{output.stem}.png")
    badge_image(data).save(still)
    staged = output.with_name(f".{output.stem}.partial.mov")
    staged.unlink(missing_ok=True)
    subprocess.run(
        [ffmpeg, "-nostdin", "-v", "error", "-loop", "1", "-framerate", str(BADGE_RATE),
         "-i", str(still), "-t", str(seconds), *BADGE_ARGUMENTS, str(staged)],
        check=True,
    )  # fmt: skip
    staged.rename(output)
    with note.open("w", encoding="utf-8") as stream:
        os.fchmod(stream.fileno(), 0o600)
        json.dump({**expected, "sha256": sha256(output)}, stream, ensure_ascii=False, indent=1)
    return output


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    drawn = commands.add_parser("badge")
    drawn.add_argument("captions", type=Path)
    drawn.add_argument("output", type=Path)
    drawn.add_argument("--ffmpeg", default="ffmpeg")
    drawn.add_argument("--seconds", type=int, required=True)
    args = parser.parse_args(argv)
    settings = record(json.loads(args.captions.read_text(encoding="utf-8")))
    if "layout" not in settings:
        raise ReplayError("captions.json has no caption layout")
    print(badge(record(settings["layout"]), args.output, ffmpeg=args.ffmpeg, seconds=args.seconds))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
