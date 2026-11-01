"""Audio-only remux: settings, voice restoration and a real stream-copy remux (local FFmpeg)."""

import json
import subprocess
import wave
from pathlib import Path

import pytest
from test_voice_deesser import voice_with_sibilants, write

from replay import voice_remux as remux
from replay import voice_repair as repair
from replay.core import ReplayError
from replay.native_join import probe_video

FFMPEG = "/opt/homebrew/bin/ffmpeg"


def settings(tmp_path: Path, value: object) -> Path:
    path = tmp_path / "voice-remux.json"
    path.write_text(json.dumps(value), encoding="utf-8")
    return path


def test_settings_name_known_components_only(tmp_path: Path) -> None:
    assert remux.settings(settings(tmp_path, {"version": 1, "components": ["deesser"]})) == (
        ["deesser"],
        "atempo",
    )
    with pytest.raises(ReplayError, match="takes version 1"):
        remux.settings(settings(tmp_path, {"version": 2, "components": ["deesser"]}))
    with pytest.raises(ReplayError, match="non-empty without a tempo"):
        remux.settings(settings(tmp_path, {"version": 1, "components": []}))
    with pytest.raises(ReplayError, match="unknown components robot"):
        remux.settings(settings(tmp_path, {"version": 1, "components": ["robot"]}))


def test_settings_may_name_only_the_resampled_tempo(tmp_path: Path) -> None:
    value = {"version": 1, "components": [], "tempo": "resample"}
    assert remux.settings(settings(tmp_path, value)) == ([], "resample")
    with pytest.raises(ReplayError, match="tempo is one of atempo, resample"):
        remux.settings(settings(tmp_path, {"version": 1, "components": [], "tempo": "rubber"}))
    with pytest.raises(ReplayError, match="optional tempo"):
        remux.settings(settings(tmp_path, {"version": 1, "components": [], "gain": 2}))


def test_the_voice_is_restored_by_each_component_with_its_length(tmp_path: Path) -> None:
    voice = write(tmp_path / "voice.wav", voice_with_sibilants(1.0))
    output = remux.restored(voice, tmp_path / "out", ["deesser"])
    assert output == tmp_path / "out/vocals.wav"
    assert repair.pcm(output).frames == repair.pcm(voice).frames
    assert (tmp_path / "out/00-deesser/receipt.json").is_file()
    assert remux.restored(voice, tmp_path / "out", ["deesser"]) == output  # reused


def test_ml_resynthesis_is_not_a_component(tmp_path: Path) -> None:
    path = tmp_path / "voice-remux.json"
    path.write_text(json.dumps({"version": 1, "components": ["restore"]}), encoding="utf-8")
    with pytest.raises(ReplayError, match="unknown components restore"):
        remux.settings(path)


def test_a_component_that_changes_the_length_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    voice = write(tmp_path / "voice.wav", voice_with_sibilants(1.0))
    short = write(tmp_path / "short.wav", voice_with_sibilants(0.5))
    monkeypatch.setitem(remux.COMPONENTS, "deesser", lambda _source, _directory: short)
    with pytest.raises(ReplayError, match="changed its length"):
        remux.restored(voice, tmp_path / "out", ["deesser"])


def movie(tmp_path: Path) -> Path:
    path = tmp_path / "movie.mp4"
    subprocess.run(
        [FFMPEG, "-v", "error", "-f", "lavfi", "-i", "testsrc=size=64x64:rate=30:duration=1",
         "-f", "lavfi", "-i", "sine=frequency=440:duration=1", "-c:v", "mpeg4", "-c:a", "aac",
         str(path)],
        check=True,
    )  # fmt: skip
    return path


def audio(tmp_path: Path) -> Path:
    path = tmp_path / "tempo.m4a"
    subprocess.run(
        [FFMPEG, "-v", "error", "-f", "lavfi", "-i", "sine=frequency=880:duration=1",
         "-c:a", "aac", str(path)],
        check=True,
    )  # fmt: skip
    return path


def test_the_movie_is_remuxed_with_the_new_audio_and_its_own_frames(tmp_path: Path) -> None:
    source, new_audio = movie(tmp_path), audio(tmp_path)
    output = tmp_path / "final-remux/candidate.mp4"
    assert remux.remuxed(source, new_audio, output, ffmpeg=FFMPEG) == 30
    assert probe_video(output).packets == 30
    with pytest.raises(ReplayError, match="Refusing to overwrite"):
        remux.remuxed(source, new_audio, output, ffmpeg=FFMPEG)


def test_a_remux_that_loses_frames_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source, new_audio = movie(tmp_path), audio(tmp_path)
    real = probe_video
    calls: list[Path] = []

    def counted(path: Path) -> object:
        calls.append(path)
        found = real(path)
        return found if len(calls) == 1 else type(found)(found.codec, found.time_base, 29)

    monkeypatch.setattr(remux, "probe_video", counted)
    with pytest.raises(ReplayError, match="does not hold the finished movie's frames"):
        remux.remuxed(source, new_audio, tmp_path / "out/candidate.mp4", ffmpeg=FFMPEG)
    assert not (tmp_path / "out/candidate.mp4").exists()


def test_the_cli_restores_a_voice(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    voice = write(tmp_path / "voice.wav", voice_with_sibilants(1.0))
    path = settings(tmp_path, {"version": 1, "components": ["deesser"]})
    assert remux.main(["restore", str(voice), str(tmp_path / "out"), str(path)]) == 0
    assert json.loads(capsys.readouterr().out) == {"voice": str(tmp_path / "out/vocals.wav")}
    with wave.open(str(tmp_path / "out/vocals.wav")) as stream:
        assert stream.getnframes() == 44_100
