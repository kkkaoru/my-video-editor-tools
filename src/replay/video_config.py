"""One per-video settings file, ``edits/<recording>/video-config.json``, read by the pipeline steps.

Every key is optional; an absent key keeps the pipeline default (``DEFAULTS-v1.json`` and the
built-in behaviour), so a recording without the file behaves exactly as before. The file replaces
the older per-feature side files (``reframe.json``, ``cleanup.json``, ``caption-colors.json``,
``caption-layout.json``, ``delivery-audio.json``); a recording may hold the file or those side
files, never both for the same setting. Schema (version 1)::

    {
      "version": 1,
      "orientation": "source" | {"aspect": [9, 16], "follow": "character"},
      "speakers": {"count": 1, "name": "streamer", "icon": "/abs/icon.png",
                   "iconSha256": "<sha256>"},
      "captions": {
        "palette": {"gradientTop": "#RRGGBB", "gradientBottom": "#RRGGBB"},
        "layout": {"canvas": [1920, 1080], "align": "left" | "centre", "originX": 780,
                   "regionTop": 560, "regionBottom": 1056, "regionRight": 1880,
                   "nameplate": {"font": "/abs/font.ttf", "fontSha256": "<sha256>",
                                 "iconX": 600, "iconY": 860, "iconSize": 150,
                                 "nameSize": 36}}
      },
      "audio": {"delivery": "vocals" | "original-mix", "intent": "why the music is kept",
                "loudness": {"targetLufs": -18, "truePeakDbtp": -1}, "repair": true,
                "deesser": true,
                "judge": true},
      "cut": {"nonSpeech": true},
      "cleanup": {"sourceCaptions": true, "commentBoxes": false},
      "clips": true,
      "channelLabel": "かしすみるく",
      "asrGlossary": ["/abs/glossary.json"],
      "verticals": {"inset": [1356, 680, 312, 324], "ranges": [[238, 840]]}
    }

A ``nameplate`` needs ``speakers`` (its name and icon). ``cut.nonSpeech`` and
``cleanup.sourceCaptions`` accept only ``true``: the recording pipeline always cuts non-speech and
always cleans (or verifies the absence of) source captions, so turning either off is refused rather
than silently ignored. ``audio.repair``, ``audio.deesser`` and ``audio.judge`` (computation-only
corrections; the repair's WavLM only verifies) default to ``true`` in a video-config.json;
``false`` turns them off. ``speakers.count`` is 1: several speakers use the multi-speaker flow.
``verticals`` asks ``replay.vertical_excerpt`` for 9:16 excerpts of the finished delivery: the
source's centre with its ``inset`` (x, y, width, height in source pixels: the character's own
window) placed at the bottom, for each ``[start, end]`` range of delivery seconds.
"""

import json
import sys
from pathlib import Path

from replay.core import ReplayError
from replay.recordings import record

NAME = "video-config.json"
KEYS = frozenset(
    {"version", "orientation", "speakers", "captions", "audio", "cut", "cleanup", "clips",
     "channelLabel", "asrGlossary", "verticals"}
)  # fmt: skip
SPEAKER_KEYS = frozenset({"count", "name", "icon", "iconSha256"})
CAPTION_KEYS = frozenset({"palette", "layout"})
AUDIO_KEYS = frozenset({"delivery", "intent", "loudness", "repair", "deesser", "judge"})
LOUDNESS_KEYS = frozenset({"targetLufs", "truePeakDbtp"})
NAMEPLATE_KEYS = frozenset({"font", "fontSha256", "iconX", "iconY", "iconSize", "nameSize"})
REFRAME = {"aspect": [9, 16], "follow": "character"}
DELIVERIES = frozenset({"vocals", "original-mix"})
VERTICAL_KEYS = frozenset({"inset", "ranges"})
# Side files the config replaces, by the setting they hold.
SIDE_FILES = {
    "orientation": "reframe.json",
    "cleanup": "cleanup.json",
    "palette": "caption-colors.json",
    "layout": "caption-layout.json",
    "audio": "delivery-audio.json",
}


def section(data: dict[str, object], key: str, keys: frozenset[str]) -> dict[str, object]:
    value = data.get(key, {})
    if not isinstance(value, dict) or not set(value) <= keys:
        raise ReplayError(f"{NAME}: {key} takes only {', '.join(sorted(keys))}")
    return value


def flag(value: object, name: str, *, fixed: bool = False) -> bool:
    if type(value) is not bool or (fixed and value is not True):
        allowed = "true (the pipeline cannot turn it off)" if fixed else "true or false"
        raise ReplayError(f"{NAME}: {name} must be {allowed}")
    return value


def validated(value: object) -> dict[str, object]:
    """The config, refused with the first offending key unless strictly well formed."""
    if not isinstance(value, dict) or value.get("version") != 1:
        raise ReplayError(f"{NAME} must be an object with version 1")
    unknown = set(value) - KEYS
    if unknown:
        raise ReplayError(f"{NAME}: unknown keys {', '.join(sorted(unknown))}")
    data: dict[str, object] = value
    if "orientation" in data and data["orientation"] not in ("source", REFRAME):
        raise ReplayError(f'{NAME}: orientation is "source" or {json.dumps(REFRAME)}')
    speakers = section(data, "speakers", SPEAKER_KEYS)
    if speakers and (speakers.get("count", 1) != 1 or type(speakers.get("count", 1)) is not int):
        raise ReplayError(f"{NAME}: speakers.count is 1 (several speakers: multi-speaker flow)")
    if ("icon" in speakers or "iconSha256" in speakers) and "name" not in speakers:
        raise ReplayError(f"{NAME}: a speaker icon needs the speaker's name")
    captions = section(data, "captions", CAPTION_KEYS)
    nameplate = record(captions.get("layout", {})).get("nameplate")
    if nameplate is not None and not {"name", "icon", "iconSha256"} <= set(speakers):
        raise ReplayError(f"{NAME}: a caption nameplate needs speakers.name and speakers.icon")
    audio = section(data, "audio", AUDIO_KEYS)
    if audio.get("delivery", "vocals") not in DELIVERIES:
        raise ReplayError(f"{NAME}: audio.delivery is vocals or original-mix")
    if audio.get("delivery") == "original-mix" and not str(audio.get("intent", "")).strip():
        raise ReplayError(f"{NAME}: audio.intent must say why the music is kept")
    if "loudness" in audio:
        loudness = section(audio, "loudness", LOUDNESS_KEYS)
        if set(loudness) != LOUDNESS_KEYS or not all(
            isinstance(level, (int, float)) and not isinstance(level, bool)
            for level in loudness.values()
        ):
            raise ReplayError(f"{NAME}: audio.loudness needs numeric targetLufs and truePeakDbtp")
    for key in ("repair", "deesser", "judge"):
        if key in audio:
            flag(audio[key], f"audio.{key}")
    if "nonSpeech" in section(data, "cut", frozenset({"nonSpeech"})):
        flag(record(data["cut"])["nonSpeech"], "cut.nonSpeech", fixed=True)
    cleanup = section(data, "cleanup", frozenset({"sourceCaptions", "commentBoxes"}))
    if "sourceCaptions" in cleanup:
        flag(cleanup["sourceCaptions"], "cleanup.sourceCaptions", fixed=True)
    if "commentBoxes" in cleanup:
        flag(cleanup["commentBoxes"], "cleanup.commentBoxes")
    if "clips" in data:
        flag(data["clips"], "clips")
    if "channelLabel" in data and (
        not isinstance(data["channelLabel"], str) or not data["channelLabel"].strip()
    ):
        raise ReplayError(f"{NAME}: channelLabel is a non-empty string")
    glossary = data.get("asrGlossary", [])
    if not isinstance(glossary, list) or not all(
        isinstance(item, str) and Path(item).is_absolute() for item in glossary
    ):
        raise ReplayError(f"{NAME}: asrGlossary is a list of absolute paths")
    if "verticals" in data:
        verticals(data["verticals"])
    return data


def verticals(value: object) -> tuple[tuple[int, int, int, int], list[tuple[float, float]]]:
    """The vertical excerpts' inset box and delivery-second ranges, refused unless well formed."""
    data = record(value)
    inset, ranges = data.get("inset"), data.get("ranges")
    if (
        set(data) != VERTICAL_KEYS
        or not isinstance(inset, list)
        or len(inset) != 4
        or not all(type(item) is int and item >= 0 for item in inset)
        or min(inset[2:]) < 2
    ):
        raise ReplayError(f"{NAME}: verticals needs inset [x, y, width, height] and ranges")
    if (
        not isinstance(ranges, list)
        or not ranges
        or not all(
            isinstance(item, list)
            and len(item) == 2
            and all(type(bound) in (int, float) and bound >= 0 for bound in item)
            and item[0] < item[1]
            for item in ranges
        )
    ):
        raise ReplayError(f"{NAME}: verticals.ranges is a list of [start, end] seconds")
    x, y, width, height = inset
    return (x, y, width, height), [(float(start), float(end)) for start, end in ranges]


def load(edit_root: Path) -> dict[str, object] | None:
    """The recording's validated config, or None without the file."""
    path = edit_root / NAME
    if not path.exists():
        return None
    return validated(json.loads(path.read_text(encoding="utf-8")))


def setting(edit_root: Path, name: str) -> tuple[bool, object]:
    """``(present, value)`` of one top-level setting; refuses a side file holding it too."""
    config = load(edit_root)
    side = SIDE_FILES.get(name)
    present = config is not None and name in _settings(config)
    if present and side is not None and (edit_root / side).exists():
        raise ReplayError(f"{edit_root.name}: {name} is set by both {NAME} and {side}")
    return present, _settings(config).get(name) if config is not None else None


def _settings(config: dict[str, object]) -> dict[str, object]:
    """Top-level settings plus the caption sub-settings, keyed as ``SIDE_FILES`` names them."""
    captions = record(config.get("captions", {}))
    return {**config, **{key: captions[key] for key in ("palette", "layout") if key in captions}}


def caption_layout(edit_root: Path) -> tuple[bool, dict[str, object] | None]:
    """The layout in ``caption_layout``'s form: a nameplate becomes its ``speaker``."""
    present, value = setting(edit_root, "layout")
    if not present:
        return False, None
    layout = {"version": 1, **{k: v for k, v in record(value).items() if k != "nameplate"}}
    if "nameplate" in record(value):
        speaker = record(record(load(edit_root))["speakers"])
        layout["speaker"] = {
            "name": speaker["name"], "icon": speaker["icon"], "iconSha256": speaker["iconSha256"],
            **record(record(value)["nameplate"]),
        }  # fmt: skip
    return True, layout


def delivery_audio(edit_root: Path) -> tuple[bool, dict[str, object] | None]:
    """The delivered-audio intent in ``caption_layout``'s form; None keeps the voice."""
    present, value = setting(edit_root, "audio")
    if not present or record(value).get("delivery", "vocals") == "vocals":
        return present, None
    audio = record(value)
    return True, {"version": 1, "source": audio["delivery"], "intent": audio["intent"]}


def value(edit_root: Path, name: str, default: object) -> object:
    """A setting's value, ``default`` when the config does not give it."""
    present, found = setting(edit_root, name)
    return found if present else default


def main(argv: list[str] | None = None) -> int:
    """``python -m replay.video_config EDIT_ROOT``: validate and print the recording's config."""
    arguments = sys.argv[1:] if argv is None else argv
    if len(arguments) != 1:
        raise SystemExit(f"Usage: python -m replay.video_config EDIT_ROOT  (checks {NAME})")
    print(json.dumps(load(Path(arguments[0])), ensure_ascii=False, indent=1))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
