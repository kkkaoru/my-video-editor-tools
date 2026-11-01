"""Durable requests to redo a recording's processing from chosen steps (``recording_queue redo``).

A request is ``<queue directory>/redo/<recording>.json`` (``{"version": 1, "from": [STEP, ...],
"chunks": [N, ...], "requestedAt": T}``). ``recording_queue redo`` writes it and drains a running
pipeline (reason ``relaunch``), which stops at its next step boundary. The runner's next tick
launches the recording with ``run --force STEP`` for every requested step (each redone with its
dependents; ``apply_forcing`` keeps its published-evidence guard) and removes the request once
the launch was dispatched.

``--chunks`` with ``--from cleanup`` redoes only those cleanup chunks: at launch their state,
media and log, the joined-source consumer and the cleanup receipt are set aside (renamed, not
deleted, under the cleanup's ``interrupted/`` directories and ``steps/interrupted/``, each move
logged to ``deletions.md`` beside the queue), and the steps that consume the cleanup are forced.
The cleanup step then redoes only the set-aside chunks and joins the source again, which needs
every other chunk's media: a recording whose chunk media was retired after its join is refused
(redo the whole cleanup instead), as is a caption-free source, which has no chunks.
"""

import json
import os
import time
from dataclasses import dataclass
from pathlib import Path

from replay import recording_pipeline as pipelines
from replay import recording_steps as steps
from replay.core import ReplayError
from replay.recordings import record

DIRECTORY = "redo"
LOG = "deletions.md"
CLEANUP = "cleanup"


@dataclass(frozen=True, slots=True)
class Request:
    """Steps to redo and, for ``cleanup``, the only chunks to redo."""

    names: tuple[str, ...]
    chunks: tuple[int, ...] = ()


def path(queue: Path, replay_id: str) -> Path:
    return queue.parent / DIRECTORY / f"{replay_id}.json"


def validated(names: tuple[str, ...], chunks: tuple[int, ...]) -> Request:
    """A request whose steps are pipeline steps; chunks only with ``cleanup``."""
    unknown = [name for name in names if name not in pipelines.DEPENDENCIES]
    if not names or unknown:
        raise ReplayError(
            f"Redo needs --from steps of {', '.join(pipelines.ORDER)}; "
            f"unknown: {', '.join(unknown) or 'none given'}"
        )
    if chunks and CLEANUP not in names:
        raise ReplayError("--chunks redoes cleanup chunks; give it with --from cleanup")
    if any(chunk < 0 for chunk in chunks) or len(set(chunks)) != len(chunks):
        raise ReplayError("--chunks takes distinct chunk numbers (0 or more)")
    return Request(tuple(dict.fromkeys(names)), tuple(sorted(chunks)))


def write(queue: Path, replay_id: str, request: Request) -> Path:
    """Store the request (a newer one replaces a pending one)."""
    target = path(queue, replay_id)
    target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    staged = target.with_name(f".{target.name}.partial")
    staged.write_text(
        json.dumps({"version": 1, "from": list(request.names), "chunks": list(request.chunks),
                    "requestedAt": round(time.time(), 1)}) + "\n",
        encoding="utf-8",
    )  # fmt: skip
    os.replace(staged, target)
    return target


def pending(queue: Path, replay_id: str) -> Request | None:
    """The recording's stored request, or None."""
    target = path(queue, replay_id)
    if not target.is_file():
        return None
    data = record(json.loads(target.read_text(encoding="utf-8")))
    names, chunks = data.get("from"), data.get("chunks", [])
    if (
        data.get("version") != 1
        or not isinstance(names, list)
        or not isinstance(chunks, list)
        or not all(type(chunk) is int for chunk in chunks)
    ):
        raise ReplayError(f"{target} is not a redo request")
    return validated(tuple(str(name) for name in names), tuple(chunks))


def clear(queue: Path, replay_id: str) -> None:
    path(queue, replay_id).unlink(missing_ok=True)


def forcing(request: Request) -> pipelines.Forcing:
    """The steps to force: with chunks, the cleanup's consumers instead of the whole cleanup."""
    if not request.chunks:
        return pipelines.Forcing(request.names)
    consumers = [name for name, upstream in pipelines.DEPENDENCIES.items() if CLEANUP in upstream]
    others = [name for name in request.names if name != CLEANUP]
    return pipelines.Forcing(tuple(dict.fromkeys([*consumers, *others])))


def set_aside_chunks(
    pipeline: steps.Pipeline, chunks: tuple[int, ...], log: Path, *, now: float | None = None
) -> list[tuple[Path, Path]]:
    """Rename the chunks' state, media and log, the consumer and the cleanup receipt aside."""
    if not pipeline.description.exists():
        raise ReplayError("This recording's cleanup has no chunks (a caption-free source)")
    work, media = steps.cleanup_layout(pipeline.description)
    names = [f"chunk-{chunk:05d}" for chunk in chunks]
    missing = [name for name in names if not (work / "configs" / f"{name}.json").is_file()]
    if missing:
        raise ReplayError(f"No such cleanup chunks: {', '.join(missing)}")
    kept = [
        state.name
        for state in sorted((work / "state").glob("chunk-*"))
        if state.name not in names and not (media / "sampled" / state.name).exists()
    ]
    if kept:
        raise ReplayError(
            f"The media of {len(kept)} other chunks was retired after the join (e.g. {kept[0]}); "
            "joining again needs it: redo the whole cleanup with --from cleanup"
        )
    stamp = round(time.time() if now is None else now)
    moves = [
        *(
            (origin / name, origin.parent / "interrupted" / f"{name}-redo-{stamp}")
            for name in names
            for origin in (work / "state", media / "sampled")
        ),
        *(
            (work / "logs" / f"{name}.log", work / "logs" / f"{name}.redo-{stamp}.log")
            for name in names
        ),
        (work / "consumer", work / "interrupted" / f"consumer-redo-{stamp}"),
        (pipeline.steps / "cleanup.json", pipeline.steps / "interrupted" / f"cleanup-{stamp}.json"),
    ]
    done = []
    for origin, target in moves:
        if not origin.exists():
            continue
        target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        origin.rename(target)
        done.append((origin, target))
    with log.open("a", encoding="utf-8") as stream:
        stream.write(
            f"\n- {time.strftime('%Y-%m-%d %H:%M', time.localtime(stamp))} (epoch {stamp}), "
            f"recording_queue redo: cleanup chunks {' '.join(f'{c:05d}' for c in chunks)} of "
            f"{pipeline.text('recording')} set aside for redo (not deleted):\n"
            + "".join(f"  - {origin} -> {target}\n" for origin, target in done)
        )
    return done
