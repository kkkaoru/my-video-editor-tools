#!/usr/bin/env python3
"""Build the web catalog from the same libraries TranscriptViewer scans.

Rules mirror TranscriptCatalog/CandidateSelection (subset, fail-closed):
 1. evidence/candidate-selection.json (validated: size + transcript SHA-256)
    selects the movie/transcript for that recording.
 2. Otherwise a canonical `<stamp>_JST_<folder>-review.mp4` under a `delivery`
    directory with a same-stem .srt/.csv beside it; superseded deliveries
    (evidence/superseded/*.json path+size match) are excluded.
Outputs data/catalog.json plus data/<id>.json (cues + saved summary) and, for a delivery with
a caption plan, data/<id>.captions.json.
Stdlib only. Never writes outside the given output dir.
"""

import argparse
import base64
import bisect
import csv
import hashlib
import io
import json
import math
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

RAW = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_JST_replay-(\d+)\.mp4$")
YOUTUBE_ORIGINAL = re.compile(
    r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_JST_youtube-([A-Za-z0-9_-]{11})$"
)
STAMP = re.compile(r"^(\d{4})-(\d{2})-(\d{2})_(\d{2})-(\d{2})-(\d{2})_JST_")


def stamp_label(name):
    m = STAMP.match(name)
    if not m:
        return None
    y, mo, d, h, mi, s = m.groups()
    return f"{y}-{mo}-{d} {h}:{mi}:{s} JST"


SPEAKER = re.compile(r"\[([A-Za-z0-9_-]{1,40})\]\s*")


def load_speakers(transcript_path):
    """Display names and colours of a multi-speaker transcript (<transcript>.speakers.json)."""
    p = transcript_path + ".speakers.json"
    if not os.path.isfile(p):
        return None
    rec = json.loads(Path(p).read_text(encoding="utf-8"))
    return {
        str(k): {"label": str(v["label"]), "color": str(v["color"])}
        for k, v in rec.get("speakers", {}).items()
        if re.fullmatch(r"#[0-9A-Fa-f]{6}", str(v.get("color", "")))
    } or None


def parse_srt(text):
    cues = []
    blocks = re.split(r"\n\s*\n", text.replace("\r\n", "\n").replace("\r", "\n"))
    for block in blocks:
        lines = [row for row in block.strip("\n").split("\n") if row.strip()]
        if len(lines) < 3 or not lines[0].strip().isdigit() or "-->" not in lines[1]:
            if block.strip():
                raise ValueError(f"malformed SRT block: {block[:40]!r}")
            continue
        start_raw, end_raw = lines[1].split("-->", 1)
        start_label, end_label = start_raw.strip(), end_raw.strip().split()[0]

        def sec(v):
            clock, ms = v.split(",")
            h, m, s = clock.split(":")
            return int(h) * 3600 + int(m) * 60 + int(s) + int(ms) / 1000

        text = "\n".join(lines[2:]).strip()
        speaker = SPEAKER.match(text)  # "[name] text": a multi-speaker transcript names each cue
        cues.append(
            {
                "id": len(cues),
                "start": sec(start_label),
                "end": sec(end_label),
                "startLabel": start_label,
                "endLabel": end_label,
                **({"speaker": speaker.group(1)} if speaker else {}),
                "text": text[speaker.end() :] if speaker else text,
            }
        )
    return cues


def parse_csv(text):
    rows = list(csv.reader(io.StringIO(text)))
    if not rows or rows[0] != ["start_seconds", "end_seconds", "start", "end", "text"]:
        raise ValueError("CSV header mismatch")
    return [
        {
            "id": i,
            "start": float(r[0]),
            "end": float(r[1]),
            "startLabel": r[2],
            "endLabel": r[3],
            "text": r[4],
        }
        for i, r in enumerate(rows[1:])
        if r
    ]


def load_cues(path):
    text = Path(path).read_text(encoding="utf-8")
    return parse_srt(text) if path.lower().endswith(".srt") else parse_csv(text)


def load_summary(transcript_path):
    p = transcript_path + ".summaries/current.json"
    if not os.path.isfile(p):
        return None
    try:
        rec = json.loads(Path(p).read_text(encoding="utf-8"))
        if rec.get("version") != 1 or not rec.get("sections") or not rec.get("overview"):
            return None
        return {
            "overview": rec["overview"],
            "sections": [
                {"start": s["startSeconds"], "end": s["endSeconds"], "text": s["text"]}
                for s in rec["sections"]
            ],
        }
    except (ValueError, KeyError, TypeError):
        return None


def current_excitement(transcript_path):
    """The excitement estimate beside a transcript, only while it was made from that transcript."""
    p = transcript_path + ".excitement.json"
    if not os.path.isfile(transcript_path) or not os.path.isfile(p):
        return None
    try:
        inputs = json.loads(Path(p).read_text(encoding="utf-8")).get("inputs") or {}
    except ValueError:
        return None
    digest = hashlib.sha256(Path(transcript_path).read_bytes()).hexdigest()
    return load_excitement(transcript_path) if inputs.get("transcriptSha256") == digest else None


def kept_data(data_path, video, out):
    """Copies a kept recording's published data, adding an excitement estimate computed since."""
    target = os.path.join(out, video["id"] + ".json")
    transcript = video.get("transcriptPath")
    excite = current_excitement(transcript) if transcript else None
    if excite is None:
        shutil.copyfile(data_path, target)
        return
    data = json.loads(Path(data_path).read_text(encoding="utf-8"))
    data["excitement"] = excite
    with open(target, "w", encoding="utf-8") as fh:
        json.dump(data, fh, ensure_ascii=False, separators=(",", ":"))


def load_excitement(transcript_path):
    """Per-bin excitement scores written by `python -m replay.excitement`."""
    p = transcript_path + ".excitement.json"
    if not os.path.isfile(p):
        return None
    try:
        rec = json.loads(Path(p).read_text(encoding="utf-8"))
    except ValueError:
        return None
    if rec.get("version") != 1:
        return None
    rec.pop("inputs", None)  # local fingerprints only
    return rec


LAYOUT_MODES = ("plan", "aboveFace", "bottom", "top", "center")
LAYOUT_NUMBERS = ("offsetX", "offsetY", "margin", "faceGap", "minScore", "maxDrift")


def layout_options_valid(options, required):
    """A layout's options (its top level, or one speaker's in version 2), checked as
    replay.caption_layout_file checks them."""
    keys = set(options)
    numbers = keys & set(LAYOUT_NUMBERS)
    return (
        required <= keys <= {*required, "mode", *LAYOUT_NUMBERS}
        and options.get("mode", "plan") in LAYOUT_MODES
        and all(
            isinstance(options[k], (int, float))
            and not isinstance(options[k], bool)
            and math.isfinite(options[k])
            for k in numbers
        )  # fmt: skip
        and all(options.get(k, 0) >= 0 for k in ("margin", "faceGap", "maxDrift"))
        and 0 <= options.get("minScore", 0.5) <= 1
    )


def caption_layout(stem, plan):
    """<stem>.caption-layout.json (docs/caption-plan.md "Caption position", version 2 with
    per-speaker options for the plan's speakers); None when absent."""
    p = stem + ".caption-layout.json"
    if not os.path.isfile(p):
        return None
    layout = json.loads(Path(p).read_text(encoding="utf-8"))
    speakers = layout.get("speakers", {}) if layout.get("version") == 2 else {}
    known = {speaker["id"] for speaker in plan.get("speakers", [])}
    top = {k: v for k, v in layout.items() if not (k == "speakers" and layout.get("version") == 2)}
    valid = (
        layout.get("version") in (1, 2)
        and layout_options_valid(top, {"version", "mode"})
        and isinstance(speakers, dict)
        and set(speakers) <= known
        and all(isinstance(o, dict) and layout_options_valid(o, set()) for o in speakers.values())
    )
    if not valid:
        raise ValueError("invalid caption layout")
    return layout


def title_options(layout, title):
    """A title's options: defaults, the layout's top level, then its speaker's (version 2)."""
    options = {"minScore": 0.5, "maxDrift": 150, **(layout or {})}
    return {**options, **(layout or {}).get("speakers", {}).get(title.get("speaker") or "", {})}


def lane_band(plan, speaker_id):
    """A speaker's horizontal band: lanes sorted by laneX, split at the midpoints between
    neighbouring lane centres (a midpoint belongs to the right band), the outer bands open."""
    centres = sorted((plan["width"] / 2 + s["laneX"], s["id"]) for s in plan["speakers"])
    at = [i for _, i in centres].index(speaker_id)
    low = -math.inf if at == 0 else (centres[at - 1][0] + centres[at][0]) / 2
    high = math.inf if at == len(centres) - 1 else (centres[at][0] + centres[at + 1][0]) / 2
    return low, high


def observed_tops(faces, plan, title, min_score):
    """(frame, top) of the samples a title's face position trusts, in frame order."""
    found = []
    for sample in faces["samples"]:
        if faces["version"] == 1:
            box = sample["box"]
            if sample["faceCount"] == 1 and box and box["score"] >= min_score:
                found.append((sample["frame"], box["y"]))
            continue
        boxes = sample["boxes"]
        if plan["version"] == 2 and title.get("speaker"):
            low, high = lane_band(plan, title["speaker"])
            boxes = [
                b for b in boxes if b["score"] >= min_score and low <= b["x"] + b["w"] / 2 < high
            ]
            if len(boxes) == 1:
                found.append((sample["frame"], boxes[0]["y"]))
        elif len(boxes) == 1 and boxes[0]["score"] >= min_score:
            found.append((sample["frame"], boxes[0]["y"]))
    return sorted(found, key=lambda face: face[0])


def caption_face_tops(stem, plan, layout):
    """Per title, the face top the "aboveFace" position reads, as [top, "face"|"held"] or None:
    replay.caption_layout_file's rule (person_top_with_hold over the trusted samples, held only
    from both sides within maxDrift) with that title's minScore and maxDrift; version 2 face
    tracks give a speaker's title the one face in its lane band. The viewer resolves only mode
    and offsets, so the megabytes of track stay here. None when there is no face track."""
    p = stem + ".faces.json"
    if not os.path.isfile(p):
        return None
    faces = json.loads(Path(p).read_text(encoding="utf-8"))
    if (faces.get("version") not in (1, 2) or faces.get("clock") != "delivery") or (
        faces.get("width"),
        faces.get("height"),
    ) != (plan["width"], plan["height"]):
        raise ValueError("face track is not version 1 or 2 on the plan's canvas")
    cache, tops = {}, []
    for title in plan["titles"]:
        options = title_options(layout, title)
        lane = title.get("speaker") if plan["version"] == 2 and faces["version"] == 2 else None
        key = (options["minScore"], lane)
        if key not in cache:
            trusted = observed_tops(faces, plan, title, options["minScore"])
            cache[key] = (trusted, [face[0] for face in trusted])
        trusted, frames = cache[key]
        low = bisect.bisect_left(frames, title["startFrame"])
        high = bisect.bisect_right(frames, title["endFrame"])
        if low < high:
            tops.append([min(face[1] for face in trusted[low:high]), "face"])
        elif low == 0 or high == len(trusted):
            tops.append(None)
        else:
            before, after = trusted[low - 1][1], trusted[high][1]
            drift = options["maxDrift"]
            tops.append(None if abs(before - after) > drift else [min(before, after), "held"])
    return tops


def caption_plan(transcript_path, movie_name, vid, out, issues):
    """The caption plan beside a delivery (docs/caption-plan.md) for live drawing, written as
    <out>/<id>.captions.json with its nameplate icon inlined; only a plan of this very movie."""
    p = transcript_path[: -len(Path(transcript_path).suffix)] + ".captions.json"
    if not os.path.isfile(p):
        return None
    try:
        plan = json.loads(Path(p).read_text(encoding="utf-8"))
        if plan.get("version") not in (1, 2) or plan.get("clock") != "delivery":
            raise ValueError("unsupported version or clock")
        if plan.get("movie") != movie_name or not isinstance(plan.get("burned"), bool):
            raise ValueError("not this delivery's plan")
        # The nameplate's and each version 2 speaker's icon, inlined once checked.
        owners = [plan.get("nameplate")] + [s for s in plan.get("speakers", []) if "icon" in s]
        for owner in filter(None, owners):
            icon = Path(os.path.dirname(p), owner["icon"]).read_bytes()
            if hashlib.sha256(icon).hexdigest() != owner["iconSha256"]:
                raise ValueError(f"{owner['icon']} does not have its recorded SHA-256")
            owner["iconData"] = "data:image/png;base64," + base64.b64encode(icon).decode()
        stem = p[: -len(".captions.json")]
        layout = caption_layout(stem, plan)
        tops = caption_face_tops(stem, plan, layout)
        if layout is not None:
            plan["layout"] = layout
        if tops is not None:
            plan["faceTops"] = tops
    except (OSError, ValueError, KeyError, TypeError) as e:
        issues.append(f"caption plan: {vid} ({e})")
        return None
    with open(os.path.join(out, vid + ".captions.json"), "w", encoding="utf-8") as fh:
        json.dump(plan, fh, ensure_ascii=False, separators=(",", ":"))
    return {"key": f"captions/{vid}.json", "burned": plan["burned"]}


def superseded(folder):
    out, d = {}, os.path.join(folder, "evidence", "superseded")
    if os.path.isdir(d):
        for name in sorted(os.listdir(d)):
            if name.endswith(".json"):
                try:
                    rec = json.loads(Path(d, name).read_text(encoding="utf-8"))
                    out[rec["delivery"]] = rec["size"]
                except (ValueError, KeyError, TypeError):
                    pass
    return out


def candidate_selection(folder, library):
    path = os.path.join(folder, "evidence", "candidate-selection.json")
    if not os.path.isfile(path):
        return None
    rec = json.loads(Path(path).read_text(encoding="utf-8"))
    if rec.get("version") != 1 or rec.get("status") != "unverified-candidate":
        raise ValueError("candidate-selection: bad version/status")
    movie = os.path.join(folder, rec["movie"])
    transcript = os.path.join(folder, rec["transcript"])
    target = os.path.realpath(movie)
    lib_real = os.path.realpath(library)
    if not target.startswith(lib_real + os.sep) or os.path.getsize(target) != rec["movieSize"]:
        raise ValueError("candidate-selection: movie target/size mismatch")
    digest = hashlib.sha256(Path(transcript).read_bytes()).hexdigest()
    if digest != rec["transcriptSha256"]:
        raise ValueError("candidate-selection: transcript hash mismatch")
    return target, transcript, True


def canonical_deliveries(folder, name, roots=()):
    """A delivery movie may live at the same relative path below another root
    (an offloaded movie beside a mirrored transcript)."""
    sup = superseded(folder)
    found = []
    for dirpath, dirnames, filenames in os.walk(folder):
        dirnames[:] = [
            d
            for d in dirnames
            if d not in ("work", "audio", "evidence", "transcripts", "proxy", "models")
        ]
        rel_parts = os.path.relpath(dirpath, folder).split(os.sep)
        if "delivery" not in rel_parts:
            continue
        stems = {f[:-4] for f in filenames if f.endswith((".mp4", ".srt", ".csv"))}
        for stem in sorted(stems):
            if not STAMP.match(stem) or not stem.endswith(f"_JST_{name}-review"):
                continue
            rel = os.path.relpath(os.path.join(dirpath, stem + ".mp4"), folder)
            movie = next(
                (
                    m
                    for m in [os.path.join(folder, rel)]
                    + [os.path.join(r, name, rel) for r in roots]
                    if os.path.isfile(m)
                ),
                None,
            )
            if movie is not None and sup.get(rel) == os.path.getsize(movie):
                continue
            tr = next(
                (
                    os.path.join(dirpath, stem + e)
                    for e in (".srt", ".csv")
                    if os.path.isfile(os.path.join(dirpath, stem + e))
                ),
                None,
            )
            if tr:
                found.append((movie, tr))
    return found


def offloaded_delivery(name, base, roots):
    """A delivery kept only in R2: the entry of its recording's offload.json (in any root)."""
    for root in roots:
        record = os.path.join(root, name, "offload.json")
        if not os.path.isfile(record):
            continue
        files = json.loads(Path(record).read_text(encoding="utf-8")).get("files", [])
        for item in files:
            path = item.get("localPath", "")
            if item.get("role") == "delivery" and os.path.basename(path) == base:
                return None if os.path.lexists(path) else item
    return None


def published_duration(keep, vid):
    """The duration the published catalog lists for a video (an offloaded movie is not probed)."""
    catalog = os.path.join(keep or "", "catalog.json")
    if not keep or not os.path.isfile(catalog):
        return None
    videos = json.loads(Path(catalog).read_text(encoding="utf-8"))["videos"]
    return next((v.get("duration") for v in videos if v["id"] == vid), None)


def find_original(folder, name, youtube_originals, roots=()):
    """Pre-edit source: edit-config.json `source` (size-validated), else the
    YouTube originals layout `<stamp>_youtube-<id>/<same>.{mp4,mkv}`.
    Candidate folders in a second library find their edit-config in another root."""
    for base in [folder] + [os.path.join(r, name) for r in roots]:
        cfg = os.path.join(base, "edit-config.json")
        if not os.path.isfile(cfg):
            continue
        try:
            rec = json.loads(Path(cfg).read_text(encoding="utf-8"))
            src = rec.get("source")
            if src and os.path.isfile(src) and os.path.getsize(src) == rec.get("source_size"):
                return src
        except (ValueError, KeyError, TypeError):
            pass
    if name.startswith("youtube-") and youtube_originals:
        import glob as g

        hits = g.glob(os.path.join(youtube_originals, f"*_youtube-{name[8:]}/*"))
        media = [h for h in hits if h.lower().endswith((".mp4", ".mkv"))]
        if len(media) == 1:
            return media[0]
    return None


def channel_of(name, original, issues):
    """A YouTube video's channel ID from channel.json beside its original; viewers map it to a
    label through the worker's /api/channels.json."""
    if not name.startswith("youtube-"):
        return None
    p = os.path.join(os.path.dirname(original), "channel.json") if original else None
    try:
        channel_id = json.loads(Path(p).read_text(encoding="utf-8"))["channel_id"] if p else None
    except (OSError, ValueError, KeyError, TypeError):
        channel_id = None
    if isinstance(channel_id, str) and re.fullmatch(r"UC[A-Za-z0-9_-]{22}", channel_id):
        return channel_id
    issues.append(f"no channel: {name} (default streamer)")
    return None


def duration(path):
    try:
        out = subprocess.run(
            ["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", path],
            capture_output=True,
            text=True,
            timeout=60,
        )
        return round(float(out.stdout.strip()), 3)
    except Exception:
        return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", action="append", required=True, help="library root (repeatable)")
    ap.add_argument("--out", required=True)
    ap.add_argument("--no-duration", action="store_true")
    ap.add_argument("--streamer", required=True, help="streamer tag for every video in these roots")
    ap.add_argument("--youtube-originals", default=None)
    ap.add_argument("--raw", default=None, help="downloads whose recording has no delivery yet")
    ap.add_argument("--keep", default=None, help="published catalog dir: keep entries not found")
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    videos, issues = [], []
    seen_ids = set()
    for root in a.root:
        for name in sorted(os.listdir(root)):
            folder = os.path.join(root, name)
            if not os.path.isdir(folder) or not name.startswith(("replay-", "youtube-")):
                continue
            try:
                sel = candidate_selection(folder, root)
                if sel:
                    choices, unverified = [sel[:2]], True
                else:
                    choices, unverified = canonical_deliveries(folder, name, a.root), False
                if not choices:
                    continue
                # A transcript left by a retired revision has no movie; never let it compete
                # with the one playable delivery.
                playable = [c for c in choices if c[0]]
                if len(choices) > 1 and len(playable) == 1:
                    choices = playable
                if len(choices) > 1:  # prefer the one carrying saved summaries
                    with_sum = [c for c in choices if load_summary(c[1])]
                    if len(with_sum) == 1:
                        choices = with_sum
                    else:
                        issues.append(f"ambiguous: {name} ({len(choices)} deliveries)")
                        continue
                movie, transcript = choices[0]
                base = (
                    os.path.basename(movie) if movie else os.path.basename(transcript)[:-4] + ".mp4"
                )
                vid = base[:-4]
                # R2-only movie: its data is rebuilt and published; the movie itself has no
                # localPath, so the sync never uploads it again.
                offloaded = offloaded_delivery(name, base, a.root) if movie is None else None
                playable = movie is not None or offloaded is not None
                if vid in seen_ids:
                    continue
                seen_ids.add(vid)
                cues = load_cues(transcript)
                data = {"id": vid, "cues": cues, "summary": load_summary(transcript)}
                excite = load_excitement(transcript)
                if excite:
                    data["excitement"] = excite
                speakers = load_speakers(transcript)
                if speakers:
                    data["speakers"] = speakers
                with open(os.path.join(a.out, vid + ".json"), "w", encoding="utf-8") as fh:
                    json.dump(data, fh, ensure_ascii=False, separators=(",", ":"))
                original = find_original(folder, name, a.youtube_originals, a.root)
                if not playable and not original:
                    continue  # a transcript alone has nothing to play
                channel_id = channel_of(name, original, issues)
                captions = caption_plan(transcript, base, vid, a.out, issues) if playable else None
                if a.no_duration:
                    seconds = None
                elif offloaded:
                    seconds = published_duration(a.keep, vid)
                else:
                    seconds = duration(movie or original)
                videos.append(
                    {
                        "id": vid,
                        "streamer": a.streamer,
                        "title": name
                        + (" [未検証候補]" if unverified else "")
                        + ("" if playable else " [文字起こし先行・元動画]"),
                        "broadcastStartJST": stamp_label(base),
                        "movie": base if playable else None,
                        "size": offloaded["bytes"]
                        if offloaded
                        else os.path.getsize(movie or original),
                        # MKV originals are served as a generated H.264 MP4 proxy
                        # (same basename, .mp4); the uploader creates it from localPath.
                        "original": (
                            {
                                "movie": os.path.basename(original)[:-4] + ".mp4"
                                if original.lower().endswith(".mkv")
                                else os.path.basename(original),
                                "size": os.path.getsize(original),
                                "localPath": os.path.abspath(original),
                            }
                            if original
                            else None
                        ),
                        "duration": seconds,
                        "transcriptPath": os.path.abspath(transcript),
                        "cueCount": len(cues),
                        "hasSummary": data["summary"] is not None,
                        "localPath": os.path.abspath(movie) if movie else None,
                        **({"channelId": channel_id} if channel_id else {}),
                        **({"captions": captions} if captions else {}),
                    }
                )
            except Exception as e:  # fail-closed per recording, like the native scan
                issues.append(f"unreadable: {name} ({e})")
    delivered = {v["title"].split(" ")[0] for v in videos}
    if a.keep and os.path.isfile(os.path.join(a.keep, "catalog.json")):
        # A published recording whose local files were retired stays published as it is.
        published = json.loads(Path(a.keep, "catalog.json").read_text(encoding="utf-8"))
        for video in published["videos"]:
            folder = video["title"].split(" ")[0]
            data_path = os.path.join(a.keep, video["id"] + ".json")
            raw = video["movie"] is None  # rebuilt from the downloads below
            if (
                raw
                or folder in delivered
                or video["id"] in seen_ids
                or not os.path.isfile(data_path)
            ):
                continue
            kept_data(data_path, video, a.out)
            channel_id = channel_of(folder, (video.get("original") or {}).get("localPath"), issues)
            videos.append(
                {**video, "kept": True, **({"channelId": channel_id} if channel_id else {})}
            )
            delivered.add(folder)
            issues.append(f"kept published: {folder} (local delivery not found)")
    if a.youtube_originals:
        # A YouTube original with no edit yet is published alone, like a raw TikTok download.
        for name in sorted(os.listdir(a.youtube_originals)):
            match = YOUTUBE_ORIGINAL.match(name)
            if not match or f"youtube-{match.group(1)}" in delivered:
                continue
            folder = f"youtube-{match.group(1)}"
            media = [
                os.path.join(a.youtube_originals, name, f)
                for f in os.listdir(os.path.join(a.youtube_originals, name))
                if f.lower().endswith((".mp4", ".mkv"))
            ]
            if len(media) != 1:
                issues.append(f"no single original: {name}")
                continue
            original = media[0]
            vid = name
            data_path = os.path.join(a.out, vid + ".json")
            with open(data_path, "w", encoding="utf-8") as fh:
                json.dump({"id": vid, "cues": [], "summary": None}, fh, separators=(",", ":"))
            channel_id = channel_of(folder, original, issues)
            videos.append(
                {
                    "id": vid,
                    "streamer": a.streamer,
                    "title": f"{folder} [未編集・字幕なし]",
                    "broadcastStartJST": stamp_label(name),
                    "movie": None,
                    "size": os.path.getsize(original),
                    "original": {
                        # MKV originals are served as their H.264 MP4 proxy, as above.
                        "movie": os.path.basename(original)[:-4] + ".mp4",
                        "size": os.path.getsize(original),
                        "localPath": os.path.abspath(original),
                    },
                    "duration": None if a.no_duration else duration(original),
                    "transcriptPath": os.path.abspath(data_path),
                    "cueCount": 0,
                    "hasSummary": False,
                    "localPath": None,
                    **({"channelId": channel_id} if channel_id else {}),
                }
            )
    if a.raw:
        for base in sorted(os.listdir(a.raw)):
            match = RAW.match(base)
            if not match or f"replay-{match.group(1)}" in delivered:
                continue
            movie, vid = os.path.join(a.raw, base), base[:-4]
            data_path = os.path.join(a.out, vid + ".json")
            with open(data_path, "w", encoding="utf-8") as fh:
                json.dump({"id": vid, "cues": [], "summary": None}, fh, separators=(",", ":"))
            # Only the original is published, at the key its edited delivery will reuse.
            videos.append(
                {
                    "id": vid,
                    "streamer": a.streamer,
                    "title": f"replay-{match.group(1)} [未編集・字幕なし]",
                    "broadcastStartJST": stamp_label(base),
                    "movie": None,
                    "size": os.path.getsize(movie),
                    "original": {
                        "movie": base,
                        "size": os.path.getsize(movie),
                        "localPath": os.path.abspath(movie),
                    },
                    "duration": None if a.no_duration else duration(movie),
                    "transcriptPath": os.path.abspath(data_path),
                    "cueCount": 0,
                    "hasSummary": False,
                    "localPath": None,
                }
            )
    videos.sort(key=lambda v: (v["broadcastStartJST"] or "", v["id"]), reverse=True)
    catalog = {"version": 1, "videos": videos, "issues": issues}
    with open(os.path.join(a.out, "catalog.json"), "w", encoding="utf-8") as fh:
        json.dump(catalog, fh, ensure_ascii=False, indent=1)
    total = sum(v["size"] for v in videos)
    print(f"videos={len(videos)} total={total / 2**30:.1f} GiB issues={len(issues)}")
    for i in issues:
        print("  " + i)


if __name__ == "__main__":
    sys.exit(main())
