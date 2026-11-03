"""YouTube uploads of finished deliveries, encoded by Compressor's own "Publish to YouTube" setting.

The setting (H.264 High, AAC) keeps the source frame up to 4096x2304, so each orientation gets
its own size from its delivery: a vertical delivery is encoded at 1080x1920 or 720x1280 and a
landscape one at 1920x1080 or 1280x720 (720p allowed by the user, 2026-11-02). A delivery of any
other size is refused rather than rescaled, and every output must keep its delivery's size. A
delivery already replaced by its encode (``replay.youtube_master``) is skipped. The output sits
beside the delivery's own folder, at ``<revision>/youtube/<stem>-youtube.mov`` (outside
``delivery``, so the viewers never list it).
Jobs run one at a time, in the order given, through ``replay.compressor_batch``, which records
each submission and resumes a stopped run. Outside a pipeline step (which holds it already),
``--lease DIR`` takes the machine-wide Compressor lease for each movie in turn, so a recording's
caption render waits for at most one encode.

    python -m replay.youtube_encode --state DIR [--lease DIR] MOVIE [MOVIE ...]
"""

import argparse
import json
import subprocess
import sys
from collections.abc import Sequence
from contextlib import nullcontext
from pathlib import Path

from replay import gpu_lease
from replay.compressor_batch import COMPRESSOR
from replay.core import ReplayError

SETTINGS = Path(
    "/Applications/Compressor Creator Studio.app/Contents/PlugIns/Compressor/CompressorKit.bundle"
    "/Contents/Frameworks/Compressor.framework/Versions/A/Frameworks/StompUI.framework/Versions/A"
    "/Resources/BuiltInSettings/Publish to YouTube/WebShareName.compressorsetting"
)
SIZES = {
    "vertical": frozenset({(1080, 1920), (720, 1280)}),
    "landscape": frozenset({(1920, 1080), (1280, 720)}),
}
DIRECTORY = "youtube"
SUFFIX = "-youtube.mov"
NAME_LIMIT = 120


def frame(movie: Path, ffprobe: str) -> tuple[int, int]:
    found = subprocess.run(
        [ffprobe, "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height",
         "-of", "csv=p=0", str(movie)],
        capture_output=True, text=True, check=True,
    ).stdout.strip()  # fmt: skip
    width, _, height = found.partition(",")
    return int(width), int(height)


def orientation(movie: Path, ffprobe: str) -> str:
    """The delivery's orientation, refused unless it has that orientation's YouTube size."""
    size = frame(movie, ffprobe)
    for name, sizes in SIZES.items():
        if size in sizes:
            return name
    raise ReplayError(f"{movie.name} is {size[0]}x{size[1]}, not a 1080p or 720p frame")


def allowed(size: tuple[int, int]) -> bool:
    return any(size in sizes for sizes in SIZES.values())


def output_of(movie: Path) -> Path:
    return movie.parents[1] / DIRECTORY / f"{movie.stem}{SUFFIX}"


def job_name(movie: Path) -> str:
    """A Compressor job name from the delivery's own (ASCII) name."""
    name = movie.stem.removesuffix("-review")
    return name[-NAME_LIMIT:].lstrip("-_.")


def encode(movies: Sequence[Path], state: Path, *, python: str, ffprobe: str) -> list[Path]:
    """Encode every movie not encoded yet, in order; the outputs, each checked for its size."""
    movies = [movie for movie in movies if movie.exists()]  # the rest became their masters
    for movie in movies:
        orientation(movie, ffprobe)
    expected = {movie: frame(movie, ffprobe) for movie in movies}
    pending = [movie for movie in movies if not output_of(movie).exists()]
    if pending:
        state.mkdir(mode=0o700, parents=True, exist_ok=True)
        jobs = state / "jobs.json"
        rows = [
            {"name": job_name(movie), "source": str(movie), "preset": str(SETTINGS),
             "output": str(output_of(movie))}
            for movie in pending
        ]  # fmt: skip
        jobs.write_text(json.dumps({"jobs": rows}, indent=1) + "\n", encoding="utf-8")
        for movie in pending:
            output_of(movie).parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        subprocess.run(
            [python, "-B", "-m", "replay.compressor_batch", str(jobs), "--state",
             str(state / "state"), "--batch-name", "YouTube", "--in-flight", "1",
             "--executable", str(COMPRESSOR)],
            check=True,
        )  # fmt: skip
    for movie, size in expected.items():
        if frame(output_of(movie), ffprobe) != size:
            raise ReplayError(f"{output_of(movie)} is not {size[0]}x{size[1]}")
    return [output_of(movie) for movie in movies]


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("movies", type=Path, nargs="+")
    parser.add_argument("--state", type=Path, required=True)
    parser.add_argument("--ffprobe", default="ffprobe")
    parser.add_argument("--lease", type=Path, help="the Compressor lease directory, per movie")
    args = parser.parse_args(argv)
    for movie in args.movies:
        held = (
            nullcontext()
            if args.lease is None
            else gpu_lease.lease(args.lease, job_name(movie), step="youtube-encode")
        )
        with held:
            for path in encode([movie], args.state, python=sys.executable, ffprobe=args.ffprobe):
                print(path, flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
