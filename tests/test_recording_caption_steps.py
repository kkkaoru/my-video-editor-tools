"""Caption steps rebind the reference plans, call the producers in order and reset on force."""

import json
import time
import wave
from collections.abc import Sequence
from pathlib import Path

import numpy as np
import pytest

from replay import (
    caption_faces,
    caption_plan,
    pipeline_completion,
    r2_fetch,
    voice_remux,
    youtube_master,
)
from replay import recording_caption_steps as captions
from replay import recording_steps as steps
from replay import recording_viewer_steps as viewer
from replay.core import ReplayError

ROLE_NAMES = ("directory", "stereo", "mono", "monoReceipt", "clock", "clockReceipt", "video",
              "labels", "asr", "source")  # fmt: skip


def configure(tmp_path: Path) -> Path:
    """A pipeline whose front half finished, with caption settings and reference files."""
    source = tmp_path / "source.mp4"
    source.write_bytes(b"video")
    work = tmp_path / "replay-123"
    for name in ("separation-v1/pipeline", "asr-v1/prepared/state", "asr-v1/prepared/asr", "state"):
        (work / name).mkdir(parents=True, exist_ok=True)
    (work / "audio-clock.json").write_text("{}", encoding="utf-8")
    (work / "source.sha256").write_text(f"{'b' * 64}  {source}\n", encoding="utf-8")
    (work / "separation-v1/pipeline/vocals.wav").write_bytes(b"v")
    data = {
        "version": 1, "recording": "123", "editRoot": str(tmp_path / "edit"),
        "source": str(source), "work": str(work), "steps": str(tmp_path / "steps"),
        "python": "/py", "providerPython": "/provider", "native": "/native",
        "ffmpeg": "/ffmpeg", "ffprobe": "/ffprobe",
        "references": {name: "/r" for name in steps.REFERENCES},
        "yolo": {"weights": "/w.pt", "sha256": "a" * 64, "configDirectory": "/c", "step": "2"},
    }  # fmt: skip
    (tmp_path / "steps").mkdir()
    for name in ("clocks", "asr", "yolo-track"):
        (tmp_path / "steps" / f"{name}.json").write_text("{}", encoding="utf-8")
    (tmp_path / "steps/cleanup.json").write_text(
        json.dumps({"outputs": {"cleanSource": str(tmp_path / "clean.mov")}}), encoding="utf-8"
    )
    references = reference_files(tmp_path)
    settings = {
        "version": 1,
        "references": references,
        "relocations": str(relocations(tmp_path)),
        "mediaRoot": str(tmp_path / "media"),
        "placement": {"regionTop": 120, "regionBottom": 1800, "clearance": 60, "maximumDrift": 150},
        "motionFinish": ["--font-size", "82"],
        "compressor": {"preset": "/p.compressorsetting", "inFlight": 2, "submitWorkers": 1},
        "compose": {"server": "/server", "serverSha256": "c" * 64, "workers": 3},
    }
    (tmp_path / "captions.json").write_text(json.dumps(settings), encoding="utf-8")
    path = tmp_path / "pipeline.json"
    path.write_text(json.dumps(data), encoding="utf-8")
    return path


def reference_files(tmp_path: Path) -> dict[str, str]:
    old = tmp_path / "old"
    old.mkdir()
    roles = {name: str(old / name) for name in ROLE_NAMES}
    (tmp_path / "roles.json").write_text(json.dumps(roles), encoding="utf-8")
    stage = {
        "name": "only",
        "command": ["true", *(f"{old}/{name}" for name in ROLE_NAMES)],
        "inputs": [f"{old}/directory/in.json"],
        "outputs": [f"{old}/directory/out.json"],
        "timeout_seconds": 10,
    }
    workflow = {"version": 1, "stages": [stage]}
    (tmp_path / "workflow.json").write_text(json.dumps(workflow), encoding="utf-8")
    closure = tmp_path / "closure.json"
    code = tmp_path / "code.py"
    code.write_text("x = 1\n", encoding="utf-8")
    closure.write_text(json.dumps({"inputs": {str(code): "0" * 64}}), encoding="utf-8")
    measurement = tmp_path / "measurement.json"
    measurement.write_text('{"width": 972}', encoding="utf-8")
    font = tmp_path / "font.json"
    font.write_text("{}", encoding="utf-8")
    paging = {
        "version": 1,
        "inputs": {
            "source": {"path": "/old.mp4", "sha256": "0"},
            "codeClosure": {"path": str(closure), "sha256": "0"},
            "measurementReference": {"path": str(measurement), "sha256": "0"},
            "fontReceipt": {"path": str(font), "sha256": steps.sha256(font)},
        },
        "futurePaths": {},
        "evidence": {"0": {"path": str(font), "sha256": steps.sha256(font)}},
    }
    (tmp_path / "paging.json").write_text(json.dumps(paging), encoding="utf-8")
    calibration = tmp_path / "envelope.json"
    calibration.write_text("{}", encoding="utf-8")
    return {
        "cutPlanWorkflow": str(tmp_path / "workflow.json"),
        "cutPlanRoles": str(tmp_path / "roles.json"),
        "pagingConfig": str(tmp_path / "paging.json"),
        "calibration": str(calibration),
        "motionTemplate": str(tmp_path / "template.moti"),
    }


def relocations(tmp_path: Path) -> Path:
    table = tmp_path / "RELOCATED.tsv"
    table.write_text(
        "name\told_path\tnew_path\tfiles\tbytes\tmoved_at\n"
        f"moved\t{tmp_path / 'gone'}\t{tmp_path / 'ssd'}\t1\t1\tx\n",
        encoding="utf-8",
    )
    return table


class Producers:
    """Records commands and writes what the real producers would."""

    def __init__(self, tmp_path: Path) -> None:
        self.tmp = tmp_path
        self.commands: list[list[str]] = []
        self.opened: list[str] = []

    def __call__(self, command: Sequence[str], log: Path) -> None:
        if command[0] == "open":
            self.opened = list(command)
            return
        self.commands.append(list(command))
        work = self.tmp / "replay-123"
        name = command[command.index("-m") + 1]
        arguments = list(command[command.index("-m") + 2 :])
        if name == "replay.workflow" and "cut-plan-v1" in arguments[0]:
            for output in ("supported-cuts.json", "native-config.json"):
                (work / "cut-plan-v1" / output).write_text("{}", encoding="utf-8")
        elif name == "replay.caption_native_continuation":
            prepared = Path(arguments[1])
            for directory in ("paging", "tempo"):
                (prepared / directory).mkdir(parents=True)
            paging = ["true", "-m", "replay.caption_native_paging", "a"]
            verify = ["true", "-m", "replay.caption_reflow_verify", "r"]
            stage = {"name": "s", "command": paging, "verify_command": verify,
                     "inputs": [str(prepared / "i")],
                     "outputs": [str(prepared / "o")], "timeout_seconds": 1}  # fmt: skip
            (prepared / "workflow.json").write_text(json.dumps({"version": 1, "stages": [stage]}))
        elif name == "replay.workflow":
            prepared = work / "paging-v1/prepared"
            (prepared / "paging/reflow.json").write_text("{}", encoding="utf-8")
            (prepared / "tempo/candidate.wav").write_bytes(b"t")
        elif name == "replay.caption_fixed_y_job":
            placed = {"plannedRuns": {"placedCues": 9, "failedRuns": [{}]}}
            Path(arguments[4]).write_text(json.dumps(placed), encoding="utf-8")
        elif name == "replay.voice_repair":
            repaired_root = Path(arguments[1])
            repaired_root.mkdir(parents=True)
            (repaired_root / "vocals.wav").write_bytes(b"r")
            voice = {"path": str(repaired_root / "vocals.wav"), "sha256": "r"}
            receipt = {"voice": voice, "repaired": True}
            (repaired_root / "receipt.json").write_text(json.dumps(receipt), encoding="utf-8")
        elif name == "replay.voice_remux":
            restored = Path(arguments[2])
            restored.mkdir(parents=True)
            (restored / "vocals.wav").write_bytes(b"restored")
        elif name == "replay.clocked_tempo_audio" and "voice-remux-v1" in arguments[1]:
            tempo = Path(arguments[1])
            tempo.mkdir(parents=True)
            (tempo / "candidate.wav").write_bytes(b"tempo")
            (tempo / "receipt.json").write_text("{}", encoding="utf-8")
        elif name == "replay.voice_tempo":
            tempo = Path(arguments[2])
            tempo.mkdir(parents=True)
            (tempo / "candidate.wav").write_bytes(b"resampled")
            (tempo / "receipt.json").write_text("{}", encoding="utf-8")
        elif name == "replay.voice_judge":
            Path(arguments[-1]).write_text('{"passed": true}', encoding="utf-8")
        elif name == "replay.voice_deesser":
            deessed_root = Path(arguments[1])
            deessed_root.mkdir(parents=True)
            (deessed_root / "vocals.wav").write_bytes(b"d")
            voice = {"path": str(deessed_root / "vocals.wav"), "sha256": "d"}
            (deessed_root / "receipt.json").write_text(json.dumps({"voice": voice}), "utf-8")
        elif name == "replay.speech_onsets":
            Path(arguments[2]).write_text('{"tokens": []}', encoding="utf-8")
        elif name == "replay.caption_parts":
            Path(arguments[1]).mkdir()
            for output in ("parts.json", "jobs.json"):
                (Path(arguments[1]) / output).write_text("[]", encoding="utf-8")
            (Path(arguments[1]) / "moti").mkdir()
            title = {"text": "あ", "startFrame": 0, "endFrame": 40, "graphemeFrames": [0]}
            (Path(arguments[1]) / "moti/w000-part-0000.reveal.json").write_text(
                json.dumps({"offsetFrame": 0, "titles": [title]}), "utf-8"
            )
        elif name == "replay.caption_compose" and arguments[0] == "plan":
            Path(arguments[5]).mkdir(parents=True)
            groups = '[{"frames": 5}, {"frames": 2}]'
            (Path(arguments[5]) / "groups.json").write_text(groups, encoding="utf-8")
            video = {"encoding": {"averageBitRate": 8_200_000}}
            (Path(arguments[5]) / "g000.json").write_text(json.dumps({"recipe": {"video": video}}))
        elif name == "replay.caption_compose" and arguments[0] == "finish":
            Path(arguments[3]).write_bytes(b"movie")


def modules(producers: Producers) -> list[str]:
    return [command[command.index("-m") + 1] for command in producers.commands]


def run_through(path: Path, producers: Producers, *names: str) -> None:
    for name in names:
        steps.execute(name, path, run=producers, graph=captions.BACK)


def test_the_cut_plan_rebinds_every_reference_role_and_runs_the_workflow(tmp_path: Path) -> None:
    path = configure(tmp_path)
    producers = Producers(tmp_path)
    receipt = steps.execute("cut-plan", path, run=producers, graph=captions.BACK)
    plan = tmp_path / "replay-123/cut-plan-v1"
    written = json.loads((plan / "workflow-v1.json").read_text(encoding="utf-8"))
    assert written["stages"][0]["command"][1] == str(plan)
    assert written["stages"][0]["command"][-1] == str(tmp_path / "source.mp4")
    assert written["stages"][0]["outputs"] == [str(plan / "out.json")]
    native = json.loads((plan / "native-template.json").read_text(encoding="utf-8"))
    assert native["expectedInputs"]["source"]["sha256"] == "b" * 64
    assert modules(producers) == ["replay.workflow"]
    data = json.loads(receipt.read_text(encoding="utf-8"))
    assert sorted(data["inputs"]) == sorted(
        [str(path), str(tmp_path / "captions.json"),
         str(tmp_path / "steps/clocks.json"), str(tmp_path / "steps/asr.json")]
    )  # fmt: skip


def test_a_reference_path_the_workflow_lacks_is_refused() -> None:
    roles = {name: f"/old/{name}" for name in ROLE_NAMES}
    with pytest.raises(ReplayError, match="does not use its clockReceipt path"):
        captions.rebind_workflow("/old/directory", roles, {n: Path("/new") for n in ROLE_NAMES})


def test_a_reference_path_left_after_substitution_is_refused() -> None:
    roles = {name: f"/old/{name}" for name in ROLE_NAMES}
    text = " ".join(roles.values())
    new = {name: Path(f"/new/{name}") for name in ROLE_NAMES}
    new["asr"] = Path("/old/source")
    with pytest.raises(ReplayError, match="remain in the workflow: source"):
        captions.rebind_workflow(text, roles, new)


def test_the_reference_recording_rebinds_onto_itself() -> None:
    roles = {name: f"/old/{name}" for name in ROLE_NAMES}
    text = " ".join(roles.values())
    new = {name: Path(f"/old/{name}") for name in ROLE_NAMES}
    assert captions.rebind_workflow(text, roles, new) == text


def test_incomplete_reference_roles_are_refused() -> None:
    with pytest.raises(ReplayError, match="roles are incomplete"):
        captions.rebind_workflow("", {"directory": "/d"}, {})


def test_paging_rebinds_the_configuration_and_refreshes_the_closure(tmp_path: Path) -> None:
    path = configure(tmp_path)
    producers = Producers(tmp_path)
    run_through(path, producers, "cut-plan", "paging")
    root = tmp_path / "replay-123/paging-v1"
    config = json.loads((root / "config.json").read_text(encoding="utf-8"))
    assert config["inputs"]["source"]["path"] == str(tmp_path / "source.mp4")
    assert config["inputs"]["codeClosure"]["path"] == str(root / "CODE-CLOSURE.json")
    assert config["futurePaths"]["cuts"].endswith("cut-plan-v1/supported-cuts.json")
    closure = json.loads((root / "CODE-CLOSURE.json").read_text(encoding="utf-8"))
    lines = Path(captions.__file__).with_name("caption_native_paging_lines.py")
    stable = Path(captions.__file__).with_name("caption_stable_lines.py")
    assert closure == {
        "inputs": {
            str(tmp_path / "code.py"): steps.sha256(tmp_path / "code.py"),
            str(lines): steps.sha256(lines),
            str(stable): steps.sha256(stable),
        }
    }
    plan = json.loads((root / "prepared/workflow.json").read_text(encoding="utf-8"))
    assert plan["stages"][0]["command"] == [
        "true", "-m", "replay.caption_native_paging_lines", "--maximum-lines", "2",
        "--module", "replay.caption_native_paging", "--", "a",
    ]  # fmt: skip
    assert plan["stages"][0]["verify_command"][3:7] == [
        "--maximum-lines", "2", "--module", "replay.caption_reflow_verify",
    ]  # fmt: skip
    captions.limit_paging_lines(root / "prepared/workflow.json")  # an applied limit is kept
    again = json.loads((root / "prepared/workflow.json").read_text(encoding="utf-8"))
    assert again["stages"][0]["command"] == [
        "true", "-m", "replay.caption_native_paging_lines", "--maximum-lines", "2",
        "--module", "replay.caption_native_paging", "--", "a",
    ]  # fmt: skip
    assert modules(producers)[1:] == ["replay.caption_native_continuation", "replay.workflow"]
    assert producers.commands[-1][1].startswith("PYTHONPATH=")


def test_a_relocated_reference_is_copied_back_after_its_digest_matches(tmp_path: Path) -> None:
    table = relocations(tmp_path)
    moved = tmp_path / "ssd/sub/a.json"
    moved.parent.mkdir(parents=True)
    moved.write_text("{}", encoding="utf-8")
    target = tmp_path / "gone/sub/a.json"
    captions.restore(target, steps.sha256(moved), table)
    assert target.read_text(encoding="utf-8") == "{}"


def test_a_relocated_reference_with_another_digest_is_refused(tmp_path: Path) -> None:
    table = relocations(tmp_path)
    (tmp_path / "ssd").mkdir()
    (tmp_path / "ssd/b.json").write_text("x", encoding="utf-8")
    with pytest.raises(ReplayError, match="differs from its pin"):
        captions.restore(tmp_path / "gone/b.json", "0" * 64, table)


def test_a_missing_unrelocated_reference_is_refused(tmp_path: Path) -> None:
    with pytest.raises(ReplayError, match="was not relocated"):
        captions.restore(tmp_path / "elsewhere.json", "0" * 64, relocations(tmp_path))


def test_placement_runs_over_the_whole_output_with_the_yolo_track(tmp_path: Path) -> None:
    path = configure(tmp_path)
    producers = Producers(tmp_path)
    run_through(path, producers, "cut-plan", "paging")
    cuts = {"cuts": [{"source_start_frame": 10, "source_end_frame": 40, "output_start_frame": 60}]}
    (tmp_path / "replay-123/cut-plan-v1/supported-cuts.json").write_text(json.dumps(cuts), "utf-8")
    receipt = steps.execute("placement", path, run=producers, graph=captions.BACK)
    command = producers.commands[-1]
    assert command[command.index("--end-seconds") + 1] == "90/30"
    assert command[command.index("--yolo-track") + 1].endswith("yolo-track-v2.json")
    assert command[-4:] == ["--max-lines", "2", "--unknown-person", "region-top"]
    outputs = json.loads(receipt.read_text(encoding="utf-8"))["outputs"]
    assert (outputs["placedCues"], outputs["failedRuns"]) == (9, 1)
    assert outputs["calibration"]["status"].startswith("provisional")


def test_onsets_align_the_tokens_in_the_ctc_environment_once(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = configure(tmp_path)
    producers = Producers(tmp_path)
    monkeypatch.chdir(tmp_path)
    for name in ("cut-plan", "paging"):
        (tmp_path / "steps" / f"{name}.json").write_text("{}", encoding="utf-8")
    receipt = steps.execute("onsets", path, run=producers, graph=captions.BACK)
    command = producers.commands[-1]
    assert command[0] == "/usr/bin/env"
    # The package the step runs (here src/), not the working directory's.
    assert f"PYTHONPATH={Path(captions.__file__).resolve().parents[1]}" in command
    assert f"TMPDIR={tmp_path / 'replay-123/speech-onsets-scratch'}" in command
    assert command[command.index("-m") - 2 :] == [
        str(tmp_path / "edits/.venv-ctc-alignment/bin/python"), "-B",
        "-m", "replay.speech_onsets",
        str(tmp_path / "replay-123/cut-plan-v1/projected-tokens.json"),
        str(tmp_path / "replay-123/paging-v1/prepared/tempo/candidate.wav"),
        str(tmp_path / "replay-123/speech-onsets-v1.json"),
        "--model", str(tmp_path / "edits/models/japanese-wav2vec2-large-rs35kh-2162c9ed"),
    ]  # fmt: skip
    outputs = json.loads(receipt.read_text(encoding="utf-8"))["outputs"]
    assert outputs["onsets"]["path"] == str(tmp_path / "replay-123/speech-onsets-v1.json")
    receipt.unlink()
    steps.execute("onsets", path, run=producers, graph=captions.BACK)
    assert len(producers.commands) == 1  # an existing onsets file is reused


def test_parts_compose_and_finish_chain_the_producers_without_captions(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = configure(tmp_path)
    producers = Producers(tmp_path)
    (tmp_path / "replay-123/placement-v1.json").write_text("{}", encoding="utf-8")
    upstream = ("cut-plan", "paging", "placement", "onsets")
    for name in upstream:
        (tmp_path / "steps" / f"{name}.json").write_text("{}", encoding="utf-8")
    monkeypatch.setattr(captions, "packet_count", lambda _p: -1)  # the join's count is used
    run_through(path, producers, "caption-parts", "compose", "finish")
    assert modules(producers) == [
        "replay.caption_parts", "replay.caption_compose", "replay.caption_compose",
        "replay.caption_compose",
    ]  # fmt: skip
    parts = producers.commands[0]
    assert parts[parts.index("--") + 1 :] == ["--font-size", "82"]
    assert parts[parts.index("--tokens") + 1] == str(tmp_path / "replay-123/speech-onsets-v1.json")
    planned = producers.commands[1]
    uncaptioned = tmp_path / "replay-123/compose-v1/uncaptioned-parts.json"
    assert planned[planned.index("plan") + 2 : planned.index("plan") + 4] == [
        str(uncaptioned), str(tmp_path / "clean.mov")
    ]  # fmt: skip
    assert json.loads(uncaptioned.read_text(encoding="utf-8")) == []
    assert "--states" not in planned
    assert planned[-4:] == [
        "--rate-source", str(tmp_path / "source.mp4"),
        "--frame-clock", str(tmp_path / "replay-123/video-clock.json"),
    ]  # fmt: skip
    composed = json.loads((tmp_path / "steps/compose.json").read_text(encoding="utf-8"))
    assert composed["outputs"]["averageBitRate"] == 8_200_000
    render = producers.commands[2]
    assert render[render.index("--tempo") :] == [
        "--tempo", str(tmp_path / "replay-123/paging-v1/prepared/tempo/candidate.wav"),
        "--audio", str(tmp_path / "media/replay-123/final/audio/tempo.m4a"),
        "--ffmpeg", "/ffmpeg",
    ]  # fmt: skip
    finish = json.loads((tmp_path / "steps/finish.json").read_text(encoding="utf-8"))
    assert finish["outputs"]["packets"] == 7
    assert (tmp_path / "media/replay-123/final/candidate.mp4").read_bytes() == b"movie"


def test_a_recording_palette_reaches_the_caption_parts(tmp_path: Path) -> None:
    path = configure(tmp_path)
    settings = json.loads((tmp_path / "captions.json").read_text(encoding="utf-8"))
    green = {"gradientTop": "#B6DF9E", "gradientBottom": "#6DBE3C"}
    (tmp_path / "captions.json").write_text(json.dumps({**settings, "palette": green}), "utf-8")
    producers = Producers(tmp_path)
    (tmp_path / "replay-123/placement-v1.json").write_text("{}", encoding="utf-8")
    for name in ("cut-plan", "paging", "placement", "onsets"):
        (tmp_path / "steps" / f"{name}.json").write_text("{}", encoding="utf-8")
    run_through(path, producers, "caption-parts")
    parts = producers.commands[0]
    assert parts[parts.index("--palette") : parts.index("--")] == [
        "--palette", "#B6DF9E", "#6DBE3C"
    ]  # fmt: skip
    (tmp_path / "captions.json").write_text(
        json.dumps({**settings, "palette": {"gradientTop": "#000000"}}), "utf-8"
    )
    with pytest.raises(ReplayError, match="exactly gradientTop"):
        captions.load_captions(steps.load(path))


def test_caption_parts_whose_text_flashes_by_are_refused_before_the_render(
    tmp_path: Path,
) -> None:
    path = configure(tmp_path)
    (tmp_path / "replay-123/placement-v1.json").write_text("{}", encoding="utf-8")
    for name in ("cut-plan", "paging", "placement", "onsets"):
        (tmp_path / "steps" / f"{name}.json").write_text("{}", encoding="utf-8")
    parts = tmp_path / "replay-123/caption-parts-v1"
    (parts / "moti").mkdir(parents=True)
    for name in ("parts.json", "jobs.json"):
        (parts / name).write_text("[]", encoding="utf-8")
    flash = {"text": "あい", "startFrame": 0, "endFrame": 10, "graphemeFrames": [0, 9]}
    (parts / "moti/w000-part-0000.reveal.json").write_text(
        json.dumps({"offsetFrame": 0, "titles": [flash]}), encoding="utf-8"
    )
    with pytest.raises(ReplayError, match="too briefly"):
        captions.caption_parts_step(steps.load(path), Producers(tmp_path))


@pytest.mark.parametrize(
    ("change", "message"),
    [
        ({"version": 2}, "Unsupported caption"),
        ({"references": {}}, "Incomplete caption references"),
        ({"compose": {}}, "Incomplete compose settings"),
        ({"motionFinish": [1]}, "list of arguments"),
        ({"mediaRoot": "relative"}, "absolute"),
    ],
)
def test_invalid_caption_settings_are_refused(
    tmp_path: Path, change: dict[str, object], message: str
) -> None:
    path = configure(tmp_path)
    settings = json.loads((tmp_path / "captions.json").read_text(encoding="utf-8"))
    (tmp_path / "captions.json").write_text(json.dumps({**settings, **change}), "utf-8")
    with pytest.raises(ReplayError, match=message):
        captions.load_captions(steps.load(path))


def test_a_source_digest_for_another_file_is_refused(tmp_path: Path) -> None:
    path = configure(tmp_path)
    (tmp_path / "replay-123/source.sha256").write_text(f"{'b' * 64}  /other\n", "utf-8")
    with pytest.raises(ReplayError, match="does not name"):
        captions.source_digest(steps.load(path))


def test_an_empty_cut_plan_has_no_duration(tmp_path: Path) -> None:
    (tmp_path / "cuts.json").write_text('{"cuts": []}', encoding="utf-8")
    with pytest.raises(ReplayError, match="empty"):
        captions.output_seconds(tmp_path / "cuts.json")


def test_a_missing_closure_file_is_refused(tmp_path: Path) -> None:
    (tmp_path / "c.json").write_text('{"inputs": {"/missing.py": "0"}}', encoding="utf-8")
    with pytest.raises(ReplayError, match="missing"):
        captions.refreshed_closure(tmp_path / "c.json", tmp_path / "out.json")


def test_the_cli_runs_one_caption_step(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    path = configure(tmp_path)
    monkeypatch.setattr(steps, "Producer", lambda: Producers(tmp_path))
    assert captions.main(["cut-plan", str(path)]) == 0
    assert (tmp_path / "steps/cut-plan.json").exists()


def test_a_paging_plan_without_a_paging_stage_is_refused(tmp_path: Path) -> None:
    plan = tmp_path / "workflow.json"
    plan.write_text(json.dumps({"version": 1, "stages": [{"command": ["true"]}]}), "utf-8")
    with pytest.raises(ReplayError, match="no paging stage"):
        captions.limit_paging_lines(plan)


def test_the_mono_receipt_is_the_context_streams_own_when_present(tmp_path: Path) -> None:
    assert captions.mono_receipt(tmp_path) == tmp_path / "state/mono.json"
    (tmp_path / "mono-receipt.json").write_text("{}", encoding="utf-8")
    assert captions.mono_receipt(tmp_path) == tmp_path / "mono-receipt.json"


def test_upload_runs_the_web_viewer_sync_under_its_lock(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    pipeline = steps.load(configure(tmp_path))
    (tmp_path / "steps" / "publish.json").write_text(
        '{"outputs": {"delivery": "/d/movie.mp4"}}', encoding="utf-8"
    )
    monkeypatch.setattr(viewer, "WEB_VIEWER", tmp_path / "web")
    monkeypatch.setattr("shutil.which", lambda _name: "/bin/bun")
    monkeypatch.setattr(time, "monotonic", lambda: 3.0)
    checked: list[tuple[str, Path]] = []
    monkeypatch.setattr(
        viewer, "require_uploaded", lambda video, movie, root: checked.append((video, movie))
    )
    (tmp_path / "web").mkdir()
    commands: list[list[str]] = []
    outputs = captions.upload(pipeline, lambda command, _log: commands.append(list(command)))
    assert commands == [["/bin/bun", str(tmp_path / "web/src/bin/sync.ts"), "--only", "movie"]]
    log = tmp_path / "steps/logs/upload.log"
    assert log.read_text(encoding="utf-8") == "PHASE lock 0.0 s\n"
    assert checked == [("movie", Path("/d/movie.mp4"))]
    assert outputs == {"published": {"delivery": "/d/movie.mp4"}}
    assert (tmp_path / "web/sync.lock").exists()


def test_upload_without_bun_is_refused(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    pipeline = steps.load(configure(tmp_path))
    (tmp_path / "steps" / "publish.json").write_text(
        '{"outputs": {"delivery": "/d/movie.mp4"}}', encoding="utf-8"
    )
    monkeypatch.setattr("shutil.which", lambda _name: None)
    with pytest.raises(ReplayError, match="bun is required"):
        captions.upload(pipeline, lambda _command, _log: None)


def test_source_probe_records_the_original_once(tmp_path: Path) -> None:
    captions.source_probe(tmp_path, "/m/2026-10-14_18-30-45_JST_replay-1.mp4")
    captions.source_probe(tmp_path, "/m/2026-10-14_18-30-45_JST_replay-1.mp4")
    assert json.loads((tmp_path / "evidence/source-probe.json").read_text(encoding="utf-8")) == {
        "format": {"filename": "/m/2026-10-14_18-30-45_JST_replay-1.mp4"}
    }
    with pytest.raises(ReplayError, match="different original"):
        captions.source_probe(tmp_path, "/m/other.mp4")


def test_source_probe_follows_a_relocated_original(tmp_path: Path) -> None:
    captions.source_probe(tmp_path, "/m/2026-10-07_18-30-20_JST_replay-1.mp4")
    captions.source_probe(tmp_path, "/m/replay-1/2026-10-07_18-30-20_JST_replay-1.mp4")
    assert json.loads((tmp_path / "evidence/source-probe.json").read_text(encoding="utf-8")) == {
        "format": {"filename": "/m/replay-1/2026-10-07_18-30-20_JST_replay-1.mp4"}
    }


def test_publish_writes_the_storage_source_probe(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    pipeline = steps.load(configure(tmp_path))
    (tmp_path / "steps" / "finish.json").write_text(
        '{"outputs": {"movie": {"path": "/f/candidate.mp4", "sha256": "abcdef0123456789"}}}',
        encoding="utf-8",
    )
    storage = tmp_path / "storage"
    delivery = storage / "revisions/pipeline-abcdef012345/delivery/movie.mp4"
    index = storage / "revisions/pipeline-abcdef012345/metadata/pipeline-evidence.json"
    index.parent.mkdir(parents=True)
    index.write_text('{"source": "/m/source.mp4"}', encoding="utf-8")
    seen: list[str] = []

    def publish(_root: Path, **options: object) -> tuple[Path, Path]:
        seen.append(str(options["revision"]))
        return delivery, index

    monkeypatch.setattr(pipeline_completion, "publish", publish)
    released: list[tuple[Path, Path]] = []
    monkeypatch.setattr(
        viewer, "release_older", lambda root, published: released.append((root, published))
    )
    outputs = captions.publish(pipeline, lambda _command, _log: None)
    assert seen == ["pipeline-abcdef012345"]
    assert released == [(tmp_path / "edit", delivery)]
    assert outputs["delivery"] == str(delivery)
    assert json.loads((storage / "evidence/source-probe.json").read_text(encoding="utf-8")) == {
        "format": {"filename": "/m/source.mp4"}
    }


def test_an_unverified_candidate_selection_is_moved_aside(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(time, "time_ns", lambda: 42)
    selection = tmp_path / "evidence/candidate-selection.json"
    selection.parent.mkdir()
    selection.write_text('{"status": "unverified-candidate"}', encoding="utf-8")
    retired = captions.retire_selection(tmp_path)
    assert retired == tmp_path / "evidence/retired/candidate-selection-retired-42.json"
    assert retired.read_text(encoding="utf-8") == '{"status": "unverified-candidate"}'
    assert not selection.exists()
    assert captions.retire_selection(tmp_path) is None


def test_a_selection_of_another_status_is_kept(tmp_path: Path) -> None:
    selection = tmp_path / "evidence/candidate-selection.json"
    selection.parent.mkdir()
    selection.write_text('{"status": "verified"}', encoding="utf-8")
    assert captions.retire_selection(tmp_path) is None
    assert selection.read_text(encoding="utf-8") == '{"status": "verified"}'


def renders(tmp_path: Path, *, gate: object, evidence: str) -> steps.Pipeline:
    """A recording that published, summarized, and still holds its render media."""
    pipeline = steps.load(configure(tmp_path))
    media = tmp_path / "media/replay-123"
    (media / "alpha/w001").mkdir(parents=True)
    (media / "alpha/w001/part.mov").write_bytes(b"alpha")
    (media / "compose").mkdir()
    (media / "compose/g001.mov").write_bytes(b"composed")
    (media / "final").mkdir()
    (media / "final/candidate.mp4").write_bytes(b"movie")
    index = tmp_path / "storage/metadata/pipeline-evidence.json"
    index.parent.mkdir(parents=True)
    index.write_text(evidence, encoding="utf-8")
    published = {"delivery": str(media / "final/candidate.mp4"), "evidence": {"path": str(index)}}
    (tmp_path / "steps/publish.json").write_text(
        json.dumps({"outputs": published}), encoding="utf-8"
    )
    (tmp_path / "steps/summary.json").write_text(
        json.dumps({"outputs": {"gate": gate}}), encoding="utf-8"
    )
    return pipeline


def test_renders_are_retired_after_the_gate_passed(tmp_path: Path) -> None:
    pipeline = renders(tmp_path, gate={"complete": True}, evidence="{}")
    outputs = captions.retire_renders(pipeline, lambda _command, _log: None)
    assert outputs == {
        "removed": [
            {"path": str(tmp_path / "media/replay-123/alpha"), "bytes": 5},
            {"path": str(tmp_path / "media/replay-123/compose"), "bytes": 8},
        ],
        "bytes": 13,
    }
    assert (tmp_path / "media/replay-123/final/candidate.mp4").read_bytes() == b"movie"


def test_renders_renamed_aside_are_retired_with_them(tmp_path: Path) -> None:
    pipeline = renders(tmp_path, gate={"complete": True}, evidence="{}")
    media = tmp_path / "media/replay-123"
    (media / "compose.groups300-20261023").mkdir()
    (media / "compose.groups300-20261023/g001.mov").write_bytes(b"old")
    (media / "alpha.v1").mkdir()
    (media / "alpha.v1/part.mov").write_bytes(b"older")
    outputs = captions.retire_renders(pipeline, lambda _command, _log: None)
    assert outputs == {
        "removed": [
            {"path": str(tmp_path / "media/replay-123/alpha"), "bytes": 5},
            {"path": str(tmp_path / "media/replay-123/compose"), "bytes": 8},
            {"path": str(tmp_path / "media/replay-123/alpha.v1"), "bytes": 5},
            {"path": str(tmp_path / "media/replay-123/compose.groups300-20261023"), "bytes": 3},
        ],
        "bytes": 21,
    }
    assert (tmp_path / "media/replay-123/final/candidate.mp4").read_bytes() == b"movie"


def test_a_renamed_render_named_by_the_evidence_is_kept(tmp_path: Path) -> None:
    media = tmp_path / "media/replay-123"
    pipeline = renders(tmp_path, gate={"complete": True}, evidence=f'["{media}/compose.old/g.mov"]')
    (media / "compose.old").mkdir()
    (media / "compose.old/g.mov").write_bytes(b"old")
    with pytest.raises(ReplayError, match="still references"):
        captions.retire_renders(pipeline, lambda _command, _log: None)
    assert (media / "compose.old/g.mov").read_bytes() == b"old"


def test_renders_named_by_the_evidence_are_kept(tmp_path: Path) -> None:
    media = tmp_path / "media/replay-123"
    pipeline = renders(tmp_path, gate={"complete": True}, evidence=f'["{media}/compose/g001.mov"]')
    with pytest.raises(ReplayError, match="still references"):
        captions.retire_renders(pipeline, lambda _command, _log: None)
    assert (media / "compose/g001.mov").read_bytes() == b"composed"


def test_an_older_summary_passes_the_gate_before_renders_are_retired(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    pipeline = renders(tmp_path, gate=None, evidence="{}")
    gated: list[str] = []

    def gate(passed: steps.Pipeline) -> dict[str, object]:
        gated.append(passed.text("recording"))
        return {}

    monkeypatch.setattr(viewer, "gate", gate)
    outputs = captions.retire_renders(pipeline, lambda _command, _log: None)
    assert gated == ["123"]
    assert outputs["bytes"] == 13


def test_a_failed_gate_keeps_the_renders(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    pipeline = renders(tmp_path, gate={"complete": False}, evidence="{}")

    def refuse(_pipeline: steps.Pipeline) -> dict[str, object]:
        raise ReplayError("The completion gate did not pass")

    monkeypatch.setattr(viewer, "gate", refuse)
    with pytest.raises(ReplayError, match="gate did not pass"):
        captions.retire_renders(pipeline, lambda _command, _log: None)
    assert (tmp_path / "media/replay-123/alpha/w001/part.mov").read_bytes() == b"alpha"


def test_finish_counts_a_movie_left_by_an_earlier_run_and_refuses_a_changed_plan(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = configure(tmp_path)
    pipeline = steps.load(path)
    movie = captions.final_movie(pipeline)
    movie.parent.mkdir(parents=True)
    movie.write_bytes(b"movie")
    monkeypatch.setattr(captions, "packet_count", lambda _p: 9)
    assert captions.finish(pipeline, lambda _c, _l: None)["packets"] == 9
    groups = tmp_path / "groups.json"
    groups.write_text('[{"frames": 4}]', encoding="utf-8")
    receipt = {"outputs": {"groups": {"path": str(groups), "sha256": "0" * 64}}}
    (tmp_path / "steps" / "compose.json").write_text(json.dumps(receipt), encoding="utf-8")
    with pytest.raises(ReplayError, match="differs from the compose receipt's pin"):
        captions.planned_frames(pipeline)


def test_finish_fetches_an_offloaded_candidate_back_before_counting_it(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    pipeline = steps.load(configure(tmp_path))
    movie = captions.final_movie(pipeline)
    asked: list[tuple[Path, Path]] = []

    def restore(path: Path, *, root: Path) -> bool:
        asked.append((path, root))
        path.parent.mkdir(parents=True)
        path.write_bytes(b"movie")
        return True

    monkeypatch.setattr(r2_fetch, "restore_if_offloaded", restore)
    monkeypatch.setattr(captions, "packet_count", lambda _p: 9)
    assert captions.finish(pipeline, lambda _c, _l: None)["packets"] == 9
    assert asked == [(movie, pipeline.path_of("editRoot"))]


def test_closure_files_map_to_the_package_this_step_runs() -> None:
    package = Path(captions.__file__).resolve().parent
    assert captions.running_copy("/old/checkout/src/replay/stages.py") == str(package / "stages.py")
    assert captions.running_copy("/old/checkout/src/replay/sub/x.py") == str(package / "sub/x.py")
    assert captions.running_copy("/usr/lib/python3/os.py") == "/usr/lib/python3/os.py"


def native_render(tmp_path: Path) -> dict[str, object]:
    return {"binary": "/native/caption-alpha-render", "binarySha256": "a" * 64,
            "floor": "/pinned/floor.json", "sample": 2, "workers": 2}  # fmt: skip


def test_incomplete_native_render_settings_are_refused(tmp_path: Path) -> None:
    path = configure(tmp_path)
    settings = json.loads((tmp_path / "captions.json").read_text(encoding="utf-8"))
    (tmp_path / "captions.json").write_text(
        json.dumps({**settings, "nativeRender": {"binary": "/b"}}), "utf-8"
    )
    with pytest.raises(ReplayError, match="Incomplete nativeRender settings"):
        captions.load_captions(steps.load(path))


def test_a_voice_level_policy_reaches_compose_and_finish_and_is_pinned(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = configure(tmp_path)
    settings = json.loads((tmp_path / "captions.json").read_text(encoding="utf-8"))
    level = {"targetLufs": -18, "truePeakDbtp": -1}
    (tmp_path / "captions.json").write_text(
        json.dumps({**settings, "voiceLoudness": level}), "utf-8"
    )
    producers = Producers(tmp_path)
    (tmp_path / "steps/caption-parts.json").write_text("{}", encoding="utf-8")
    sidecar = tmp_path / "media/replay-123/final/audio/tempo.m4a.loudness.json"
    sidecar.parent.mkdir(parents=True)
    sidecar.write_text('{"gainDb": -9.4}', encoding="utf-8")
    monkeypatch.setattr(captions, "packet_count", lambda _p: -1)
    run_through(path, producers, "compose", "finish")
    render, finished = producers.commands[1], producers.commands[2]
    assert render[-4:] == ["--loudness-target", "-18", "--true-peak-ceiling", "-1"]
    assert finished[-4:] == ["--loudness-target", "-18", "--true-peak-ceiling", "-1"]
    composed = json.loads((tmp_path / "steps/compose.json").read_text(encoding="utf-8"))
    assert composed["outputs"]["voiceLoudness"]["path"] == str(sidecar)


def test_an_invalid_voice_level_policy_is_refused(tmp_path: Path) -> None:
    path = configure(tmp_path)
    settings = json.loads((tmp_path / "captions.json").read_text(encoding="utf-8"))
    (tmp_path / "captions.json").write_text(
        json.dumps({**settings, "voiceLoudness": {"targetLufs": "loud", "truePeakDbtp": -1}}),
        "utf-8",
    )
    with pytest.raises(ReplayError, match="Incomplete voiceLoudness settings"):
        captions.load_captions(steps.load(path))


def test_captions_read_the_stock_phrase_filtered_labels_when_the_asr_step_wrote_them(
    tmp_path: Path,
) -> None:
    assert captions.asr_labels(tmp_path) == tmp_path / "labels-silence-v1.json"
    (tmp_path / "labels-hallucination-v1.json").write_text("{}", encoding="utf-8")
    assert captions.asr_labels(tmp_path) == tmp_path / "labels-hallucination-v1.json"


def test_captions_read_the_glossary_corrected_labels_when_the_asr_step_wrote_them(
    tmp_path: Path,
) -> None:
    (tmp_path / "labels-hallucination-v1.json").write_text("{}", encoding="utf-8")
    (tmp_path / "labels-glossary-v1.json").write_text("{}", encoding="utf-8")
    assert captions.asr_labels(tmp_path) == tmp_path / "labels-glossary-v1.json"


class LayoutProducers(Producers):
    """Also writes what the nameplate, the mix decode and the mix's tempo render would."""

    def __call__(self, command: Sequence[str], log: Path) -> None:
        if command[0] == "/ffmpeg":
            Path(command[-1]).write_bytes(b"mix")
            self.commands.append(list(command))
            return
        name = command[command.index("-m") + 1] if "-m" in command else ""
        arguments = list(command[command.index("-m") + 2 :]) if name else []
        if name == "replay.caption_layout":
            Path(arguments[2]).parent.mkdir(parents=True, exist_ok=True)
            Path(arguments[2]).write_bytes(b"badge")
        elif name == "replay.clocked_tempo_audio":
            Path(arguments[1]).mkdir()
            (Path(arguments[1]) / "candidate.wav").write_bytes(b"mix tempo")
            (Path(arguments[1]) / "receipt.json").write_text("{}", encoding="utf-8")
        super().__call__(command, log)


def layout_settings(tmp_path: Path) -> dict[str, object]:
    icon, font = tmp_path / "icon.png", tmp_path / "font.ttf"
    icon.write_bytes(b"icon")
    font.write_bytes(b"font")
    speaker = {
        "name": "speaker", "icon": str(icon), "iconSha256": steps.sha256(icon), "font": str(font),
        "fontSha256": steps.sha256(font), "iconX": 600, "iconY": 860, "iconSize": 150,
        "nameSize": 36,
    }  # fmt: skip
    return {
        "version": 1, "canvas": [1920, 1080], "align": "left", "originX": 780,
        "regionTop": 560, "regionBottom": 1056, "regionRight": 1880, "speaker": speaker,
    }  # fmt: skip


def with_intents(tmp_path: Path, **intents: object) -> Path:
    path = configure(tmp_path)
    settings = json.loads((tmp_path / "captions.json").read_text(encoding="utf-8"))
    (tmp_path / "captions.json").write_text(json.dumps({**settings, **intents}), "utf-8")
    return path


def test_a_layout_places_left_aligned_bottom_captions_on_its_canvas(tmp_path: Path) -> None:
    path = with_intents(tmp_path, layout=layout_settings(tmp_path))
    producers = LayoutProducers(tmp_path)
    run_through(path, producers, "cut-plan", "paging")
    cuts = {"cuts": [{"source_start_frame": 10, "source_end_frame": 40, "output_start_frame": 60}]}
    (tmp_path / "replay-123/cut-plan-v1/supported-cuts.json").write_text(json.dumps(cuts), "utf-8")
    steps.execute("placement", path, run=producers, graph=captions.BACK)
    command = producers.commands[-1]
    assert command[command.index("--canvas") : command.index("--maximum-drift")] == [
        "--canvas", "1920", "1080", "--align", "left", "--origin-x", "780",
        "--region-top", "560", "--region-bottom", "1056", "--region-right", "1880",
        "--clearance", "0", "--ignore-person",
    ]  # fmt: skip
    assert "--yolo-track" not in command


def test_a_layout_reaches_the_parts_and_gives_compose_only_its_canvas(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = with_intents(tmp_path, layout=layout_settings(tmp_path))
    producers = LayoutProducers(tmp_path)
    (tmp_path / "replay-123/placement-v1.json").write_text("{}", encoding="utf-8")
    for name in ("cut-plan", "paging", "placement", "onsets"):
        (tmp_path / "steps" / f"{name}.json").write_text("{}", encoding="utf-8")
    run_through(path, producers, "caption-parts")
    parts = producers.commands[0]
    assert parts[parts.index("--width") : parts.index("--")] == [
        "--width", "1920", "--height", "1080", "--align", "left"
    ]  # fmt: skip
    monkeypatch.setattr(captions, "packet_count", lambda _p: -1)
    run_through(path, producers, "compose")
    planned = producers.commands[1]
    assert planned[-3:] == ["--canvas", "1920", "1080"]  # the nameplate is drawn live
    composed = json.loads((tmp_path / "steps/compose.json").read_text(encoding="utf-8"))
    assert "badge" not in composed["outputs"]


def test_a_layout_without_a_speaker_gives_compose_only_its_canvas(tmp_path: Path) -> None:
    layout = layout_settings(tmp_path)
    del layout["speaker"]
    path = with_intents(tmp_path, layout=layout)
    settings = captions.load_captions(steps.load(path))
    assert captions.compose_layout(settings) == ["--canvas", "1920", "1080"]


def mix_inputs(tmp_path: Path) -> Path:
    path = with_intents(
        tmp_path,
        deliveryAudio={"version": 1, "source": "original-mix", "intent": "keep the music"},
        voiceLoudness={"targetLufs": -18, "truePeakDbtp": -1},
    )
    work = tmp_path / "replay-123"
    vocals = work / "separation-v1/pipeline/vocals.wav"
    (work / "paging-v1/prepared").mkdir(parents=True)
    selection = {"version": 1, "source": steps.pin(vocals), "intervals": []}
    (work / "paging-v1/prepared/tempo-selection.json").write_text(json.dumps(selection), "utf-8")
    gain = {"version": 1, "gainDB": -0.5, "peakDB": 0.4,
            "source": {"path": str(tmp_path / "decodable.mp4"), "sha256": "d" * 64}}  # fmt: skip
    (work / "separation-v1/input-gain.json").write_text(json.dumps(gain), "utf-8")
    for name in ("cut-plan", "paging", "placement", "onsets", "caption-parts"):
        (tmp_path / "steps" / f"{name}.json").write_text("{}", encoding="utf-8")
    return path


def test_kept_music_is_the_mix_cut_and_timed_as_the_voice_and_levelled_by_it(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = mix_inputs(tmp_path)
    monkeypatch.setattr(captions, "pcm_layout", lambda _path: (44, 1000))
    monkeypatch.setattr(captions, "packet_count", lambda _p: -1)
    restored: list[tuple[str, ...]] = []
    monkeypatch.setattr(
        captions, "restore_music", lambda *paths: restored.append(tuple(p.name for p in paths))
    )
    producers = LayoutProducers(tmp_path)
    run_through(path, producers, "compose", "finish")
    work = tmp_path / "replay-123"
    root = work / "delivery-audio-v1"
    decode = producers.commands[1]
    assert decode[decode.index("-i") + 1] == str(tmp_path / "decodable.mp4")
    assert "asetpts=N/SR/TB,aresample=44100,volume=-0.500000dB" in decode
    assert (root / "mix.wav").read_bytes() == b"mix"
    assert restored == [(".mix.partial.wav", "vocals.wav", "vocals.wav")]
    rebound = json.loads((root / "tempo-selection.json").read_text(encoding="utf-8"))
    assert rebound["source"] == steps.pin(root / "mix.wav")
    tempo = producers.commands[2]
    assert tempo[tempo.index("-m") + 1 : tempo.index("--sha256")] == [
        "replay.clocked_tempo_audio", str(root / "tempo-selection.json"), str(root / "tempo")
    ]  # fmt: skip
    voice_tempo = str(work / "paging-v1/prepared/tempo/candidate.wav")
    render, finished = producers.commands[3], producers.commands[4]
    assert render[render.index("--tempo") + 1] == str(root / "tempo/candidate.wav")
    assert render[-2:] == ["--loudness-reference", voice_tempo]
    assert finished[finished.index("finish") + 2] == str(root / "tempo/candidate.wav")
    assert finished[-2:] == ["--loudness-reference", voice_tempo]
    finish = json.loads((tmp_path / "steps/finish.json").read_text(encoding="utf-8"))
    composed = json.loads((tmp_path / "steps/compose.json").read_text(encoding="utf-8"))
    assert finish["outputs"]["deliveryAudio"] == composed["outputs"]["deliveryAudio"]
    assert finish["outputs"]["deliveryAudio"]["intent"] == "keep the music"
    assert finish["outputs"]["deliveryAudio"]["tempo"]["path"] == str(root / "tempo/candidate.wav")


def test_a_mix_of_another_length_than_the_voice_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = mix_inputs(tmp_path)
    (tmp_path / "replay-123/separation-v1/input-gain.json").unlink()
    monkeypatch.setattr(
        captions, "pcm_layout", lambda path: (44, 999 if path.name.startswith(".mix") else 1000)
    )
    producers = LayoutProducers(tmp_path)
    pipeline = steps.load(path)
    with pytest.raises(ReplayError, match="differ in length"):
        captions.delivery_tempo(pipeline, captions.load_captions(pipeline), producers, tmp_path)
    decode = producers.commands[0]
    assert decode[decode.index("-i") + 1] == str(tmp_path / "source.mp4")
    assert "asetpts=N/SR/TB,aresample=44100,volume=-0.100000dB" in decode


def test_a_tempo_selection_of_another_voice_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = mix_inputs(tmp_path)
    monkeypatch.setattr(captions, "pcm_layout", lambda _path: (44, 1000))
    monkeypatch.setattr(captions, "restore_music", lambda *_paths: None)
    (tmp_path / "replay-123/separation-v1/pipeline/vocals.wav").write_bytes(b"changed")
    pipeline = steps.load(path)
    with pytest.raises(ReplayError, match="no longer names the separated voice"):
        captions.delivery_tempo(
            pipeline, captions.load_captions(pipeline), LayoutProducers(tmp_path), tmp_path
        )


def pcm16(path: Path, samples: list[int]) -> Path:
    with wave.open(str(path), "wb") as out:
        out.setnchannels(2)
        out.setsampwidth(2)
        out.setframerate(44100)
        out.writeframes(np.array(samples, np.int16).tobytes())
    return path


def test_the_music_returns_under_the_processed_voice(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(captions, "MUSIC_BLOCK", 3)  # several blocks, one partial
    mix = pcm16(tmp_path / "mix.wav", [100, 200, 32700, -32700, 0, 50])
    raw = pcm16(tmp_path / "raw.wav", [40, 200, -700, 700, 0, 0])
    voice = pcm16(tmp_path / "voice.wav", [10, 100, 0, 0, 7, 0])
    captions.restore_music(mix, raw, voice)
    with wave.open(str(mix)) as done:
        assert np.frombuffer(done.readframes(3), np.int16).tolist() == [
            70, 100, 32767, -32768, 7, 50
        ]  # fmt: skip
    short = pcm16(tmp_path / "short.wav", [1, 2])
    with pytest.raises(ReplayError, match="differ in layout"):
        captions.restore_music(mix, raw, short)


def test_a_rendered_mix_tempo_is_reused(tmp_path: Path) -> None:
    path = mix_inputs(tmp_path)
    tempo = tmp_path / "replay-123/delivery-audio-v1/tempo"
    tempo.mkdir(parents=True)
    (tempo / "receipt.json").write_text("{}", encoding="utf-8")
    pipeline = steps.load(path)
    producers = LayoutProducers(tmp_path)
    found = captions.delivery_tempo(pipeline, captions.load_captions(pipeline), producers, tmp_path)
    assert (found, producers.commands) == (tempo / "candidate.wav", [])


@pytest.mark.parametrize(
    ("intents", "message"),
    [
        ({"layout": {"version": 1}}, "caption-layout.json must hold exactly"),
        ({"deliveryAudio": {"version": 1, "source": "voice", "intent": "x"}}, "delivery-audio"),
    ],
)
def test_invalid_intents_are_refused(
    tmp_path: Path, intents: dict[str, object], message: str
) -> None:
    path = with_intents(tmp_path, **intents)
    with pytest.raises(ReplayError, match=message):
        captions.load_captions(steps.load(path))


def test_a_layouts_nameplate_is_retired_with_the_renders(tmp_path: Path) -> None:
    pipeline = renders(tmp_path, gate={"complete": True}, evidence="{}")
    badge = tmp_path / "media/replay-123/badge"
    badge.mkdir()
    (badge / "badge.mov").write_bytes(b"plate")
    outputs = captions.retire_renders(pipeline, lambda _command, _log: None)
    assert outputs == {
        "removed": [
            {"path": str(tmp_path / "media/replay-123/alpha"), "bytes": 5},
            {"path": str(tmp_path / "media/replay-123/compose"), "bytes": 8},
            {"path": str(badge), "bytes": 5},
        ],
        "bytes": 18,
    }
    assert not badge.exists()


def repairing(tmp_path: Path) -> Path:
    path = configure(tmp_path)
    settings = json.loads((tmp_path / "captions.json").read_text(encoding="utf-8"))
    (tmp_path / "captions.json").write_text(json.dumps({**settings, "voiceRepair": True}), "utf-8")
    (tmp_path / "steps/separation.json").write_text("{}", encoding="utf-8")
    return path


def test_without_voice_repair_the_separated_voice_passes_through(tmp_path: Path) -> None:
    path = configure(tmp_path)
    (tmp_path / "steps/separation.json").write_text("{}", encoding="utf-8")
    producers = Producers(tmp_path)
    receipt = steps.execute("voice-repair", path, run=producers, graph=captions.BACK)
    outputs = json.loads(receipt.read_text(encoding="utf-8"))["outputs"]
    assert outputs == {
        "voice": {
            "path": str(tmp_path / "replay-123/separation-v1/pipeline/vocals.wav"),
            "sha256": steps.sha256(tmp_path / "replay-123/separation-v1/pipeline/vocals.wav"),
        },
        "repaired": False,
    }
    assert producers.commands == []


def test_voice_repair_runs_offline_and_paging_reads_the_repaired_voice(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = repairing(tmp_path)
    producers = Producers(tmp_path)
    monkeypatch.chdir(tmp_path)
    run_through(path, producers, "voice-repair", "cut-plan", "paging")
    command = producers.commands[0]
    assert "HF_HUB_OFFLINE=1" in command
    assert f"TMPDIR={tmp_path / 'replay-123/voice-repair-scratch'}" in command
    assert command[command.index("-m") - 2 :] == [
        str(tmp_path / "edits/.venv-voice-repair/bin/python"), "-B",
        "-m", "replay.voice_repair",
        str(tmp_path / "replay-123/separation-v1/pipeline/vocals.wav"),
        str(tmp_path / "replay-123/voice-repair-v1"),
        "--source", str(tmp_path / "source.mp4"), "--gain-db", "-0.1", "--ffmpeg", "/ffmpeg",
        "--speaker", str(tmp_path / "edits/models/wavlm-base-plus-sv"),
    ]  # fmt: skip
    repaired = tmp_path / "replay-123/voice-repair-v1/vocals.wav"
    outputs = json.loads((tmp_path / "steps/voice-repair.json").read_text("utf-8"))["outputs"]
    assert outputs["voice"] == {"path": str(repaired), "sha256": steps.sha256(repaired)}
    assert outputs["repaired"] is True
    config = json.loads((tmp_path / "replay-123/paging-v1/config.json").read_text("utf-8"))
    assert config["inputs"]["stereoPCM"] == {
        "path": str(repaired),
        "sha256": "454349e422f05297191ead13e21d3db520e5abef52055e4964b82fb213f593a1",
    }
    (tmp_path / "steps/voice-repair.json").unlink()
    steps.execute("voice-repair", path, run=producers, graph=captions.BACK)
    assert len(producers.commands) == 4  # an existing repair receipt is reused
    assert captions.step_outputs(steps.load(path), "voice-repair") == (
        tmp_path / "replay-123/voice-repair-v1",
        tmp_path / "replay-123/voice-repair-scratch",
        tmp_path / "replay-123/voice-deesser-v1",
        tmp_path / "replay-123/voice-deesser-v1-4db",
        tmp_path / "replay-123/voice-deesser-v1-2db",
        tmp_path / "replay-123/voice-restore-v1",
    )


def test_the_cut_plan_with_voice_repair_set_needs_the_repair_receipt(tmp_path: Path) -> None:
    path = repairing(tmp_path)
    with pytest.raises(ReplayError, match="voice-repair has not passed"):
        run_through(path, Producers(tmp_path), "cut-plan")


def test_a_paging_receipt_bound_before_voice_repair_existed_is_still_finished(
    tmp_path: Path,
) -> None:
    path = configure(tmp_path)
    (tmp_path / "steps/separation.json").write_text("{}", encoding="utf-8")
    producers = Producers(tmp_path)
    run_through(path, producers, "cut-plan", "paging")
    recorded = json.loads((tmp_path / "steps/paging.json").read_text("utf-8"))["inputs"]
    assert str(tmp_path / "steps/voice-repair.json") not in recorded
    run_through(path, producers, "voice-repair")
    assert steps.finished(steps.load(path), "paging", graph=captions.BACK) is True


def test_a_voice_repair_setting_other_than_true_is_refused(tmp_path: Path) -> None:
    path = configure(tmp_path)
    settings = json.loads((tmp_path / "captions.json").read_text(encoding="utf-8"))
    (tmp_path / "captions.json").write_text(json.dumps({**settings, "voiceRepair": 1}), "utf-8")
    with pytest.raises(ReplayError, match="voiceRepair takes only true"):
        captions.load_captions(steps.load(path))


def test_the_deesser_alone_turns_down_the_separated_voice_for_paging(tmp_path: Path) -> None:
    path = configure(tmp_path)
    settings = json.loads((tmp_path / "captions.json").read_text(encoding="utf-8"))
    (tmp_path / "captions.json").write_text(json.dumps({**settings, "voiceDeesser": True}), "utf-8")
    (tmp_path / "steps/separation.json").write_text("{}", encoding="utf-8")
    producers = Producers(tmp_path)
    run_through(path, producers, "voice-repair", "cut-plan", "paging")
    assert producers.commands[0] == [
        "/py", "-B", "-m", "replay.voice_deesser",
        str(tmp_path / "replay-123/separation-v1/pipeline/vocals.wav"),
        str(tmp_path / "replay-123/voice-deesser-v1"), "--max-reduction-db", "8",
    ]  # fmt: skip
    outputs = json.loads((tmp_path / "steps/voice-repair.json").read_text("utf-8"))["outputs"]
    deessed = tmp_path / "replay-123/voice-deesser-v1/vocals.wav"
    assert outputs["voice"] == {"path": str(deessed), "sha256": steps.sha256(deessed)}
    assert outputs["repaired"] is False
    config = json.loads((tmp_path / "replay-123/paging-v1/config.json").read_text("utf-8"))
    assert config["inputs"]["stereoPCM"]["path"] == str(deessed)
    lineage = json.loads(
        (tmp_path / "replay-123/cut-plan-v1/lineage-template.json").read_text("utf-8")
    )
    assert lineage["artifactPaths"]["stereoPCM"] == str(deessed)


def test_the_deesser_runs_on_the_repaired_voice_when_both_are_set(tmp_path: Path) -> None:
    path = repairing(tmp_path)
    settings = json.loads((tmp_path / "captions.json").read_text(encoding="utf-8"))
    (tmp_path / "captions.json").write_text(json.dumps({**settings, "voiceDeesser": True}), "utf-8")
    producers = Producers(tmp_path)
    run_through(path, producers, "voice-repair")
    assert producers.commands[1][-4:] == [
        str(tmp_path / "replay-123/voice-repair-v1/vocals.wav"),
        str(tmp_path / "replay-123/voice-deesser-v1"), "--max-reduction-db", "8",
    ]  # fmt: skip
    outputs = json.loads((tmp_path / "steps/voice-repair.json").read_text("utf-8"))["outputs"]
    assert outputs["repaired"] is True
    assert outputs["deesser"]["path"] == str(tmp_path / "replay-123/voice-deesser-v1/receipt.json")


def test_a_deesser_setting_other_than_true_is_refused(tmp_path: Path) -> None:
    path = configure(tmp_path)
    settings = json.loads((tmp_path / "captions.json").read_text(encoding="utf-8"))
    (tmp_path / "captions.json").write_text(json.dumps({**settings, "voiceDeesser": 1}), "utf-8")
    with pytest.raises(ReplayError, match="voiceDeesser takes only true"):
        captions.load_captions(steps.load(path))


def finished(tmp_path: Path) -> Path:
    path = configure(tmp_path)
    movie = tmp_path / "media/replay-123/final/candidate.mp4"
    movie.parent.mkdir(parents=True)
    movie.write_bytes(b"movie")
    (tmp_path / "steps/finish.json").write_text(
        json.dumps({"outputs": {"movie": {"path": str(movie), "sha256": "m"}}}), encoding="utf-8"
    )
    return path


def test_without_settings_voice_remux_passes_the_finished_movie_on(tmp_path: Path) -> None:
    path = finished(tmp_path)
    producers = Producers(tmp_path)
    receipt = steps.execute("voice-remux", path, run=producers, graph=captions.BACK)
    outputs = json.loads(receipt.read_text(encoding="utf-8"))["outputs"]
    assert outputs == {
        "movie": {"path": str(tmp_path / "media/replay-123/final/candidate.mp4"), "sha256": "m"},
        "remuxed": False,
    }
    assert producers.commands == []
    assert captions.delivered_movie(steps.load(path)) == outputs["movie"]


def test_voice_remux_restores_the_voice_and_remuxes_without_touching_the_video(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = finished(tmp_path)
    vocals = tmp_path / "replay-123/separation-v1/pipeline/vocals.wav"
    prepared = tmp_path / "replay-123/paging-v1/prepared"
    prepared.mkdir(parents=True)
    selection = {"version": 1, "source": steps.pin(vocals), "intervals": []}
    (prepared / "tempo-selection.json").write_text(json.dumps(selection), encoding="utf-8")
    (tmp_path / "voice-remux.json").write_text(
        json.dumps({"version": 1, "components": ["deesser"]}), encoding="utf-8"
    )
    encoded: list[object] = []

    def encode(tempo: Path, audio: Path, *, ffmpeg: str, loudness: object) -> Path:
        encoded.append((tempo, loudness))
        audio.parent.mkdir(parents=True, exist_ok=True)
        audio.write_bytes(b"aac")
        return audio

    def mux(movie: Path, audio: Path, output: Path, *, ffmpeg: str) -> int:
        output.parent.mkdir(parents=True)
        output.write_bytes(movie.read_bytes() + audio.read_bytes())
        return 5

    monkeypatch.setattr(captions, "encode_audio", encode)
    monkeypatch.setattr(voice_remux, "remuxed", mux)
    producers = Producers(tmp_path)
    receipt = steps.execute("voice-remux", path, run=producers, graph=captions.BACK)
    assert modules(producers) == [
        "replay.voice_remux", "replay.clocked_tempo_audio", "replay.voice_judge"
    ]  # fmt: skip
    restored = tmp_path / "replay-123/voice-remux-v1/voice/vocals.wav"
    rebound = json.loads((tmp_path / "replay-123/voice-remux-v1/tempo-selection.json").read_text())
    assert rebound == {**selection, "source": steps.pin(restored)}
    assert encoded == [(tmp_path / "replay-123/voice-remux-v1/tempo/candidate.wav", None)]
    outputs = json.loads(receipt.read_text(encoding="utf-8"))["outputs"]
    remuxed = tmp_path / "media/replay-123/final-remux/candidate.mp4"
    assert outputs["remuxed"] is True
    assert outputs["movie"] == steps.pin(remuxed)
    assert captions.delivered_movie(steps.load(path)) == steps.pin(remuxed)
    assert captions.step_outputs(steps.load(path), "voice-remux") == (
        tmp_path / "replay-123/voice-remux-v1",
        tmp_path / "media/replay-123/final-remux",
    )


def remux_fakes(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    vocals = tmp_path / "replay-123/separation-v1/pipeline/vocals.wav"
    prepared = tmp_path / "replay-123/paging-v1/prepared"
    prepared.mkdir(parents=True)
    selection = {"version": 1, "source": steps.pin(vocals), "intervals": []}
    (prepared / "tempo-selection.json").write_text(json.dumps(selection), encoding="utf-8")

    def encode(tempo: Path, audio: Path, *, ffmpeg: str, loudness: object) -> Path:
        audio.parent.mkdir(parents=True, exist_ok=True)
        audio.write_bytes(tempo.read_bytes())
        return audio

    def mux(movie: Path, audio: Path, output: Path, *, ffmpeg: str) -> int:
        output.parent.mkdir(parents=True)
        output.write_bytes(movie.read_bytes() + audio.read_bytes())
        return 5

    monkeypatch.setattr(captions, "encode_audio", encode)
    monkeypatch.setattr(voice_remux, "remuxed", mux)


def test_voice_remux_resamples_the_tempo_of_the_delivered_voice_and_judges_it(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = finished(tmp_path)
    remux_fakes(tmp_path, monkeypatch)
    (tmp_path / "voice-remux.json").write_text(
        json.dumps({"version": 1, "components": [], "tempo": "resample"}), encoding="utf-8"
    )
    producers = Producers(tmp_path)
    receipt = steps.execute("voice-remux", path, run=producers, graph=captions.BACK)
    vocals = tmp_path / "replay-123/separation-v1/pipeline/vocals.wav"
    work = tmp_path / "replay-123/voice-remux-v1"
    assert producers.commands == [
        ["/py", "-B", "-m", "replay.voice_tempo", str(vocals),
         str(tmp_path / "replay-123/paging-v1/prepared/tempo/receipt.json"), str(work / "tempo")],
        ["/py", "-B", "-m", "replay.voice_judge", "tempo", str(vocals),
         str(work / "tempo/candidate.wav"), str(work / "tempo/receipt.json"),
         str(work / "judge.json")],
    ]  # fmt: skip
    outputs = json.loads(receipt.read_text(encoding="utf-8"))["outputs"]
    assert outputs["tempoBackend"] == "resample"
    assert outputs["voice"] == steps.pin(vocals)
    assert outputs["judge"] == steps.pin(work / "judge.json")
    assert outputs["settings"] == steps.pin(tmp_path / "voice-remux.json")
    remuxed = tmp_path / "media/replay-123/final-remux/candidate.mp4"
    assert remuxed.read_bytes() == b"movieresampled"


def test_voice_judge_alone_delivers_the_judged_resampled_tempo(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = finished(tmp_path)
    remux_fakes(tmp_path, monkeypatch)
    settings = json.loads((tmp_path / "captions.json").read_text(encoding="utf-8"))
    (tmp_path / "captions.json").write_text(json.dumps({**settings, "voiceJudge": True}), "utf-8")
    vocals = tmp_path / "replay-123/separation-v1/pipeline/vocals.wav"
    (tmp_path / "steps/voice-repair.json").write_text(
        json.dumps({"outputs": {"voice": steps.pin(vocals)}}), encoding="utf-8"
    )
    producers = Producers(tmp_path)
    receipt = steps.execute("voice-remux", path, run=producers, graph=captions.BACK)
    assert modules(producers) == ["replay.voice_tempo", "replay.voice_judge"]
    outputs = json.loads(receipt.read_text(encoding="utf-8"))["outputs"]
    assert outputs["tempoBackend"] == "resample"
    assert "settings" not in outputs


def test_voice_judge_gates_the_deessed_voice(tmp_path: Path) -> None:
    path = configure(tmp_path)
    settings = json.loads((tmp_path / "captions.json").read_text(encoding="utf-8"))
    (tmp_path / "captions.json").write_text(
        json.dumps({**settings, "voiceDeesser": True, "voiceJudge": True}), "utf-8"
    )
    (tmp_path / "steps/separation.json").write_text("{}", encoding="utf-8")
    producers = Producers(tmp_path)
    run_through(path, producers, "voice-repair")
    report = tmp_path / "replay-123/voice-repair-v1/judge.json"
    assert producers.commands[1] == [
        "/py", "-B", "-m", "replay.voice_judge", "same",
        str(tmp_path / "replay-123/separation-v1/pipeline/vocals.wav"),
        str(tmp_path / "replay-123/voice-deesser-v1/vocals.wav"), str(report),
    ]  # fmt: skip
    assert len(producers.commands) == 2  # no ML resynthesis
    outputs = json.loads((tmp_path / "steps/voice-repair.json").read_text("utf-8"))["outputs"]
    assert outputs["judge"] == steps.pin(report)
    assert outputs["voice"] == steps.pin(tmp_path / "replay-123/voice-deesser-v1/vocals.wav")
    assert "restore" not in outputs


class Recolouring(Producers):
    """A judge that fails the first ``failures`` voices it hears (a re-coloured timbre)."""

    def __init__(self, tmp_path: Path, failures: int) -> None:
        super().__init__(tmp_path)
        self.failures = failures

    def __call__(self, command: Sequence[str], log: Path) -> None:
        super().__call__(command, log)
        if "replay.voice_judge" in command and self.failures:
            self.failures -= 1
            Path(command[-1]).write_text('{"passed": false}', encoding="utf-8")
            raise ReplayError("Producer failed (1)")


def deesser_judged(tmp_path: Path, producers: Producers) -> None:
    path = configure(tmp_path)
    settings = json.loads((tmp_path / "captions.json").read_text(encoding="utf-8"))
    (tmp_path / "captions.json").write_text(
        json.dumps({**settings, "voiceDeesser": True, "voiceJudge": True}), "utf-8"
    )
    (tmp_path / "steps/separation.json").write_text("{}", encoding="utf-8")
    run_through(path, producers, "voice-repair")


def test_a_voice_the_judge_finds_recoloured_is_de_essed_less_deeply(tmp_path: Path) -> None:
    producers = Recolouring(tmp_path, failures=1)
    deesser_judged(tmp_path, producers)
    assert [command[-1] for command in producers.commands] == [
        "8", str(tmp_path / "replay-123/voice-repair-v1/judge.json"),
        "4", str(tmp_path / "replay-123/voice-repair-v1/judge-4db.json"),
    ]  # fmt: skip
    outputs = json.loads((tmp_path / "steps/voice-repair.json").read_text("utf-8"))["outputs"]
    assert outputs["voice"] == steps.pin(tmp_path / "replay-123/voice-deesser-v1-4db/vocals.wav")
    assert outputs["judge"] == steps.pin(tmp_path / "replay-123/voice-repair-v1/judge-4db.json")


def test_a_voice_failing_the_judge_at_every_depth_is_refused(tmp_path: Path) -> None:
    with pytest.raises(ReplayError, match="every depth"):
        deesser_judged(tmp_path, Recolouring(tmp_path, failures=3))


def test_a_judge_that_crashes_without_a_report_is_not_a_failing_voice(tmp_path: Path) -> None:
    class Crashing(Producers):
        def __call__(self, command: Sequence[str], log: Path) -> None:
            if "replay.voice_judge" in command:
                raise ReplayError("Producer failed (2)")
            super().__call__(command, log)

    with pytest.raises(ReplayError, match="failed"):
        deesser_judged(tmp_path, Crashing(tmp_path))


def test_voice_judge_alone_runs_no_ml_restoration(tmp_path: Path) -> None:
    path = configure(tmp_path)
    settings = json.loads((tmp_path / "captions.json").read_text(encoding="utf-8"))
    (tmp_path / "captions.json").write_text(json.dumps({**settings, "voiceJudge": True}), "utf-8")
    (tmp_path / "steps/separation.json").write_text("{}", encoding="utf-8")
    producers = Producers(tmp_path)
    run_through(path, producers, "voice-repair")
    assert modules(producers) == []
    outputs = json.loads((tmp_path / "steps/voice-repair.json").read_text("utf-8"))["outputs"]
    assert "restore" not in outputs


def test_a_voice_judge_setting_other_than_true_is_refused(tmp_path: Path) -> None:
    path = configure(tmp_path)
    settings = json.loads((tmp_path / "captions.json").read_text(encoding="utf-8"))
    (tmp_path / "captions.json").write_text(json.dumps({**settings, "voiceJudge": "yes"}), "utf-8")
    with pytest.raises(ReplayError, match="voiceJudge takes only true"):
        captions.load_captions(steps.load(path))


def test_voice_remux_refuses_a_tempo_selection_of_another_voice(tmp_path: Path) -> None:
    path = finished(tmp_path)
    prepared = tmp_path / "replay-123/paging-v1/prepared"
    prepared.mkdir(parents=True)
    (prepared / "tempo-selection.json").write_text(
        json.dumps({"source": {"path": "/other.wav", "sha256": "x"}}), encoding="utf-8"
    )
    (tmp_path / "voice-remux.json").write_text(
        json.dumps({"version": 1, "components": ["deesser"]}), encoding="utf-8"
    )
    with pytest.raises(ReplayError, match="no longer names the delivered voice"):
        steps.execute("voice-remux", path, run=Producers(tmp_path), graph=captions.BACK)


def test_the_judge_alone_passes_a_delivery_that_keeps_its_music_on(tmp_path: Path) -> None:
    path = finished(tmp_path)
    settings = json.loads((tmp_path / "captions.json").read_text(encoding="utf-8"))
    intent = {"version": 1, "source": "original-mix", "intent": "kept"}
    (tmp_path / "captions.json").write_text(
        json.dumps({**settings, "deliveryAudio": intent, "voiceJudge": True})
    )
    producers = Producers(tmp_path)
    receipt = steps.execute("voice-remux", path, run=producers, graph=captions.BACK)
    assert json.loads(receipt.read_text(encoding="utf-8"))["outputs"]["remuxed"] is False
    assert producers.commands == []


def test_voice_remux_refuses_a_delivery_that_keeps_its_music(tmp_path: Path) -> None:
    path = finished(tmp_path)
    settings = json.loads((tmp_path / "captions.json").read_text(encoding="utf-8"))
    intent = {"version": 1, "source": "original-mix", "intent": "kept"}
    (tmp_path / "captions.json").write_text(json.dumps({**settings, "deliveryAudio": intent}))
    (tmp_path / "voice-remux.json").write_text(
        json.dumps({"version": 1, "components": ["deesser"]}), encoding="utf-8"
    )
    with pytest.raises(ReplayError, match="keeps its music has none"):
        steps.execute("voice-remux", path, run=Producers(tmp_path), graph=captions.BACK)


def test_publication_needs_a_finished_movie(tmp_path: Path) -> None:
    path = configure(tmp_path)
    with pytest.raises(ReplayError, match="Neither voice-remux nor finish"):
        captions.delivered_movie(steps.load(path))


def test_the_youtube_encode_step_encodes_the_published_delivery(tmp_path: Path) -> None:
    pipeline = steps.load(configure(tmp_path))
    delivery = tmp_path / "storage/revisions/p/delivery/S_JST_r-review.mp4"
    (tmp_path / "steps" / "publish.json").write_text(
        json.dumps({"outputs": {"delivery": str(delivery)}}), encoding="utf-8"
    )
    encoded = tmp_path / "storage/revisions/p/youtube/S_JST_r-review-youtube.mov"
    commands: list[list[str]] = []

    def run(command: Sequence[str], _log: Path) -> None:
        commands.append(list(command))
        encoded.parent.mkdir(parents=True)
        encoded.write_bytes(b"encoded")

    outputs = captions.youtube_encode_step(pipeline, run)
    assert commands[0][3:] == [
        "replay.youtube_encode", str(delivery), "--state",
        str(pipeline.work / "youtube-encode-v1"), "--ffprobe", pipeline.text("ffprobe"),
    ]  # fmt: skip
    assert outputs == {
        "youtube": {
            "path": str(encoded),
            "sha256": "766adc67b02bf315b9b5057994bfe6cfbd9354c433f259b29ba415dbe0f7afa5",
        }
    }


def test_the_youtube_master_step_replaces_the_delivery_and_syncs_it(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    pipeline = steps.load(configure(tmp_path))
    delivery = tmp_path / "storage/revisions/p/delivery/S_JST_r-review.mp4"
    (tmp_path / "steps" / "publish.json").write_text(
        json.dumps({"outputs": {"delivery": str(delivery)}}), encoding="utf-8"
    )
    published = tmp_path / "storage/revisions/y/delivery/S_JST_r-review.mp4"
    published.parent.mkdir(parents=True)
    published.write_bytes(b"master")
    seen: list[object] = []

    def master(movie: Path, **_tools: str) -> Path:
        seen.append(movie)
        return published

    monkeypatch.setattr(youtube_master, "master", master)
    monkeypatch.setattr(viewer, "sync", lambda run, log, *options: seen.append(options))
    monkeypatch.setattr(
        viewer, "require_uploaded", lambda video, movie, root: seen.append((video, movie))
    )
    assert captions.youtube_master_step(pipeline, lambda _command, _log: None) == {
        "master": {
            "path": str(published),
            "sha256": "fc613b4dfd6736a7bd268c8a0e74ed0d1c04a959f59dd74ef2874983fd443fc9",
        }
    }
    assert seen == [
        delivery,
        ("--only", "S_JST_r-review"),
        ("S_JST_r-review", published),
    ]


def test_the_caption_plan_step_writes_the_plan_and_faces_beside_the_delivery(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    pipeline = steps.load(configure(tmp_path))
    delivery = tmp_path / "storage/delivery/d-review.mp4"
    delivery.parent.mkdir(parents=True)
    (tmp_path / "steps/publish.json").write_text(
        json.dumps({"outputs": {"delivery": str(delivery)}}), encoding="utf-8"
    )
    plan = {"titles": [{"text": "a"}, {"text": "b"}], "burned": False}
    built: list[tuple[Path, Path]] = []

    def build(work: Path, movie: Path) -> tuple[dict[str, object], None]:
        built.append((work, movie))
        return plan, None

    def write_plan(value: dict[str, object], directory: Path, icon: Path | None) -> Path:
        target = directory / "d-review.captions.json"
        target.write_text(json.dumps(value), encoding="utf-8")
        return target

    def write_faces(value: dict[str, object], movie: Path) -> Path:
        target = movie.with_name("d-review.faces.json")
        target.write_text(json.dumps(value), encoding="utf-8")
        return target

    monkeypatch.setattr(caption_plan, "build", build)
    monkeypatch.setattr(caption_plan, "write", write_plan)
    monkeypatch.setattr(caption_faces, "build", lambda _work, _movie: {"samples": []})
    monkeypatch.setattr(caption_faces, "write", write_faces)
    receipt = captions.caption_plan_step(pipeline, lambda _c, _l: None)
    assert built == [(pipeline.work, delivery)]
    assert receipt == {
        "plan": steps.pin(delivery.with_name("d-review.captions.json")),
        "faces": steps.pin(delivery.with_name("d-review.faces.json")),
        "titles": 2, "burned": False,
    }  # fmt: skip
    assert captions.step_outputs(pipeline, "caption-plan") == ()


def test_paging_refuses_a_line_wider_than_the_frames_margins() -> None:
    captions.line_width_inside_margins(972.0, (1080, 1920))
    with pytest.raises(ReplayError, match="crosses the margins"):
        captions.line_width_inside_margins(973.0, (1080, 1920))


def test_the_reveal_reads_the_voice_onsets_once_they_exist(tmp_path: Path) -> None:
    pipeline = steps.load(configure(tmp_path))
    assert captions.reveal_tokens_path(pipeline) == pipeline.work / "speech-onsets-v1.json"
    voiced = pipeline.work / "speech-onsets-voice-v1.json"
    voiced.parent.mkdir(parents=True, exist_ok=True)
    voiced.write_text("{}", encoding="utf-8")
    assert captions.reveal_tokens_path(pipeline) == voiced


def test_incomplete_voice_onset_settings_are_refused(tmp_path: Path) -> None:
    path = configure(tmp_path)
    settings = path.parent / "captions.json"
    data = json.loads(settings.read_text(encoding="utf-8"))
    settings.write_text(json.dumps({**data, "voiceOnset": {"binary": "/b"}}), encoding="utf-8")
    with pytest.raises(ReplayError, match="Incomplete voiceOnset"):
        captions.load_captions(steps.load(path))


def test_onsets_with_voice_onset_settings_move_the_finished_onsets_once(tmp_path: Path) -> None:
    path = configure(tmp_path)
    settings = path.parent / "captions.json"
    data = json.loads(settings.read_text(encoding="utf-8"))
    tools = {"binary": "/b", "binarySha256": "b", "model": "/m", "modelSha256": "m"}
    settings.write_text(json.dumps({**data, "voiceOnset": tools}), encoding="utf-8")
    pipeline = steps.load(path)
    pipeline.work.mkdir(parents=True, exist_ok=True)
    (pipeline.work / "speech-onsets-v1.json").write_text("{}", encoding="utf-8")
    voiced = pipeline.work / "speech-onsets-voice-v1.json"
    ran: list[Sequence[str]] = []

    def writes(command: Sequence[str], log: Path) -> None:
        assert log.name == "onsets.log"
        ran.append(command)
        voiced.write_text("{}", encoding="utf-8")

    outputs = captions.onsets(pipeline, writes)
    assert set(outputs) == {"onsets", "voiceOnsets"}
    assert "replay.caption_voice_onset" in ran[0]
    captions.onsets(pipeline, writes)
    assert len(ran) == 1
