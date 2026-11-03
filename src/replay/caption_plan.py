"""A delivery's caption plan: the style-20 titles a player draws live over the delivery movie.

``<movie stem>.captions.json`` beside the delivery movie is the one contract the web viewer and
the Mac TranscriptViewer read (``docs/caption-plan.md``). It is built only from a recording's
pipeline evidence, never inferred:

- the titles and style of every caption part, read from its finished ``.moti`` by the pinned
  native renderer's ``--inspect`` (``captions.json`` ``nativeRender``), with each part's
  ``offsetFrame`` added so every frame is on the delivery clock;
- the delivery's canvas, rate and frame count (ffprobe), which must equal the compose plan's
  frames and every part's canvas;
- ``burned``: whether the compose requests laid each part's alpha movie over the delivery, at
  exactly that part's offset (every part or none);
- the nameplate of the recording's ``layout.speaker``: its pinned icon is copied beside the plan.

    python -m replay.caption_plan build WORK-DIR DELIVERY-MOVIE

``WORK-DIR`` is ``edits/work/recordings/<recording>/<recording>``; the movie must be the one the
recording's ``publish`` or ``youtube-master`` step recorded. A plan already beside the movie is
kept when identical and refused when it differs.
"""

import argparse
import hashlib
import json
import math
import os
import re
import subprocess
from collections.abc import Callable, Sequence
from fractions import Fraction
from pathlib import Path

import regex

from replay import caption_layout, caption_render_native
from replay.core import ReplayError
from replay.recordings import record

VERSION = 1
CLOCK = "delivery"
SUFFIX = ".captions.json"
CHAIN_TOLERANCE = (
    1  # frames: a placed cue's seconds round to the frame its title starts on or before
)
STYLE_KEYS = frozenset(
    {"fontName", "fontSize", "faceGrow", "keyline", "whiteBorder", "rim", "shadowOpacity",
     "shadowOffsetY", "shadowSigma", "gradientTop", "gradientBottom", "topLocation",
     "bottomLocation", "outputGamma"}
)  # fmt: skip
TITLE_KEYS = frozenset({"text", "startFrame", "endFrame", "x", "y", "graphemeFrames"})
TITLE_OPTIONAL = frozenset({"style", "alignment", "speaker"})
PLAN_KEYS = frozenset(
    {"version", "clock", "movie", "burned", "width", "height", "rate", "frames", "style",
     "titles", "nameplate"}
)  # fmt: skip
NAMEPLATE_KEYS = frozenset(
    {"burned", "icon", "iconSha256", "iconX", "iconY", "iconSize", "ringWidth", "ringColour",
     "name", "nameFont", "nameFontSha256", "nameFontVariation", "nameSize", "nameColour",
     "nameStrokeWidth", "nameStrokeColour", "nameX", "nameY", "nameAnchor"}
)  # fmt: skip
WHITE, BLACK = "#FFFFFF", "#000000"
SPEAKERS_VERSION = 2  # several speakers (``docs/caption-plan.md`` "Multiple speakers")
SPEAKER_KEYS = frozenset({"id", "name", "colorTop", "colorBottom", "laneX"})
SPEAKER_ICON_KEYS = frozenset({"icon", "iconSha256"})
UNASSIGNED = "unassigned"  # reserved: the titles without a speaker
COLOUR = re.compile(r"#[0-9A-Fa-f]{6}")

Run = Callable[[Sequence[str]], str]


def sha256(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def read_json(path: Path) -> dict[str, object]:
    return record(json.loads(path.read_text(encoding="utf-8")))


def rows(value: object, name: str) -> list[dict[str, object]]:
    if not isinstance(value, list):
        raise ReplayError(f"{name} must be a list")
    return [record(item) for item in value]


def integer(value: object, name: str) -> int:
    if type(value) is not int:
        raise ReplayError(f"{name} must be an integer")
    return value


def real(value: object, name: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        raise ReplayError(f"{name} must be a finite number")
    return float(value)


def graphemes(text: str) -> int:
    """Extended grapheme clusters, as Swift's ``String.count`` (a line break counts as one)."""
    return len(regex.findall(r"\X", text))


def style(value: object) -> dict[str, object]:
    data = record(value)
    if set(data) != STYLE_KEYS:
        raise ReplayError(f"A caption style has exactly {', '.join(sorted(STYLE_KEYS))}")
    for key in STYLE_KEYS - {"fontName", "gradientTop", "gradientBottom"}:
        real(data[key], key)
    for key in ("fontName", "gradientTop", "gradientBottom"):
        if not isinstance(data[key], str) or not data[key]:
            raise ReplayError(f"Caption style {key} must be a string")
    return data


def title(value: object) -> dict[str, object]:
    """A title as ``MotionCaptionTitle.validate()`` accepts it."""
    data = record(value)
    if not TITLE_KEYS <= set(data) <= TITLE_KEYS | TITLE_OPTIONAL:
        raise ReplayError(f"A caption title has {', '.join(sorted(TITLE_KEYS))}")
    text, start, end = data["text"], data["startFrame"], data["endFrame"]
    if not isinstance(text, str) or not text:
        raise ReplayError("A caption title needs text")
    start, end = integer(start, "startFrame"), integer(end, "endFrame")
    onsets = [integer(item, "graphemeFrames") for item in rows_of(data["graphemeFrames"])]
    if not 0 <= start < end:
        raise ReplayError(f"Caption title {text!r} has an empty or negative span")
    if len(onsets) != graphemes(text):
        raise ReplayError(f"Caption title {text!r} needs one onset frame per grapheme")
    if not all(start <= item < end for item in onsets) or onsets != sorted(onsets):
        raise ReplayError(f"Caption title {text!r} has onsets outside its span or out of order")
    real(data["x"], "x")
    real(data["y"], "y")
    if "style" in data:
        style(data["style"])
    if data.get("alignment", "left") != "left":
        raise ReplayError("A caption title's alignment is left or absent (centred)")
    if data.get("speaker") is not None and not isinstance(data["speaker"], str):
        raise ReplayError("A caption title's speaker is a name or null")
    return data


def rows_of(value: object) -> list[object]:
    if not isinstance(value, list):
        raise ReplayError("graphemeFrames must be a list")
    return value


def nameplate(value: object) -> dict[str, object] | None:
    if value is None:
        return None
    data = record(value)
    if set(data) != NAMEPLATE_KEYS or not isinstance(data["burned"], bool):
        raise ReplayError(f"A nameplate has exactly {', '.join(sorted(NAMEPLATE_KEYS))}")
    icon = data["icon"]
    if not isinstance(icon, str) or not icon or Path(icon).name != icon:
        raise ReplayError("The nameplate icon is a file name beside the plan")
    for key in ("iconX", "iconY", "iconSize", "ringWidth", "nameSize", "nameStrokeWidth"):
        integer(data[key], key)
    real(data["nameX"], "nameX")
    real(data["nameY"], "nameY")
    return data


def speakers(value: object) -> list[dict[str, object]]:
    """A version-2 speaker table: at least two speakers in lane order, unique ids."""
    table = rows(value, "speakers")
    for item in table:
        if (
            not SPEAKER_KEYS <= set(item) <= SPEAKER_KEYS | SPEAKER_ICON_KEYS
            or len(set(item) & SPEAKER_ICON_KEYS) == 1
        ):
            raise ReplayError(
                f"A speaker has {', '.join(sorted(SPEAKER_KEYS))} (+ icon, iconSha256)"
            )
        for key in ("id", "name", *(set(item) & SPEAKER_ICON_KEYS)):
            if not isinstance(item[key], str) or not str(item[key]).strip():
                raise ReplayError(f"A speaker's {key} must be a non-empty string")
        if (
            item["id"] == UNASSIGNED
            or "icon" in item
            and Path(str(item["icon"])).name != item["icon"]
        ):
            raise ReplayError(f"Speaker {item['id']!r}: a reserved id or an icon path")
        if not all(COLOUR.fullmatch(str(item[key])) for key in ("colorTop", "colorBottom")):
            raise ReplayError(f"Speaker {item['id']!r} colours are #RRGGBB")
        real(item["laneX"], "laneX")
    ids = [item["id"] for item in table]
    if len(ids) < SPEAKERS_VERSION or len(set(ids)) != len(ids):
        raise ReplayError("A version-2 plan has at least two speakers with unique ids")
    return table


def validate(value: object) -> dict[str, object]:
    """A version-1 or version-2 plan, refused unless every title fits the delivery in display
    order: version 1's titles never overlap; version 2's start in order, one speaker's titles
    never overlap, and unassigned titles may (readers stack them)."""
    plan = record(value)
    version = plan.get("version")
    keys = PLAN_KEYS | ({"speakers"} if version == SPEAKERS_VERSION else set())
    if set(plan) != keys or version not in (VERSION, SPEAKERS_VERSION) or plan["clock"] != CLOCK:
        raise ReplayError(f"A caption plan has exactly {', '.join(sorted(keys))}, version 1 or 2")
    if not isinstance(plan["movie"], str) or Path(plan["movie"]).name != plan["movie"]:
        raise ReplayError("The plan's movie is the delivery's file name")
    if not isinstance(plan["burned"], bool):
        raise ReplayError("burned must be true or false")
    for key in ("width", "height", "rate", "frames"):
        if integer(plan[key], key) <= 0:
            raise ReplayError(f"The plan's {key} must be positive")
    style(plan["style"])
    ids = {str(item["id"]) for item in speakers(plan["speakers"])} if "speakers" in plan else set()
    ends: dict[object, int] = {}  # per speaker (version 1: one lane), the last title's end
    first = 0
    for item in rows(plan["titles"], "titles"):
        title(item)
        start, end = int(str(item["startFrame"])), int(str(item["endFrame"]))
        lane = item.get("speaker") if ids else None
        if lane is not None and lane not in ids:
            raise ReplayError(f"Caption title {item['text']!r} names an unknown speaker")
        stacked = ids and lane is None
        if (
            start < first
            or not stacked
            and start < ends.get(lane, 0)
            or end > int(str(plan["frames"]))
        ):
            raise ReplayError(f"Caption title {item['text']!r} overlaps another or the end")
        first = start if ids else end
        if not stacked:
            ends[lane] = end
    nameplate(plan["nameplate"])
    return plan


def recorded_movie(root: Path, movie: Path) -> None:
    """Refuse a movie that is not the one the recording's publication recorded."""
    known: list[Path] = []
    # live-publish: a live-caption sibling's delivery (``replay.live_caption_delivery``).
    for step, key in (
        ("publish", "delivery"),
        ("youtube-master", "master"),
        ("live-publish", "delivery"),
    ):
        receipt = root / "steps" / f"{step}.json"
        if receipt.is_file():
            output = record(read_json(receipt)["outputs"])[key]
            known.append(Path(str(record(output)["path"] if isinstance(output, dict) else output)))
    if movie.resolve() in {path.resolve() for path in known}:
        return
    # A master ``replay.youtube_master`` made outside the pipeline records its delivery beside it.
    proof = movie.parent.parent / "metadata" / "reencode-verification.json"
    if proof.is_file():
        verified = read_json(proof)
        master = Path(str(record(verified["master"])["path"])).resolve()
        source = Path(str(record(verified["delivery"])["path"])).resolve()
        if master == movie.resolve() and source in {path.resolve() for path in known}:
            return
    raise ReplayError(f"{movie} is not a delivery recorded by {root / 'steps'}")


def probed(movie: Path, ffprobe: str) -> tuple[int, int, int, int]:
    """Width, height, integer rate and frame count of the delivery's video stream."""
    done = subprocess.run(
        [ffprobe, "-v", "error", "-select_streams", "v:0", "-show_entries",
         "stream=width,height,r_frame_rate,nb_frames,start_time", "-of", "json", str(movie)],
        capture_output=True, text=True, check=True,
    )  # fmt: skip
    stream = rows(read_stream(done.stdout), "streams")
    if len(stream) != 1:
        raise ReplayError(f"{movie} has no single video stream")
    data = stream[0]
    rate = Fraction(str(data["r_frame_rate"]))
    if rate.denominator != 1:
        raise ReplayError(f"{movie} is not at an integer frame rate")
    if Fraction(str(data["start_time"])) != 0:  # players map frame f to f / rate seconds
        raise ReplayError(f"{movie} does not start at time zero")
    return (integer(data["width"], "width"), integer(data["height"], "height"),
            int(rate), int(str(data["nb_frames"])))  # fmt: skip


def read_stream(text: str) -> object:
    return record(json.loads(text)).get("streams")


def recorded_parts(work: Path, root: Path) -> list[dict[str, object]]:
    """The parts the caption-parts step pinned."""
    path = work / "caption-parts-v1" / "parts.json"
    pin = record(record(read_json(root / "steps" / "caption-parts.json")["outputs"])["parts"])
    if sha256(path) != pin["sha256"]:
        raise ReplayError(f"{path} differs from the caption-parts receipt's pin")
    parts = rows(json.loads(path.read_text(encoding="utf-8")), "parts")
    if not parts:
        raise ReplayError("The recording has no caption parts")
    return parts


def unchanged_since_render(root: Path, parts: list[dict[str, object]]) -> None:
    """Refuse burned parts whose ``.moti`` changed after the caption render started (a part
    staged again no longer shows what was burned)."""
    render = root / "steps" / "caption-render.json"
    if not render.is_file():
        return
    started = real(read_json(render)["startedAt"], "startedAt")
    for part in parts:
        if Path(str(part["moti"])).stat().st_mtime > started:
            raise ReplayError(f"{part['name']}: its .moti changed after the caption render")


def inspected(parts: list[dict[str, object]], binary: Path, run: Run) -> list[dict[str, object]]:
    documents = []
    for part in parts:
        document = record(json.loads(run([str(binary), "--inspect", str(part["moti"])])))
        if document["frames"] != part["frames"]:
            raise ReplayError(f"{part['name']}: its .moti is not the part's length")
        documents.append(document)
    return documents


def burned_layers(work: Path, root: Path, parts: list[dict[str, object]], rate: int) -> bool:
    """Whether compose laid every part's alpha at its offset (none: false; some: refused)."""
    groups_pin = record(record(read_json(root / "steps" / "compose.json")["outputs"])["groups"])
    groups_path = Path(str(groups_pin["path"]))
    if sha256(groups_path) != groups_pin["sha256"]:
        raise ReplayError(f"{groups_path} differs from the compose receipt's pin")
    renders = {str(part["renderDir"]).rstrip("/") + "/": part for part in parts}
    laid: set[str] = set()
    start = 0
    for group in rows(json.loads(groups_path.read_text(encoding="utf-8")), "groups"):
        request = read_json(work / "compose-v1" / "requests" / f"{group['name']}.json")
        for layer in rows(record(request["recipe"]).get("additionalVideo", []), "layers"):
            source = str(layer["sourcePath"])
            part = next((p for prefix, p in renders.items() if source.startswith(prefix)), None)
            if part is None:
                continue  # a nameplate or other full-frame layer
            selection = record(layer["selection"])
            offset = (
                start
                + round(real(layer["offsetSeconds"], "offsetSeconds") * rate)
                - round(real(selection["startSeconds"], "startSeconds") * rate)
            )
            if offset != part["offsetFrame"]:
                raise ReplayError(f"{part['name']} was composed at frame {offset}, not its offset")
            laid.add(str(part["name"]))
        start += integer(group["frames"], "frames")
    if laid and len(laid) != len(parts):
        raise ReplayError("Only some caption parts were composed into the delivery")
    return bool(laid)


def composed_frames(root: Path) -> int:
    outputs = record(read_json(root / "steps" / "compose.json")["outputs"])
    groups = json.loads(Path(str(record(outputs["groups"])["path"])).read_text(encoding="utf-8"))
    return sum(integer(row["frames"], "frames") for row in rows(groups, "groups"))


def speaker_plate(
    root: Path, movie: Path, layout: dict[str, object]
) -> tuple[dict[str, object], Path]:
    """The nameplate parameters ``caption_layout.badge_image`` draws, and its pinned icon."""
    person = record(layout["speaker"])
    icon = caption_layout.pinned(person, "icon")
    if icon.suffix.lower() != ".png":
        raise ReplayError(f"The nameplate icon {icon} is not a PNG")
    font = caption_layout.pinned(person, "font")
    size = round(real(person["iconSize"], "iconSize"))
    left, top = round(real(person["iconX"], "iconX")), round(real(person["iconY"], "iconY"))
    name_size = int(real(person["nameSize"], "nameSize"))  # Pillow's truetype size
    compose = record(read_json(root / "steps" / "compose.json")["outputs"])
    return {
        "burned": "badge" in compose,
        "icon": f"{movie.stem}.nameplate.png",
        "iconSha256": person["iconSha256"],
        "iconX": left, "iconY": top, "iconSize": size,
        "ringWidth": caption_layout.RING, "ringColour": WHITE,
        "name": person["name"],
        "nameFont": font.name, "nameFontSha256": person["fontSha256"],
        "nameFontVariation": caption_layout.FONT_WEIGHT.decode(),
        "nameSize": name_size, "nameColour": WHITE,
        "nameStrokeWidth": max(2, math.ceil(name_size / 8)), "nameStrokeColour": BLACK,
        "nameX": left + size / 2, "nameY": top + size + caption_layout.NAME_GAP,
        "nameAnchor": "mt",
    }, icon  # fmt: skip


def build(
    work: Path, movie: Path, *, run: Run | None = None, reader: dict[str, object] | None = None
) -> tuple[dict[str, object], Path | None]:
    """The validated plan of ``movie`` and the nameplate icon to copy beside it (or None).

    The ``.moti`` reader is the recording's ``nativeRender`` binary; a recording rendered before
    that setting existed needs an explicit ``reader`` (``binary`` and ``binarySha256``).
    """
    root = work.parent
    recorded_movie(root, movie)
    captions = read_json(root / "captions.json")
    if "nativeRender" not in captions and reader is None:
        raise ReplayError("The recording names no native .moti reader; pass --binary")
    native = record(captions.get("nativeRender", reader))
    binary = caption_render_native.verified_binary(
        Path(str(native["binary"])), str(native["binarySha256"])
    )
    parts = recorded_parts(work, root)
    width, height, rate, frames = probed(movie, str(read_json(root / "pipeline.json")["ffprobe"]))
    if composed_frames(root) != frames:
        raise ReplayError(f"{movie} is not the compose plan's length")
    end = 0
    for part in parts:
        offset = integer(part["offsetFrame"], "offsetFrame")
        if offset < end:
            raise ReplayError(f"{part['name']} starts before the previous part ends")
        end = offset + integer(part["frames"], "frames")
    if end > frames:
        raise ReplayError("The caption parts run past the delivery")
    documents = inspected(parts, binary, run or caption_render_native.run_checked)
    common = style(documents[0]["style"])
    titles: list[dict[str, object]] = []
    for part, document in zip(parts, documents, strict=True):
        if (document["width"], document["height"], document["rate"]) != (width, height, rate):
            raise ReplayError(f"{part['name']} is not on the delivery's canvas and rate")
        own = style(document["style"])
        offset = int(str(part["offsetFrame"]))
        for item in rows(document["titles"], "titles"):
            shown = title(item)
            onsets = [int(str(at)) + offset for at in rows_of(shown["graphemeFrames"])]
            if int(str(shown["endFrame"])) > int(str(part["frames"])):
                raise ReplayError(f"{part['name']}: a title runs past its part")
            titles.append({
                **shown,
                "startFrame": int(str(shown["startFrame"])) + offset,
                "endFrame": int(str(shown["endFrame"])) + offset,
                "graphemeFrames": onsets,
                **({"style": own} if own != common and "style" not in shown else {}),
            })  # fmt: skip
    plate, icon = (None, None)
    layout = captions.get("layout")
    if layout is not None and "speaker" in caption_layout.layout(layout):
        plate, icon = speaker_plate(root, movie, record(layout))
    burned = burned_layers(work, root, parts, rate)
    if burned:
        unchanged_since_render(root, parts)
    plan = validate({
        "version": VERSION, "clock": CLOCK, "movie": movie.name,
        "burned": burned,
        "width": width, "height": height, "rate": rate, "frames": frames,
        "style": common, "titles": titles, "nameplate": plate,
    })  # fmt: skip
    return plan, icon


def flat(text: str) -> str:
    return text.replace("\n", "")


def chain_title(
    titles: list[dict[str, object]], start: int, text: str, last: int | None
) -> int | None:
    """The title from the last shown one on that shows a chain: its text leads the title's, it
    starts within ``CHAIN_TOLERANCE`` of the title (or inside the last shown title) and
    before the title ends; the nearest start wins, the later title on a tie (a short repeated
    word is its own title)."""
    found: int | None = None
    for index in range(last or 0, len(titles)):
        title = titles[index]
        begin = int(str(title["startFrame"]))
        if begin - CHAIN_TOLERANCE > start:
            break
        # The current title also shows a later chain growing it (a cue that started a frame late).
        near = abs(begin - start) <= CHAIN_TOLERANCE or index == last and begin <= start
        fits = near and start < int(str(title["endFrame"]))
        if fits and flat(str(title["text"])).startswith(flat(text)):
            nearest = found is None or abs(begin - start) <= abs(
                int(str(titles[found]["startFrame"])) - start
            )
            found = index if nearest else found
    return found


def chain_failure(plan: dict[str, object], chains: Sequence[tuple[str, int, int]]) -> str | None:
    """Why the plan does not show the displayed caption chains the SRT was written from, or None.

    Every chain (``pipeline_completion.displayed``: final text and frames) must be shown, in
    order, by one title whose text it leads (a title may show several chains that grow it), and
    every title must show at least one chain.
    """
    titles = rows(validate(plan)["titles"], "titles")
    shown: set[int] = set()
    last: int | None = None
    for text, start, _ in chains:
        index = chain_title(titles, start, text, last)
        if index is None:
            return f"no title shows the caption {text!r} at frame {start}"
        shown.add(index)
        last = index
    if len(shown) != len(titles):
        return f"{len(titles) - len(shown)} titles show no displayed caption"
    return None


def publish_once(target: Path, data: bytes) -> None:
    """Create ``target`` (0600) atomically; an identical one is kept, a different one refused."""
    if target.exists():
        if target.read_bytes() != data:
            raise ReplayError(f"{target} exists with other content; inspect it")
        return
    staged = target.with_name(f".{target.name}.partial")
    with staged.open("xb") as stream:
        os.fchmod(stream.fileno(), 0o600)
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    try:
        os.link(staged, target)  # never replaces an existing file
    finally:
        staged.unlink()


def write(plan: dict[str, object], delivery_dir: Path, icon: Path | None = None) -> Path:
    """Write the plan beside its movie, with its nameplate icon verified by SHA-256."""
    validate(plan)
    plate = nameplate(plan["nameplate"])
    if (plate is None) != (icon is None):
        raise ReplayError("A nameplate plan needs its icon, and only it")
    if plate is not None and icon is not None:
        data = icon.read_bytes()
        if hashlib.sha256(data).hexdigest() != plate["iconSha256"]:
            raise ReplayError(f"{icon} is not the nameplate's icon")
        publish_once(delivery_dir / str(plate["icon"]), data)
    target = delivery_dir / f"{Path(str(plan['movie'])).stem}{SUFFIX}"
    text = json.dumps(plan, ensure_ascii=False, indent=1, allow_nan=False) + "\n"
    publish_once(target, text.encode("utf-8"))
    return target


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    built = commands.add_parser("build")
    built.add_argument("work", type=Path)
    built.add_argument("movie", type=Path)
    built.add_argument("--binary", type=Path, help="reader for a recording without nativeRender")
    built.add_argument("--binary-sha256")
    args = parser.parse_args(argv)
    reader: dict[str, object] | None = None if args.binary is None else {
        "binary": str(args.binary), "binarySha256": str(args.binary_sha256)
    }  # fmt: skip
    plan, icon = build(args.work.resolve(), args.movie.resolve(), reader=reader)
    print(write(plan, args.movie.resolve().parent, icon))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
