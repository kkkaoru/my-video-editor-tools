"""Show, set and resolve a delivery's caption position, and build its face track.

    python -m replay.caption_position show MOVIE
    python -m replay.caption_position set MOVIE --mode aboveFace --offset-y -40
    python -m replay.caption_position resolve MOVIE
    python -m replay.caption_position faces build WORK-DIR MOVIE

``set`` changes only the options it is given and keeps the others of the existing
``<stem>.caption-layout.json``; ``resolve`` prints how many titles each basis placed (and with
``--titles`` every resolved title) for the plan beside the movie (``replay.caption_layout_file``).
"""

import argparse
import json
from collections import Counter
from collections.abc import Sequence
from pathlib import Path

from replay import caption_faces, caption_layout_file

OPTIONS = {
    "offset_x": "offsetX", "offset_y": "offsetY", "margin": "margin", "face_gap": "faceGap",
    "min_score": "minScore", "max_drift": "maxDrift",
}  # fmt: skip


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("show").add_argument("movie", type=Path)
    changed = commands.add_parser("set")
    changed.add_argument("movie", type=Path)
    changed.add_argument("--mode", choices=caption_layout_file.MODES)
    for option in OPTIONS:
        changed.add_argument(f"--{option.replace('_', '-')}", dest=option, type=float)
    resolved = commands.add_parser("resolve")
    resolved.add_argument("movie", type=Path)
    resolved.add_argument("--titles", action="store_true")
    faces = commands.add_parser("faces").add_subparsers(dest="faces", required=True)
    built = faces.add_parser("build")
    built.add_argument("work", type=Path)
    built.add_argument("movie", type=Path)
    args = parser.parse_args(argv)
    movie = args.movie.resolve()
    if args.command == "show":
        layout = caption_layout_file.read(movie)
        print(json.dumps({**layout, **caption_layout_file.settings(layout)}, indent=1))
    elif args.command == "set":
        layout = dict(caption_layout_file.read(movie))
        if args.mode is not None:
            layout["mode"] = args.mode
        layout.update({key: getattr(args, option) for option, key in OPTIONS.items()
                       if getattr(args, option) is not None})  # fmt: skip
        print(caption_layout_file.write(layout, movie))
    elif args.command == "resolve":
        titles = caption_layout_file.resolve_movie(movie)
        summary: dict[str, object] = {
            "titles": len(titles),
            "basis": dict(Counter(str(t["basis"]) for t in titles)),
        }
        print(json.dumps({**summary, **({"resolved": titles} if args.titles else {})}, indent=1))
    else:
        print(caption_faces.write(caption_faces.build(args.work.resolve(), movie), movie))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
