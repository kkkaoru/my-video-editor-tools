"""A copy of a source in codecs the native (AVFoundation) tools decode, at the source's size.

OCR, caption composition and other native stages read the source through AVFoundation, which
refuses VP9/AV1 video (``AVFoundationErrorDomain -11869``) that YouTube serves. Such a source is
encoded once: video to HEVC with every frame's PTS kept, audio losslessly to ALAC (Opus is not
copied either), at the source's own size; scaling to the delivery canvas stays the composition's
last step. The derived file keeps the source's name in its own directory, beside a record
(source size and SHA-256, codecs, output SHA-256); a rerun reuses it only when the record still
matches. A source already in decodable codecs is returned as is.

A source whose stream lost packets inside a group of pictures (a live-stream splice: in decode
order a non-key packet more than ``SPLICE_FRAMES`` frame steps after the one before it) keeps
predicted frames whose references are gone; decoders then disagree on its frames and the software
decoder emits one out of presentation order (10/27: 0.54 s lost, one P frame decoded after the
next GOP's B frame), so no frame clock can match every consumer. Such a source is re-encoded once
like an undecodable one, in one FFmpeg pass that keeps every frame its decoder outputs strictly
after the previously kept one (``select`` on ``prev_selected_pts``) and drops the rest; no PTS is
altered (``-fps_mode passthrough`` alone would nudge a late one by a tick). FFmpeg and ffprobe
decode such a splice differently (10/27: ffmpeg stamped an orphaned frame with its DTS where
ffprobe emitted a late P frame), so the copy itself is the reference: its GOPs are whole, and it
must decode exactly its packets' PTS, strictly increasing, in software and in VideoToolbox (the
decoders the pipeline's consumers use). At most ``DROPS_PER_SPLICE`` source PTS per splice, or
``MAX_CHANGED_SHARE`` of the frames when more, may be lost or re-stamped (10/27: FFmpeg also
re-stamps the 3 B frames before each of 7 keyframes after a 2 s stall, 42 of 132,181 frames); the
splices, dropped and re-stamped PTS are recorded.
"""

import json
import math
import subprocess
from fractions import Fraction
from itertools import pairwise
from pathlib import Path

from replay.core import ReplayError
from replay.recordings import record
from replay.showinfo import SHOWN_PTS
from replay.source_lead_in import BITRATE, COLOUR, sha256, video_pts

DIRECTORY = "source-decodable"
RECORD = "decodable.json"
VIDEO_CODECS = frozenset({"h264", "hevc"})
AUDIO_CODECS = frozenset({"aac", "alac", "pcm_s16le", "pcm_s24le"})
# 10/27's source: 14 non-key steps of 4 frames decode consistently; its 16-frame splice does not.
SPLICE_FRAMES = 8
# A splice orphans the few predicted frames between the gap and the next keyframe's reordering
# (10/27: 1 frame); more drops mean something else is wrong.
DROPS_PER_SPLICE = 8
MAX_CHANGED_SHARE = 0.001
MONOTONIC = "select='isnan(prev_selected_pts)+gt(pts,prev_selected_pts)'"
SPLICE_CHECK = "splice-check.json"  # a source whose splices decode in order


def streams(source: Path, ffprobe: str) -> tuple[dict[str, object], dict[str, object]]:
    """The single video and single audio stream of a source."""
    result = subprocess.run(
        [ffprobe, "-v", "error", "-show_entries",
         "stream=codec_type,codec_name,time_base,color_range,color_space,color_primaries,"
         "color_transfer", "-of", "json", str(source)],
        capture_output=True, text=True, check=True,
    )  # fmt: skip
    found = record(json.loads(result.stdout)).get("streams")
    rows = [record(row) for row in found] if isinstance(found, list) else []
    video = [row for row in rows if row.get("codec_type") == "video"]
    audio = [row for row in rows if row.get("codec_type") == "audio"]
    if len(video) != 1 or len(audio) != 1:
        raise ReplayError("The source must hold exactly one video and one audio stream")
    return video[0], audio[0]


def splices(source: Path, ffprobe: str) -> list[int]:
    """DTS of the non-key packets that follow a gap of more than ``SPLICE_FRAMES`` frame steps
    in decode order: frames there predict from pictures the stream lost."""
    result = subprocess.run(
        [ffprobe, "-v", "error", "-select_streams", "v:0", "-show_entries", "packet=dts,flags",
         "-of", "csv=p=0", str(source)],
        capture_output=True, text=True, check=True,
    )  # fmt: skip
    rows = [line.split(",") for line in result.stdout.split()]
    packets = [(int(dts), "K" in flags) for dts, flags in rows if dts.lstrip("-").isdigit()]
    steps = sorted(later - earlier for (earlier, _), (later, _) in pairwise(packets))
    if not steps:
        return []
    step = steps[len(steps) // 2]
    return [
        dts
        for (previous, _), (dts, key) in pairwise(packets)
        if not key and dts - previous > SPLICE_FRAMES * step
    ]


def decoded_pts(movie: Path, ffprobe: str) -> list[int]:
    """Every decoded video frame's PTS, in the decoder's output order."""
    result = subprocess.run(
        [ffprobe, "-v", "error", "-threads", "0", "-select_streams", "v:0", "-show_entries",
         "frame=pts", "-of", "csv=p=0", str(movie)],
        capture_output=True, text=True, check=True,
    )  # fmt: skip
    return [int(line.strip(",")) for line in result.stdout.split()]


def encode_command(
    source: Path,
    output: Path,
    video: dict[str, object],
    audio: dict[str, object],
    *,
    ffmpeg: str,
    repair: bool = False,
) -> list[str]:
    """Re-encode only the codecs native tools cannot decode (all video when ``repair``ing a
    splice, keeping only frames decoded in presentation order); PTS (passthrough) are kept."""
    colour = [
        argument
        for key, option in COLOUR
        if isinstance(video.get(key), str) and video[key] != "unknown"
        for argument in (option, str(video[key]))
    ]
    video_codec = (
        ["-c:v", "copy"]
        if video.get("codec_name") in VIDEO_CODECS and not repair
        else ["-c:v", "hevc_videotoolbox", "-b:v", BITRATE, "-tag:v", "hvc1", *colour]
    )
    selected = ["-vf", MONOTONIC] if repair else []
    audio_codec = (
        ["-c:a", "copy"]
        if audio.get("codec_name") in AUDIO_CODECS
        # Opus decodes to floats: 24-bit ALAC keeps them to within one 16-bit step.
        else ["-c:a", "alac", "-sample_fmt", "s32p"]
    )
    return [
        ffmpeg, "-v", "error", "-nostdin", "-i", str(source), "-map", "0:v", "-map", "0:a",
        *selected, "-fps_mode", "passthrough",
        "-enc_time_base", str(Fraction(str(video["time_base"]))), *video_codec, *audio_codec,
        "-movflags", "+faststart", str(output),
    ]  # fmt: skip


def hardware_pts(movie: Path, ffmpeg: str) -> list[int]:
    """Every video frame's PTS as VideoToolbox decodes it (``showinfo``), in output order."""
    done = subprocess.run(
        [ffmpeg, "-v", "info", "-nostdin", "-copyts", "-hwaccel", "videotoolbox",
         "-hwaccel_output_format", "videotoolbox_vld", "-i", str(movie), "-map", "0:v:0",
         "-vf", "showinfo", "-fps_mode", "passthrough", "-f", "null", "-"],
        capture_output=True, check=True,
    )  # fmt: skip
    return [int(value) for value in SHOWN_PTS.findall(done.stderr)]


def repair_record(
    copy: Path, source: Path, *, ffmpeg: str, ffprobe: str, limit: int
) -> dict[str, list[int]]:
    """The source PTS a repaired copy dropped and the ones it re-stamped, once every decoder
    reads the copy's own packets in presentation order."""
    packets = video_pts(copy, ffprobe)
    for decoded in (decoded_pts(copy, ffprobe), hardware_pts(copy, ffmpeg)):
        if decoded != packets or any(later <= earlier for earlier, later in pairwise(decoded)):
            raise ReplayError("The repaired copy does not decode its frames in order; not used")
    original = set(video_pts(source, ffprobe))
    dropped, restamped = sorted(original - set(packets)), sorted(set(packets) - original)
    if len(dropped) + len(restamped) > max(limit, math.ceil(MAX_CHANGED_SHARE * len(original))):
        raise ReplayError(
            f"The splice repair would lose {len(dropped)} and re-stamp {len(restamped)} frames; "
            "it is not used"
        )
    return {"droppedPts": dropped, "restampedPts": restamped}


def broken_splices(source: Path, directory: Path, ffprobe: str) -> list[int]:
    """The source's splices whose frames the decoder does not output in presentation order, or
    none. A splice is confirmed by one software decode, whose verdict for
    a sound source is kept in ``SPLICE_CHECK`` so it is decoded once (a gap alone is harmless:
    10/02 and 10/14 decode in order across theirs)."""
    found = splices(source, ffprobe)
    if not found:
        return []
    check = directory / SPLICE_CHECK
    identity = {"source": str(source), "sourceSize": source.stat().st_size,
                "sourceSHA256": sha256(source), "splices": found}  # fmt: skip
    if check.exists() and json.loads(check.read_text(encoding="utf-8")) == identity:
        return []
    decoded, packets = decoded_pts(source, ffprobe), set(video_pts(source, ffprobe))
    if all(pts in packets for pts in decoded) and all(
        later > earlier for earlier, later in pairwise(decoded)
    ):
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        check.write_text(json.dumps(identity) + "\n", encoding="utf-8")
        return []
    return found


def prepared(source: Path, directory: Path, *, ffmpeg: str, ffprobe: str) -> Path:
    """The source itself when native tools decode it, else its verified decodable copy."""
    video, audio = streams(source, ffprobe)
    codecs = [str(video.get("codec_name")), str(audio.get("codec_name"))]
    broken = broken_splices(source, directory, ffprobe)
    if codecs[0] in VIDEO_CODECS and codecs[1] in AUDIO_CODECS and not broken:
        return source
    if directory.resolve() == source.resolve().parent:
        raise ReplayError("The decodable copy needs a directory apart from its source")
    output, receipt = directory / source.name, directory / RECORD
    expected = {
        "source": str(source),
        "sourceSize": source.stat().st_size,
        "sourceSHA256": sha256(source),
        "sourceCodecs": codecs,
        "output": str(output),
        # Recorded only for a repaired splice, so earlier copies keep matching their record.
        **({"splices": broken} if broken else {}),
    }
    if receipt.exists():
        recorded = record(json.loads(receipt.read_text(encoding="utf-8")))
        kept = {key: recorded.get(key) for key in expected}
        if kept != expected or recorded.get("outputSHA256") != sha256(output):
            raise ReplayError("The decodable copy no longer matches its record; remove it to redo")
        return output
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    partial = directory / f"partial-{output.name}"
    partial.unlink(missing_ok=True)
    subprocess.run(
        encode_command(source, partial, video, audio, ffmpeg=ffmpeg, repair=bool(broken)),
        check=True,
    )  # fmt: skip
    derived = [str(stream.get("codec_name")) for stream in streams(partial, ffprobe)]
    if derived[0] not in VIDEO_CODECS or derived[1] not in AUDIO_CODECS:
        raise ReplayError("The decodable copy is not in decodable codecs; it is not used")
    repaired = (
        repair_record(partial, source, ffmpeg=ffmpeg, ffprobe=ffprobe,
                      limit=DROPS_PER_SPLICE * len(broken))
        if broken
        else {}
    )  # fmt: skip
    if not broken and video_pts(partial, ffprobe) != video_pts(source, ffprobe):
        raise ReplayError("The decodable copy moved a source frame; it is not used")
    partial.rename(output)
    receipt.write_text(
        json.dumps({**expected, **repaired,
                    "outputCodecs": derived, "outputSHA256": sha256(output)}, indent=2)
        + "\n",
        encoding="utf-8",
    )  # fmt: skip
    return output
