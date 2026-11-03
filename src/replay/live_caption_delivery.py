"""A recording's delivery without burned captions, beside its burned one (user, 2026-11-03).

The viewers draw the captions live from the caption plan. A recording finished on the burned
pipeline gets this second delivery without being run again: a sibling pipeline (``live-v1``
beside the recording's ``pipeline.json``, as ``replay.vertical_excerpt`` makes its own) composes
and finishes the same cuts, pages and voice uncaptioned. Its upstream receipts are copied and
its inputs hard-linked, and it has its own work, steps and media root, so the recording's steps
and the burned revision's evidence are never touched and that revision still passes its gate.

The movie is published as its own library folder, ``<recording>-live`` beside the recording's
(``<stamp>_JST_<recording>-live-review.mp4``), so the Viewer and the web catalog list it beside
the burned delivery instead of superseding it. Beside it go the SRT (the same displayed captions
as the burned delivery's, which is checked), the caption plan (``burned: false``), the face
track, an evidence index of the sibling's receipts, and the completion gate's verdict
(``pipeline_completion.stage_failures``: all five stages must pass). The burned delivery's
summaries and excitement levels are copied, as their transcript and clock are the same. No clip
points, YouTube encode or master are made for it.

    python -m replay.live_caption_delivery check PIPELINE.json
    python -m replay.live_caption_delivery run PIPELINE.json
"""

import argparse
import json
import os
import shutil
import subprocess
from collections.abc import Sequence
from contextlib import ExitStack
from pathlib import Path

from replay import caption_faces, caption_plan, gpu_lease
from replay import pipeline_completion as completion
from replay import recording_caption_steps as caption_steps
from replay import recording_pipeline as pipelines
from replay.core import ReplayError
from replay.edit_completion import load_config
from replay.recordings import record

SIBLING = "live-v1"
MEDIA_ROOT = "recordings-live"  # beside the recordings' own media root
FOLDER_SUFFIX = "-live"
STEPS = ("compose", "finish", "voice-remux")
PUBLISHED = "live-publish"  # the sibling's record of its delivery (``caption_plan`` reads it)
GATE = "live-gate.json"
COPIED = ("voice-repair", "caption-parts", *completion.STEP_NAMES)
PRODUCED = frozenset({*STEPS, "caption-render"})
LINKED = (
    "cut-plan-v1/supported-cuts.json",
    "paging-v1/prepared/tempo/candidate.wav",
    "paging-v1/prepared/tempo/receipt.json",
    "paging-v1/prepared/tempo-selection.json",
    "video-clock.json",
    "caption-parts-v1/parts.json",
    "placement-v1.json",
    "source.sha256",
)
SIDECARS = (".srt.excitement.json",)
SUMMARIES = ".srt.summaries"


def read(path: Path) -> dict[str, object]:
    return record(json.loads(path.read_text(encoding="utf-8")))


def write_same(path: Path, text: str) -> None:
    """Write ``path`` once; an existing one must hold the same text."""
    if path.exists():
        if path.read_text(encoding="utf-8") != text:
            raise ReplayError(f"{path} exists with other content; inspect it")
        return
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    caption_plan.publish_once(path, text.encode("utf-8"))


def link(source: Path, target: Path) -> None:
    """A hard link (same file system, never a symbolic link) of ``source`` at ``target``."""
    if not source.is_file():
        raise ReplayError(f"{source} is missing; the sibling needs it")
    if target.exists():
        if not target.samefile(source):
            raise ReplayError(f"{target} exists and is not {source}")
        return
    target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    os.link(source, target)


def prepare(pipeline_path: Path) -> Path:
    """The sibling pipeline: the recording's settings with its own work, steps and media root;
    the upstream receipts copied and the inputs hard-linked (idempotent)."""
    data = read(pipeline_path)
    root = pipeline_path.parent / SIBLING
    work, steps = Path(str(data["work"])), Path(str(data["steps"]))
    captions = read(pipeline_path.parent / "captions.json")
    media = str(Path(str(captions["mediaRoot"])).parent / MEDIA_ROOT)
    settings = {
        "pipeline.json": {**data, "work": str(root / "work"), "steps": str(root / "steps")},
        "captions.json": {**captions, "mediaRoot": media},
    }
    for name, value in settings.items():
        write_same(root / name, json.dumps(value, indent=1, ensure_ascii=False) + "\n")
    (root / "steps" / "logs").mkdir(mode=0o700, parents=True, exist_ok=True)
    for step in sorted(set(COPIED) - PRODUCED):
        write_same(root / "steps" / f"{step}.json", (steps / f"{step}.json").read_text("utf-8"))
    for name in LINKED:
        link(work / name, root / "work" / name)
    return root / "pipeline.json"


def leases(name: str, recording: str, held: ExitStack) -> None:
    """The machine-wide leases the recording pipeline holds for ``name``."""
    if name in pipelines.SERIAL_STEPS:
        held.enter_context(
            gpu_lease.lease(pipelines.serial_leases(name), recording, step=name)
        )  # fmt: skip
    if name in pipelines.GPU_STEPS:
        held.enter_context(gpu_lease.lease(gpu_lease.root(), recording, step=name, job=name))


def produce(sibling: Path) -> None:
    """Run the sibling's compose, finish and voice-remux steps that have not passed."""
    if "caption-render" in caption_steps.STEPS:
        raise ReplayError("This compose still burns captions; apply the pipeline switch first")
    data = read(sibling)
    for name in STEPS:
        if (Path(str(data["steps"])) / f"{name}.json").is_file():
            continue
        with ExitStack() as held:
            leases(name, str(data["recording"]), held)
            subprocess.run(
                [str(data["python"]), "-B", "-m", "replay.recording_caption_steps", name,
                 str(sibling)],
                check=True,
            )  # fmt: skip


def original_delivery(steps: Path) -> Path:
    """The recording's current burned delivery: its master, else its publication."""
    for step, key in (("youtube-master", "master"), ("publish", "delivery")):
        receipt = steps / f"{step}.json"
        if receipt.is_file():
            output = record(read(receipt)["outputs"])[key]
            path = Path(str(record(output)["path"] if isinstance(output, dict) else output))
            if path.is_file():
                return path
    raise ReplayError("The recording has no burned delivery on disk")


def target(sibling: Path) -> Path:
    """The live delivery's path: its own folder beside the recording's in the library."""
    data = read(sibling)
    edit_root = Path(str(data["editRoot"]))
    config = load_config(edit_root)
    if config.storage_root is None:
        raise ReplayError("Bind the recording's storage root first")
    folder = f"{edit_root.name}{FOLDER_SUFFIX}"
    stamp = config.source.stem.split("_JST_")[0]
    return config.storage_root.parent / folder / "delivery" / f"{stamp}_JST_{folder}-review.mp4"


def publish(sibling: Path, original: Path) -> tuple[Path, Path]:
    """Link the sibling's movie into its folder with the burned delivery's SRT (checked equal),
    an evidence index of the sibling's receipts, and the caption plan and face track."""
    data = read(sibling)
    steps, edit_root = Path(str(data["steps"])), Path(str(data["editRoot"]))
    movie = Path(str(record(record(read(steps / "voice-remux.json")["outputs"])["movie"])["path"]))
    delivery = target(sibling)
    link(movie, delivery)
    candidate = Path(str(data["work"])) / "placement-v1.json"
    text = completion.srt(completion.transcript(candidate))
    if original.with_suffix(".srt").read_text(encoding="utf-8") != text:
        raise ReplayError("The burned delivery shows other captions; its sidecars do not apply")
    write_same(delivery.with_suffix(".srt"), text)
    index = delivery.parents[1] / "metadata" / completion.INDEX_NAME
    value = {
        "version": 1, "backend": completion.BACKEND, "recording": edit_root.name,
        "configurationSHA256": completion.sha256(edit_root / "edit-config.json"),
        "source": str(load_config(edit_root).source), "movie": completion.pin(delivery),
        "srt": completion.pin(delivery.with_suffix(".srt")),
        "candidate": completion.pin(candidate),
        "steps": {name: completion.pin(steps / f"{name}.json") for name in completion.STEP_NAMES
                  if name != "caption-render"},
        "qualityApproved": False,
    }  # fmt: skip
    write_same(index, json.dumps(value, indent=1) + "\n")
    write_same(steps / f"{PUBLISHED}.json",
               json.dumps({"outputs": {"delivery": str(delivery)}}, indent=1) + "\n")  # fmt: skip
    work = Path(str(data["work"]))
    plan, icon = caption_plan.build(work, delivery)
    caption_plan.write(plan, delivery.parent, icon)
    caption_faces.write(caption_faces.build(work, delivery), delivery)
    return delivery, index


def carry(original: Path, delivery: Path) -> None:
    """The burned delivery's summaries and excitement levels (same transcript and clock)."""
    for suffix in SIDECARS:
        if original.with_suffix(suffix).is_file() and not delivery.with_suffix(suffix).exists():
            shutil.copy2(original.with_suffix(suffix), delivery.with_suffix(suffix))
    summaries = original.with_suffix(SUMMARIES)
    if summaries.is_dir() and not delivery.with_suffix(SUMMARIES).exists():
        shutil.copytree(summaries, delivery.with_suffix(SUMMARIES))


def gate(index: Path, *, ffmpeg: str, ffprobe: str) -> dict[str, str]:
    """The completion gate's five stages on the live delivery, recorded beside its index."""
    failures = completion.stage_failures(read(index), ffmpeg=ffmpeg, ffprobe=ffprobe)
    verdict = {"index": completion.pin(index), "failures": failures, "passed": not failures}
    (index.parent / GATE).write_text(json.dumps(verdict, indent=1) + "\n", encoding="utf-8")
    return failures


def check(pipeline_path: Path) -> dict[str, object]:
    """What ``run`` would do, without writing anything."""
    data = read(pipeline_path)
    steps, work = Path(str(data["steps"])), Path(str(data["work"]))
    sibling = pipeline_path.parent / SIBLING
    missing = [name for name in sorted(set(COPIED) - PRODUCED)
               if not (steps / f"{name}.json").is_file()]  # fmt: skip
    missing += [name for name in LINKED if not (work / name).is_file()]
    done = [name for name in STEPS if (sibling / "steps" / f"{name}.json").is_file()]
    return {
        "sibling": str(sibling), "missingInputs": missing,
        "toRun": [name for name in STEPS if name not in done],
        "burnedDelivery": str(original_delivery(steps)),
        "liveDelivery": str(target(pipeline_path)),
    }  # fmt: skip


def run(pipeline_path: Path) -> dict[str, object]:
    """Prepare and produce the sibling, publish the live delivery and gate it."""
    original = original_delivery(Path(str(read(pipeline_path)["steps"])))
    sibling = prepare(pipeline_path)
    produce(sibling)
    delivery, index = publish(sibling, original)
    carry(original, delivery)
    data = read(sibling)
    failures = gate(index, ffmpeg=str(data["ffmpeg"]), ffprobe=str(data["ffprobe"]))
    if failures:
        raise ReplayError(f"The live delivery fails its completion gate: {failures}")
    return {"delivery": str(delivery), "index": str(index), "gate": "passed"}


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("check", "run"))
    parser.add_argument("pipeline", type=Path)
    args = parser.parse_args(argv)
    act = check if args.command == "check" else run
    print(json.dumps(act(args.pipeline.resolve()), indent=1, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
