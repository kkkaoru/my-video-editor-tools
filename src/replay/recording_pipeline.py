"""Run one recording's pipeline from download to the captioned candidate movie, idempotently.

``start REPLAY_ID --defaults DEFAULTS.json`` binds the recording (``edit_completion init`` when
its edit root is new), writes ``<root>/replay-<id>/pipeline.json`` and launches ``run``
detached through ``scripts/tmux-job.sh``. ``run`` executes the step graph with
``replay.workflow_parallel`` so the video and audio branches overlap and GPU steps share the GPU
while its measured utilization leaves room:

    clocks ─┬─ cleanup-bootstrap ── cleanup ─────────────┐
            ├─ separation (GPU) ── asr (GPU) ─────────────┴─ retire-intermediates
            └─ yolo-track (GPU) ─────────────────────────────┐
    asr ── cut-plan ── paging ── placement (+ yolo-track) ── caption-parts
                       └─ onsets ─────────────────────────────┘
        ── compose (+ cleanup; no captions) ── finish ── publish ── excitement ── upload
        ; publish ── caption-plan (the caption plan and face track the viewers draw from)
        ── summary (the completion gate and supersession, then on-device, then a data sync)
        ── clips (the viewer's clip points are ready) ; summary ── retire-renders (compose)
        ; publish ── youtube-encode (Compressor's YouTube setting, beside the delivery)
        ; youtube-encode + summary + clips ── youtube-master (the verified encode replaces it)
        ── web-hls (the web viewer's HLS ladder of the final delivery)

The caption half (``replay.recording_caption_steps``) reads ``captions.json``, written beside
``pipeline.json`` from the defaults' ``captions`` section; changing it marks only caption steps
stale.

Idempotent and resumable:

- a step whose receipt matches its current inputs (configuration and upstream receipts) is
  skipped; a receipt that predates its inputs stops the run instead of being trusted;
- an interrupted step runs again and reuses everything it finished (``replay.recording_steps``);
- ``--force STEP`` (repeatable) redoes that step and every step downstream of it;
  ``--force-all`` redoes the whole recording. Forced outputs are deleted first;
- once every step passed, ``replay.recording_retirement`` removes the recording's heavy
  intermediates (unless ``RETIREMENT-v1.json`` beside the defaults retains them or holds the
  recording); a later run whose steps need one forces its producer again first.

The candidate is not a delivery: the completion gate assesses it separately.
"""

import argparse
import json
import os
import signal
import subprocess
import sys
import time
from collections.abc import Sequence
from contextlib import ExitStack
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from types import FrameType

from replay import (
    caption_layout,
    closure_guard,
    code_snapshot,
    gpu_lease,
    memory_admission,
    memory_sampler,
    repository,
    step_retry,
    video_config,
    volume_events,
)
from replay import caption_palette as palette
from replay import recording_caption_steps as captions
from replay import recording_priority as priority
from replay import recording_retirement as retirement
from replay import recording_steps as steps
from replay import recording_viewer_steps as viewer
from replay import source_decodable as decodable
from replay import source_lead_in as lead_in
from replay import source_vertical as vertical
from replay.core import ReplayError
from replay.delivery_output_names import STAMP_FORMAT, delivery_stem
from replay.edit_completion import bind_source, bind_storage, load_config
from replay.recordings import JST, record
from replay.stages import Stage, StageError, execution_lock
from replay.workflow_parallel import Children, Result, StageSlots, run_selected

DEFAULT_KEYS = frozenset(
    {
        "version",
        "root",
        "python",
        "providerPython",
        "native",
        "ffmpeg",
        "ffprobe",
        "references",
        "yolo",
        "maxParallel",
        "gpuSlots",
        "captions",
        "deliveryRoot",
    }
)
# Beside the defaults, read when a run finishes: retainIntermediates (default false) and
# retirementHold (edit-root names). A separate file, so drivers and queue runners started before it
# existed keep loading the defaults; never copied into pipeline.json, so no receipt goes stale.
RETIREMENT_SETTINGS = "RETIREMENT-v1.json"
HOUR = 3600
GPU = "gpu"
LAUNCHER = Path("scripts/tmux-job.sh")
MAX_LAUNCHES = 100
ENV = "/usr/bin/env"
DRAINED_EXIT = 75  # EX_TEMPFAIL: stopped by a drain, not failed (the queue does not count it)
# Start order among ready steps: the longest branch first (yolo-track -> cleanup).
ORDER = (
    "audio-clock", "clocks", "yolo-track", "separation", "cleanup-bootstrap", "cleanup", "asr",
    "upload-original",
    "voice-repair", "cut-plan",
    "paging", "onsets", "placement", "caption-parts", "compose", "finish",
    "voice-remux", "publish", "caption-plan",
    "summarize", "excitement", "upload", "summary", "clips", "retire-renders", "youtube-encode",
    "youtube-master", "web-hls",
)  # fmt: skip
DEPENDENCIES = {**steps.DEPENDENCIES, **captions.DEPENDENCIES}
# Start order only, not receipt inputs: summary reuses what summarize gated and generated, so it
# never generates beside it.
# cleanup starts after yolo-track so its chunks read the stored caption boxes instead of
# detecting them again: a cleanup started first gave up waiting and ran YOLO twice (10-28).
SCHEDULE = {
    **DEPENDENCIES,
    # The YouTube encode comes first, then the summary and the clips (user, 2026-11-03).
    "summarize": (*DEPENDENCIES["summarize"], "youtube-encode"),
    "summary": (*DEPENDENCIES["summary"], "summarize"),
    "cleanup": (*DEPENDENCIES["cleanup"], "yolo-track"),
    # The master replaces the delivery only after its gate, summary and clips read it; a redo of
    # those never redoes the master.
    "youtube-master": (*DEPENDENCIES["youtube-master"], "summary", "clips"),
}
OUTER_CAP = 24 * HOUR  # a backstop (stages.Stage allows at most 24 h); IDLE_SECONDS bound each step
TIMEOUTS = dict.fromkeys(
    (
        "audio-clock", "clocks", "separation", "cleanup-bootstrap", "cleanup", "asr", "yolo-track",
        "retire-intermediates", "upload-original", "cut-plan", "voice-repair", "paging",
        "placement", "onsets", "caption-parts", "compose", "finish",
        "voice-remux", "publish", "caption-plan",
        "excitement", "upload", "summarize", "summary", "clips", "retire-renders",
        "youtube-encode", "youtube-master", "web-hls",
    ),
    OUTER_CAP,
)  # fmt: skip
# Every step is bounded by progress rather than a fixed time: it times out only when for this
# long neither its output trees and logs grew nor its processes used a minute of CPU
# (step_progress, workflow_parallel.cpu_minutes); a 10-hour recording's steps run past any fixed
# bound while they keep working. The bounds are the former per-step timeouts, at most two hours
# (a 10-24 cleanup chunk took at most ~16 min; compose may wait for a lease).
IDLE_SECONDS = {
    "audio-clock": HOUR, "clocks": 2 * HOUR, "separation": 2 * HOUR,
    "cleanup-bootstrap": 2 * HOUR, "cleanup": HOUR, "asr": 2 * HOUR, "yolo-track": 2 * HOUR,
    "retire-intermediates": HOUR, "upload-original": 2 * HOUR, "cut-plan": HOUR,
    "voice-repair": 2 * HOUR,
    "paging": 2 * HOUR, "placement": HOUR, "onsets": HOUR, "caption-parts": 2 * HOUR,
    "compose": 2 * HOUR, "finish": 2 * HOUR, "voice-remux": 2 * HOUR,
    "publish": 2 * HOUR, "caption-plan": HOUR,
    "excitement": HOUR, "upload": 2 * HOUR, "summarize": 2 * HOUR, "summary": 2 * HOUR,
    "clips": 2 * HOUR, "retire-renders": HOUR, "youtube-encode": 2 * HOUR,
    "youtube-master": 2 * HOUR, "web-hls": 2 * HOUR,
}  # fmt: skip
# youtube-encode only waits on Compressor, which renders one job at a time machine-wide: it
# takes the one Compressor lease instead of a GPU lease.
# compose runs three native media_edit renders at once; two recordings composing together timed
# out every group (2026-10-23, 10/6 and 10/2), so composes take one machine-wide lease too.
# finish streams the whole movie through the same SSD: beside another compose or finish it took
# 760-2,518 s against 110-330 s alone (2026-10-22/23), so it shares the compose lease.
# voice-repair is computation only on the CPU (no GPU lease).
# clips runs clef and clef-flash on Metal for most of its 30-minute budget: beside a compose or an
# asr its prefill (at the matmul peak alone) slowed to a quarter (11/1, 2026-11-03).
GPU_STEPS = frozenset({"separation", "asr", "yolo-track", "onsets", "compose", "clips"})
SERIAL_STEPS = {
    "compose": "compose-leases",
    "finish": "compose-leases",
    "youtube-encode": "compressor-leases",
    # x264 takes every CPU core: two recordings' ladders at once would only slow both.
    "web-hls": "web-hls-leases",
}
SERIAL_SETTINGS = '{"capacity": 1}\n'


@dataclass(frozen=True, slots=True)
class Layout:
    """Where one recording's pipeline lives."""

    directory: Path

    @property
    def pipeline(self) -> Path:
        return self.directory / "pipeline.json"

    @property
    def steps(self) -> Path:
        return self.directory / "steps"


@dataclass(frozen=True, slots=True)
class Forcing:
    """Steps to redo; ``every`` redoes all of them."""

    names: tuple[str, ...] = ()
    every: bool = False

    def arguments(self) -> list[str]:
        named = [argument for name in self.names for argument in ("--force", name)]
        return [*named, "--force-all"] if self.every else named


NO_FORCING = Forcing()


def load_defaults(path: Path) -> dict[str, object]:
    data = record(json.loads(path.read_text(encoding="utf-8")))
    if set(data) - {steps.GLOSSARY} != DEFAULT_KEYS or data["version"] != 1:
        raise ReplayError("Unsupported recording pipeline defaults")
    steps.glossary_files(data)
    references, yolo = record(data["references"]), record(data["yolo"])
    if set(references) != steps.REFERENCES or set(yolo) != steps.YOLO_KEYS:
        raise ReplayError("Incomplete pipeline references or detector settings")
    if set(record(data["captions"])) - captions.DEFAULT_OPTIONAL != captions.KEYS - {"version"}:
        raise ReplayError("Incomplete caption pipeline defaults")
    for key in ("maxParallel", "gpuSlots"):
        if type(data[key]) is not int or not 1 <= int(str(data[key])) <= 16:
            raise ReplayError(f"{key} must be an integer from 1 to 16")
    return data


def retirement_settings(defaults_path: Path) -> retirement.Settings:
    """The retirement options beside the defaults; without the file, intermediates retire."""
    path = defaults_path.with_name(RETIREMENT_SETTINGS)
    data = record(json.loads(path.read_text(encoding="utf-8"))) if path.is_file() else {}
    if data and (data.get("version") != 1 or set(data) - {"version", "retainIntermediates",
                                                           "retirementHold"}):  # fmt: skip
        raise ReplayError(f"Unsupported retirement settings in {path}")
    retain, hold = data.get("retainIntermediates", False), data.get("retirementHold", [])
    if type(retain) is not bool or not isinstance(hold, list):
        raise ReplayError("retainIntermediates must be a boolean, retirementHold a list")
    if not all(isinstance(name, str) and name for name in hold):
        raise ReplayError("retirementHold names recordings by their edit-root name")
    return retirement.Settings(retain=retain, hold=tuple(str(name) for name in hold))


def force_retired_producers(
    pipeline: steps.Pipeline, names: tuple[str, ...]
) -> list[dict[str, object]]:
    """Force again the producer of every retired intermediate a step about to run would read.

    Never past the published delivery's evidence: a step added after the delivery (no receipt yet)
    reading a retired intermediate refused there, instead of cascading (10-29: voice-repair forced
    separation and removed a delivered YouTube recording's receipts and render).
    """
    removed: list[dict[str, object]] = []
    while True:
        pending = {name for name in names if not (pipeline.steps / f"{name}.json").is_file()}
        needed = retirement.forced_producers(pipeline, pending)
        if not needed:
            return removed
        for producer, consumer, path in needed:
            print(f"retired intermediates: forcing {producer} for {consumer} ({path})", flush=True)
        producers = tuple(sorted({producer for producer, _, _ in needed}))
        removed.extend(apply_forcing(pipeline, Forcing(names=producers)))


def run_scope(forcing: Forcing, names: Sequence[str]) -> set[str]:
    """The steps a run may run, force or remove: all of them, or the forced ones and their
    dependents (a redo never runs, forces or removes a step outside them)."""
    if forcing.every or not forcing.names:
        return set(names)
    return set(downstream(forcing.names))


def not_applicable(
    pipeline: steps.Pipeline,
    names: Sequence[str],
    scope: set[str],
    *,
    published: bool,
    redo: bool,
) -> dict[str, str]:
    """Steps of a published recording the run takes as they are instead of running (step: why).

    A step without a receipt whose dependents' receipts exist was added after them: they were
    computed (and published) without it, so it does not apply, unless a redo's scope holds it.
    Outside a redo's scope a step without a receipt is never run either. An unpublished
    recording runs every unfinished step.
    """
    if not published:
        return {}
    found: dict[str, str] = {}
    for name in names:
        if (pipeline.steps / f"{name}.json").is_file():
            continue
        later = [after for after in downstream((name,)) if after != name
                 and (pipeline.steps / f"{after}.json").is_file()]  # fmt: skip
        if later and (name not in scope or not redo):
            found[name] = f"added after the published receipts of {', '.join(later)}"
        elif name not in scope:
            found[name] = "outside the redo's scope"
    return found


def mark_legacy(pipeline: steps.Pipeline, names: Sequence[str], skipped: dict[str, str]) -> None:
    """Mark the steps added after publication; clear the mark of every other step."""
    directory = pipeline.steps / steps.LEGACY
    for name in names:
        marker = directory / f"{name}.json"
        if skipped.get(name, "").startswith("added after"):
            directory.mkdir(mode=0o700, exist_ok=True)
            marker.write_text(json.dumps({"version": 1, "reason": skipped[name]}) + "\n",
                              encoding="utf-8")  # fmt: skip
        else:
            marker.unlink(missing_ok=True)


def refuse_out_of_scope_producers(
    pipeline: steps.Pipeline, pending: set[str], scope: set[str]
) -> None:
    """Before anything is removed: a retired intermediate a pending step reads may be produced
    again only by a step inside the run's scope."""
    needed = retirement.forced_producers(pipeline, pending)
    outside = sorted({(producer, consumer) for producer, consumer, _ in needed
                      if producer not in scope})  # fmt: skip
    if outside:
        named = ", ".join(f"{producer} (for {consumer})" for producer, consumer in outside)
        raise ReplayError(
            f"Retired intermediates would need {named} outside this redo's scope; "
            "redo from that producer instead"
        )


def broadcast(edit_root: Path) -> int:
    """The recording's validated JST start (epoch ns), from its dated original's name."""
    source = load_config(edit_root).source
    stamp = delivery_stem(source, edit_root.name)[: -len(f"_JST_{edit_root.name}-review")]
    return int(datetime.strptime(stamp, STAMP_FORMAT).replace(tzinfo=JST).timestamp()) * 10**9


def lease_rank(edit_root: Path) -> int:
    """Newest broadcast first; a recording without a dated original ranks by start time."""
    try:
        return gpu_lease.newest_first(broadcast(edit_root))
    except (ReplayError, OSError, ValueError):
        return time.time_ns()


def edit_source(edit_root: Path) -> Path:
    config = record(json.loads((edit_root / "edit-config.json").read_text(encoding="utf-8")))
    source = Path(str(config.get("source")))
    if not source.is_absolute() or not source.is_file():
        raise ReplayError("The edit configuration names no existing source")
    return source


def zero_origin_source(defaults: dict[str, object], edit_root: Path) -> Path:
    """The edit source; one whose video starts late is first rebound to its lead-in copy.

    The copy lives beside the delivery storage (``<deliveryRoot>/source-lead-in/<recording>``),
    so the storage root never contains the source it was validated against. A source in codecs the
    native tools cannot decode (YouTube VP9/Opus) is first copied to decodable ones
    (``source-decodable``); it only feeds the pipeline, since a YouTube config binds its original
    beside its publication record. A source smaller than the delivery canvas is processed at its
    own size: composition scales it up last (``fit``). A recording whose edit root holds the
    editing intent ``reframe.json`` is cropped around its character (``source-vertical``) between
    the decodable and lead-in copies.
    """
    config = load_config(edit_root)
    if config.storage_root is None:
        raise ReplayError("The recording has no bound storage for a lead-in copy")
    tools = {"ffmpeg": str(defaults["ffmpeg"]), "ffprobe": str(defaults["ffprobe"])}
    readable = decodable.prepared(
        config.source, config.storage_root.parent / decodable.DIRECTORY / edit_root.name, **tools
    )
    aspect = vertical.intent(edit_root)
    framed = (
        readable
        if aspect is None
        else vertical.prepared(
            readable,
            config.storage_root.parent / vertical.DIRECTORY / edit_root.name,
            **tools,
            python=str(defaults["python"]),
            yolo=record(defaults["yolo"]),
            native=str(defaults["native"]),
            aspect=aspect,
        )
    )
    derived = lead_in.prepared(
        framed, config.storage_root.parent / lead_in.DIRECTORY / edit_root.name, **tools
    )
    if framed == config.source and derived != config.source:
        bind_source(edit_root, derived)
    return derived


def bind(replay_id: str, python: str, checkout: Path) -> Path:
    """The recording's edit root, initialized from its completed download job when new.

    A YouTube recording (``youtube-<id>``) is bound beforehand from its original and publication
    record (``edit_completion init-youtube``); there is no download job to initialize it from.
    """
    edit_root = checkout / "edits" / steps.recording_name(replay_id)
    if not replay_id.isdigit() and not (edit_root / "edit-config.json").exists():
        raise ReplayError(f"Bind {edit_root.name} first with edit_completion init-youtube")
    if not (edit_root / "edit-config.json").exists():
        subprocess.run(
            [python, "-B", "-m", "replay.edit_completion", "init", str(edit_root),
             "--engine", str(checkout / "src/replay"), "--executor-scope", str(checkout)],
            check=True,
        )  # fmt: skip
    return edit_root


def pipeline_config(
    defaults: dict[str, object], layout: Layout, replay_id: str, edit_root: Path
) -> dict[str, object]:
    glossary = video_config.value(edit_root, steps.GLOSSARY, defaults.get(steps.GLOSSARY))
    return {
        "version": 1,
        "recording": replay_id,
        "editRoot": str(edit_root),
        "source": str(zero_origin_source(defaults, edit_root)),
        "work": str(layout.directory / steps.recording_name(replay_id)),
        "steps": str(layout.steps),
        "python": defaults["python"],
        "providerPython": defaults["providerPython"],
        "native": defaults["native"],
        "ffmpeg": defaults["ffmpeg"],
        "ffprobe": defaults["ffprobe"],
        "references": defaults["references"],
        "yolo": defaults["yolo"],
        # Copied only when set (the video's config, else the defaults): a recording prepared
        # before keeps its asr receipt.
        **({steps.GLOSSARY: glossary} if glossary is not None else {}),
    }


def write_once(path: Path, value: object) -> None:
    """Write a new file, or accept an identical existing one (an idempotent re-prepare)."""
    text = json.dumps(value, indent=2, allow_nan=False) + "\n"
    if path.exists():
        if path.read_text(encoding="utf-8") != text:
            raise ReplayError(f"{path.name} exists with different content; force a new setup")
        return
    with path.open("x", encoding="utf-8") as stream:
        os.fchmod(stream.fileno(), 0o600)
        stream.write(text)


def layout_of(replay_id: str, defaults: dict[str, object], checkout: Path) -> Layout:
    layout = Layout(Path(str(defaults["root"])) / steps.recording_name(replay_id))
    if not layout.directory.resolve().is_relative_to(checkout / "edits/work"):
        raise ReplayError("The pipeline root must live under the checkout's edits/work")
    return layout


def bind_delivery_storage(edit_root: Path, deliveries: Path) -> None:
    """Give the recording its external delivery storage (``<deliveryRoot>/<recording>``) once."""
    if load_config(edit_root).storage_root is not None:
        return
    storage = deliveries / edit_root.name
    storage.mkdir(mode=0o700, parents=True, exist_ok=True)
    bind_storage(edit_root, storage)


def prepare(replay_id: str, defaults_path: Path, *, checkout: Path) -> Layout:
    defaults = load_defaults(defaults_path)
    edit_root = bind(replay_id, str(defaults["python"]), checkout)
    bind_delivery_storage(edit_root, Path(str(defaults["deliveryRoot"])))
    layout = layout_of(replay_id, defaults, checkout)
    layout.steps.mkdir(mode=0o700, parents=True, exist_ok=True)
    write_pipeline(layout.pipeline, pipeline_config(defaults, layout, replay_id, edit_root))
    settings = caption_layout.captions_settings(
        palette.captions_settings(record(defaults["captions"]), palette.intent(edit_root)),
        edit_root,
    )
    write_captions(layout.directory / "captions.json", settings)
    return layout


def write_pipeline(path: Path, config: dict[str, object]) -> None:
    """``write_once``, except that an existing pipeline.json without ``asrGlossary`` is kept as
    it is: a recording prepared before the defaults gained the key keeps its asr receipt."""
    if path.exists() and steps.GLOSSARY not in record(json.loads(path.read_text("utf-8"))):
        config = {key: value for key, value in config.items() if key != steps.GLOSSARY}
    write_once(path, config)


def write_captions(path: Path, settings: dict[str, object]) -> None:
    """``write_once``, except that an existing captions.json lacking optional keys the defaults
    gained later (``captions.DEFAULT_OPTIONAL``) is kept as it is: a recording keeps its settings.
    """
    if path.exists():
        existing = record(json.loads(path.read_text(encoding="utf-8")))
        kept = {
            key: value
            for key, value in settings.items()
            if key not in captions.DEFAULT_OPTIONAL or key in existing
        }
        if existing == kept:
            return
    write_once(path, settings)


def graph_of(name: str) -> steps.Graph:
    return captions.BACK if name in captions.STEPS else steps.FRONT


def downstream(names: Sequence[str]) -> list[str]:
    """The named steps and every step that depends on them, in graph order."""
    chosen = set(names)
    unknown = chosen - set(DEPENDENCIES)
    if unknown:
        raise ReplayError(f"Unknown steps: {', '.join(sorted(unknown))}")
    changed = True
    while changed:
        before = len(chosen)
        chosen |= {name for name, after in DEPENDENCIES.items() if chosen & set(after)}
        changed = len(chosen) != before
    return [name for name in DEPENDENCIES if name in chosen]


def apply_forcing(
    pipeline: steps.Pipeline, forcing: Forcing, *, rerun: bool = False
) -> list[dict[str, object]]:
    """Remove the forced steps' receipts and outputs, dependents first.

    Unless the caller runs the recording again at once (``rerun``: ``run`` itself), a forcing that
    would remove a receipt the published delivery's evidence names is refused: the delivery's
    completion gate reads those files, and nothing would replace them (10-25: a fix-up forcing of
    paused recordings left 10/02-10/08 deliveries without their evidence).
    """
    names = list(DEPENDENCIES) if forcing.every else downstream(forcing.names)
    if not rerun:
        published = published_evidence(pipeline)
        named = sorted(str(path) for path in published if path.parent == pipeline.steps
                       and path.stem in names)  # fmt: skip
        if named:
            raise ReplayError(
                "Forcing would remove receipts the published delivery's gate reads "
                f"({', '.join(named)}); force them through `run --force` instead"
            )
    removed: list[dict[str, object]] = []
    for name in reversed(names):  # dependents first, so no receipt outlives its inputs
        removed.extend(steps.reset(pipeline, name, graph_of(name)))
    return removed


def published_evidence(pipeline: steps.Pipeline) -> set[Path]:
    """Every path the recording's published delivery evidence names (none before publish)."""
    if not (pipeline.steps / "publish.json").is_file():
        return set()
    storage = viewer.delivery(pipeline).parents[3]
    return retirement.named(retirement.evidence_files(pipeline, storage))


def stage_of(layout: Layout, python: str, name: str) -> Stage:
    receipts = [layout.steps / f"{parent}.json" for parent in DEPENDENCIES[name]]
    owner = "recording_caption_steps" if name in captions.STEPS else "recording_steps"
    settings = (layout.directory / "captions.json",) if name in captions.STEPS else ()
    return Stage(
        name=name,
        command=(python, "-B", "-m", f"replay.{owner}", name, str(layout.pipeline)),
        inputs=(layout.pipeline, *settings, *receipts),
        outputs=(layout.steps / f"{name}.json",),
        timeout_seconds=TIMEOUTS[name],
    )


def run(
    replay_id: str,
    defaults_path: Path,
    *,
    checkout: Path,
    forcing: Forcing = NO_FORCING,
    children: Children | None = None,
) -> dict[str, Result]:
    """Run every unfinished step of the recording; return each step's result."""
    defaults = load_defaults(defaults_path)
    layout = layout_of(replay_id, defaults, checkout)
    pipeline = steps.load(layout.pipeline)
    owned = children or Children()
    # Every stage inherits this rank: the GPU leases serve the newest broadcast first, so a rerun
    # keeps its place (a start-time rank sent a rerun of the newest recording to the back).
    os.environ.setdefault(gpu_lease.STARTED_VARIABLE, str(lease_rank(pipeline.path_of("editRoot"))))
    with execution_lock(layout.directory, "pipeline") as acquired:
        if not acquired:
            raise ReplayError("Another run owns this recording's pipeline")
        names = (*ORDER, "retire-intermediates")
        scope = run_scope(forcing, names)
        published = (pipeline.steps / "publish.json").is_file()
        redo = forcing.every or bool(forcing.names)
        skipped = not_applicable(pipeline, names, scope, published=published, redo=redo)
        for name, why in skipped.items():
            print(f"not applicable: {name} ({why})", flush=True)
        active = tuple(name for name in names if name in scope and name not in skipped)
        if published:
            forced_names = set(names) if forcing.every else set(downstream(forcing.names))
            pending = {name for name in active if name in forced_names
                       or not (pipeline.steps / f"{name}.json").is_file()}  # fmt: skip
            refuse_out_of_scope_producers(pipeline, pending, scope)
        mark_legacy(pipeline, names, skipped)
        forced = apply_forcing(pipeline, forcing, rerun=True)
        for item in [*forced, *force_retired_producers(pipeline, active)]:
            print(f"forced: removed {item['path']} ({item['bytes']} bytes)", flush=True)
        python = str(defaults["python"])
        stages = tuple(stage_of(layout, python, name) for name in names)

        def execute(stage: Stage, _state: Path) -> Result:
            if stage.name in skipped:
                return "skipped"
            if steps.finished(pipeline, stage.name, graph_of(stage.name)):
                return "reused"
            logs = layout.steps / "logs"
            reason = priority.drained(gpu_lease.root().parent, replay_id)
            if reason is not None:
                # Drained between steps: nothing running is stopped, nothing new starts.
                (logs / f"drain-{time.time_ns()}.json").write_text(
                    json.dumps({"step": stage.name, "reason": reason}) + "\n", encoding="utf-8"
                )
                raise StageError(f"Drained ({reason}) before {stage.name}")
            started_at = time.time()
            outputs = graph_of(stage.name).outputs(pipeline, stage.name)
            # Its own work and its cleanup work directories: where finished receipts appear.
            work_roots = (
                layout.directory,
                *layout.directory.parent.parent.glob(f"clean-{layout.directory.name}-*"),
            )
            with ExitStack() as held:
                if stage.name in SERIAL_STEPS:
                    held.enter_context(
                        gpu_lease.lease(serial_leases(stage.name), replay_id, step=stage.name)
                    )
                if stage.name in GPU_STEPS:
                    # The pipeline's own GPU slot is held already; then one machine-wide lease.
                    held.enter_context(
                        gpu_lease.lease(
                            gpu_lease.root(), replay_id, step=stage.name, job=stage.name
                        )
                    )
                command = admitted_command(stage, replay_id, held)
                snapshot = snapshotted(stage.name, held)
                if snapshot:
                    command = (ENV, *snapshot, *command)
                else:  # a step on src/: an edit under it is noticed (it is never stopped)
                    held.enter_context(
                        closure_guard.watching(
                            (layout.directory,), started_at,
                            on_change=lambda edited: closure_guard.note_edit(
                                replay_id, stage.name, logs / f"{stage.name}.log", edited
                            ),
                        )
                    )  # fmt: skip
                try:
                    step_retry.run_with_retry(
                        lambda log: owned.execute(
                            command,
                            log,
                            stage.timeout_seconds,
                            progress=lambda: step_progress(outputs, logs, stage.name),
                            idle=IDLE_SECONDS[stage.name],
                        ),
                        lambda: logs / f"{stage.name}-{time.time_ns()}.run.log",
                        logs=logs,
                        step=stage.name,
                        progress=lambda: step_retry.receipts(work_roots),
                        recording=replay_id,
                        volumes=volume_events.snapshot,
                        receipt_roots=(*work_roots, layout.steps),
                    )
                except (ReplayError, StageError) as error:
                    # A step drained mid-way (cleanup starts no new chunk) fails; the drain owns it.
                    reason = priority.drained(gpu_lease.root().parent, replay_id)
                    if reason is None:
                        raise
                    raise StageError(f"Drained ({reason}) during {stage.name}") from error
            return "completed"

        layout.steps.joinpath("logs").mkdir(mode=0o700, exist_ok=True)
        started = time.time()
        with memory_sampler.sampling(layout.steps / "logs" / "memory.jsonl"):
            results = run_selected(
                stages,
                graph={name: SCHEDULE[name] for name in names},
                state=layout.steps,
                bindings={},
                max_parallel=int(str(defaults["maxParallel"])),
                resources=StageSlots(
                    {GPU: int(str(defaults["gpuSlots"]))}, dict.fromkeys(GPU_STEPS, GPU)
                ),
                run=execute,
            )
        print(f"pipeline wall seconds: {time.time() - started:.1f}", flush=True)
        # Every step passed: the recording's heavy intermediates go (steps/logs/retirement.jsonl).
        retired = retirement.retire(pipeline, retirement_settings(defaults_path))
        print(f"retirement: {json.dumps(retired, ensure_ascii=False)}", flush=True)
        return results


def step_progress(outputs: Sequence[Path], logs: Path, step: str) -> int:
    """Files and bytes under the step's outputs plus the bytes of its logs: grows while it works."""
    files = [*outputs, *logs.glob(f"{step}*.log")]
    files += [Path(folder) / name for root in outputs for folder, _, names in os.walk(root)
              for name in names]  # fmt: skip
    total = 0
    for path in files:
        try:
            if path.is_file():
                total += 1 + path.stat().st_size
        except FileNotFoundError:  # removed while the step works: it simply no longer counts
            continue
    return total


def snapshotted(step: str, held: ExitStack) -> tuple[str, ...]:
    """``env`` arguments running the step from a code snapshot (none when not enabled)."""
    if not code_snapshot.enabled() or step in code_snapshot.UNSNAPSHOTTED:
        return ()
    sha, snapshot = code_snapshot.materialise(Path(__file__).resolve().parent)
    held.enter_context(code_snapshot.in_use(snapshot))
    return (
        f"PYTHONPATH={snapshot}",
        f"{code_snapshot.VARIABLE}={sha}",
        f"{repository.VARIABLE}={repository.root()}",
    )


def admitted_command(stage: Stage, replay_id: str, held: ExitStack) -> tuple[str, ...]:
    """Hold the step's machine-wide memory reservation; its command, torch-capped when measured.

    Separation, ASR and the other ML producers are hash-pinned, so they are gated here, at step
    start, and capped only through the environment their processes inherit.
    """
    if stage.name not in memory_admission.DEFAULT_PEAKS_GIB:
        return stage.command
    current = held.enter_context(memory_admission.reserve(stage.name, replay_id))
    text = " ".join(stage.command)
    held.enter_context(
        memory_admission.measuring(
            memory_admission.root(), stage.name, lambda arguments: arguments == text
        )
    )
    caps = memory_admission.cap_environment(stage.name, current, memory_admission.physical())
    if not caps:
        return stage.command
    return ("/usr/bin/env", *(f"{name}={value}" for name, value in caps.items()), *stage.command)


def serial_leases(step: str) -> Path:
    """The machine-wide one-lease directory of a step that runs one recording at a time."""
    directory = gpu_lease.root().parent / SERIAL_STEPS[step]
    settings = directory / "settings.json"
    if not settings.exists():
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        settings.write_text(SERIAL_SETTINGS, encoding="utf-8")
    return directory


LAUNCH_FILES = (".launch", ".log", ".status")


def launch_name(layout: Layout) -> str:
    for number in range(1, MAX_LAUNCHES + 1):
        name = f"pipeline-v{number}"
        # A launch interrupted before its record was kept can still hold the name's log.
        if not any((layout.directory / f"{name}{suffix}").exists() for suffix in LAUNCH_FILES):
            return name
    raise ReplayError("Too many pipeline launches; inspect the earlier ones")


def start(
    replay_id: str, defaults_path: Path, *, checkout: Path, forcing: Forcing = NO_FORCING
) -> str:
    """Prepare the pipeline and launch ``run`` detached; return the launch name."""
    layout = prepare(replay_id, defaults_path, checkout=checkout)
    python = str(load_defaults(defaults_path)["python"])
    return dispatch(layout, replay_id, defaults_path, python=python, checkout=checkout,
                    forcing=forcing)  # fmt: skip


def dispatch(
    layout: Layout,
    replay_id: str,
    defaults_path: Path,
    *,
    python: str,
    checkout: Path,
    forcing: Forcing = NO_FORCING,
) -> str:
    """Launch ``run`` detached in the recording's own tmux server; return the launch name."""
    name = launch_name(layout)
    command = [
        str(checkout / LAUNCHER), str(layout.directory), name, python, "-B", "-m",
        "replay.recording_pipeline", "run", replay_id, "--defaults", str(defaults_path.resolve()),
        *forcing.arguments(),
    ]  # fmt: skip
    subprocess.run(command, check=True, cwd=checkout)
    return name


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("prepare", "run", "start"))
    parser.add_argument("replay_id")
    parser.add_argument("--defaults", type=Path, required=True)
    parser.add_argument("--force", action="append", default=[], choices=tuple(DEPENDENCIES))
    parser.add_argument("--force-all", action="store_true")
    args = parser.parse_args(argv)
    checkout = Path.cwd().resolve()
    forcing = Forcing(tuple(args.force), args.force_all)
    children = Children()

    def stop(_signum: int, _frame: FrameType | None) -> None:
        children.stop_all()
        raise StageError("Pipeline stopped; finished work is reused on the next run")

    previous = signal.signal(signal.SIGTERM, stop)
    try:
        if args.action == "prepare":
            print(prepare(args.replay_id, args.defaults, checkout=checkout).directory)
        elif args.action == "run":
            prepare(args.replay_id, args.defaults, checkout=checkout)
            run(
                args.replay_id, args.defaults, checkout=checkout, forcing=forcing, children=children
            )
        else:
            print(start(args.replay_id, args.defaults, checkout=checkout, forcing=forcing))
    except (ReplayError, StageError, OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"Pipeline stopped ({type(error).__name__}): {error}", file=sys.stderr)
        drained = priority.drained(gpu_lease.root().parent, args.replay_id) is not None
        return DRAINED_EXIT if drained and "Drained (" in str(error) else 1
    finally:
        signal.signal(signal.SIGTERM, previous)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
