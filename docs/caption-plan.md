# Caption plan (`<movie stem>.captions.json`, version 1)

A delivery's caption plan lets a player draw the style-20 captions live over the delivery movie,
instead of burning them in. The web viewer and the Mac TranscriptViewer both read it, and so
does the Mac app's export of a range with burned captions. `replay.caption_plan` builds it only
from a recording's pipeline evidence:

```sh
.venv/bin/python -m replay.caption_plan build \
    edits/work/recordings/<recording>/<recording> DELIVERY.mp4
```

The movie must be the one recorded by the recording's `steps/publish.json` (`delivery`) or
`steps/youtube-master.json` (`master`). The plan is written beside it with exclusive create at
mode 0600. An identical plan is kept, and one with other content is refused. A nameplate icon is
copied beside the plan as `<movie stem>.nameplate.png` and checked against its SHA-256.
`tests/fixtures/caption-plan-v1.json` is the shared synthetic fixture for consumers.

## Fields

| Field | Meaning |
| --- | --- |
| `version`, `clock` | `1` and `"delivery"`: every frame number is a frame of the delivery movie. |
| `movie` | File name of the delivery movie beside the plan. |
| `burned` | `true` when the compose requests laid every caption part's alpha over this movie at its offset. When true, players must not draw the captions again. The value always comes from evidence and is never inferred. |
| `width`, `height`, `rate`, `frames` | The delivery's canvas, integer frame rate and frame count, from ffprobe. They must match the compose plan's frames and every part's canvas. |
| `style` | The `MotionCaptionStyle` (`native/caption-render/Sources/CaptionRender/MotionCaptionStyle.swift`, shared by `caption-alpha-render`) of the first part, with exactly its 14 fields. |
| `titles` | `MotionCaptionTitle`s in display order. Each has `text`, `startFrame`, `endFrame`, `x`, `y` and `graphemeFrames`, plus optional `style`, `alignment` (`"left"`) and `speaker` (a name or `null`). |
| `nameplate` | `null`, or the badge `replay.caption_layout.badge_image` draws, described below. |

Each title's rules are the ones `MotionCaptionTitle.validate()` checks:

- A title shows over `[startFrame, endFrame)`.
- Grapheme `i` is visible from `graphemeFrames[i]` onward, so on frame `f` the title shows `count(graphemeFrames <= f)` graphemes. A carried title, whose first words continue the previous title, has those onsets equal to `startFrame`.
- There is one onset per extended grapheme cluster, the same count as Swift's `String.count`. Line breaks (`\n`) count.
- Onsets are non-decreasing and lie inside the span.
- Titles never overlap. Touching (`end == next start`) is allowed.
- `x` and `y` follow Motion's convention: the first line's baseline centre (its start when left-aligned), with the origin at the canvas centre and y pointing up. Lines advance by the font's ascent plus descent.
- A title's own `style` replaces the plan's `style`. The builder sets it when that title's part had a different style.

## Nameplate

The nameplate is a round `icon`, `iconSize` pixels with its top-left corner at (`iconX`, `iconY`) in pixels from the top-left of the canvas:

- The icon has a white ring `ringWidth` (`ringColour`) pixels wide inside the disc.
- Below it, `name` is drawn in `nameFont` at variation `nameFontVariation`, size `nameSize` px, in `nameColour`.
- The name has a `nameStrokeWidth` px stroke in `nameStrokeColour`.
- The name is anchored `mt` (middle-top) at (`nameX`, `nameY`).
- `iconSha256` and `nameFontSha256` pin the inputs.
- `burned` says whether compose laid the badge over the movie.
- The font file itself is not copied beside the plan.

## Backfill

Older deliveries can be backfilled:

- **Recordings that still hold their finished `.moti` parts** are built directly.
- **Recordings that kept only `caption-parts-v1/plans-w*` and `speech-onsets-v1.json`** need their parts staged again first, in scratch. Use `replay.motion_reveal_stages` followed by `replay.motion_finish`, exactly as `replay.caption_parts.finished_part` does.

## Viewers

The web viewer and the Mac TranscriptViewer draw the plan over the video with the
shared style-20 renderer (`native/caption-render`, product `CaptionRender`, and its
web port). Captions start off when `burned` is true and on otherwise; the viewer
also offers the nameplate, the gradient (off = the top colour alone) and the two
colours. The Mac viewer exports ordered ranges with captions burned through the
same `CaptionRender` export API as `caption-burn`.

## Burning captions on demand

The pipeline no longer burns captions by default (user instruction, 2026-11-03).
An agent that needs a captioned movie for one video decides on its own and runs
`caption-burn`:

Build (system frameworks only, no network; scratch stays in the checkout):

```bash
mkdir -p edits/work/caption-render/tmp
TMPDIR="$PWD/edits/work/caption-render/tmp" swift build -c release \
  --package-path native/caption-render --scratch-path edits/work/caption-render/build
# binary: edits/work/caption-render/build/release/caption-burn
```

Usage:

```
caption-burn MOVIE PLAN OUTPUT.mp4 [--range START:END]... [--no-nameplate]
  [--nameplate-font FONT] [--no-gradient] [--top #RRGGBB] [--bottom #RRGGBB]
  [--codec h264|hevc] [--over-burned] [--layout FILE]
  [--mode plan|aboveFace|bottom|top|center] [--offset-x PX] [--offset-y PX]
  [--speaker ID]... [--hide-speaker ID]...
```

- `MOVIE`: the delivery the plan describes. Its canvas, frame rate and frame
  count must equal the plan's, or the tool refuses it.
- `PLAN`: `<movie stem>.captions.json`. A nameplate icon is read from the same
  directory as the plan.
- `OUTPUT.mp4`: a new absolute or relative `.mp4` path in an existing
  directory. It must not exist.
- `--range START:END`: seconds on the delivery clock, `[START, END)`, rounded to
  the nearest frame. Repeat it for more ranges. Ranges are joined in exactly the
  order given: any order, and they may overlap (the same footage then appears
  more than once). Each frame shows the captions of its own delivery frame.
  With no `--range`, the whole movie is burned.
- `--no-nameplate`: leave out the plan's nameplate and every speaker icon of a version 2
  plan (see "Speaker icons"). If the plan has a nameplate that is not burned yet, it is
  drawn under the captions.
- The nameplate font is found automatically. The tool looks for the file named
  in `nameFont`, with SHA-256 `nameFontSha256`, in these folders in order:
  beside the plan, `/Library/Fonts`, `/System/Library/Fonts` and
  `~/Library/Fonts`. These are the pipeline's font folders; the pipeline's
  `NotoSansJP-VariableFont_wght.ttf` is in `~/Library/Fonts`. A file with the
  right name but a different SHA-256 is skipped.
- `--nameplate-font FONT`: use this file instead of searching. Its SHA-256 must
  still match.
- `--no-gradient`: a single fill in the top colour.
- `--top` and `--bottom`: replace the gradient colours. The plan's colours are
  used otherwise.
- `--codec`: `h264` (default) or `hevc`. Video is tagged BT.709, at about
  9 Mbit/s for 1080p30. Audio is the movie's own audio, re-encoded as 48 kHz
  stereo AAC. The `moov` atom is at the front, so QuickTime opens the file.
- Position (see "Caption position" below): captions are placed by
  `--layout FILE`. Without it, the movie's own `<movie stem>.caption-layout.json`
  is used. If that is missing too, captions stay where the plan puts them.
  `--mode`, `--offset-x` and `--offset-y` replace those values of the layout and
  keep its other options. `aboveFace` reads the movie's `<movie stem>.faces.json`
  and fails if it is missing. The rule is the shared one, so the Mac viewer, the
  web viewer and `caption-burn` place every title the same way.
- `--speaker ID` (repeatable) keeps only those speakers' titles.
  `--hide-speaker ID` (repeatable) drops a speaker's titles. `unassigned` names the titles
  that have no speaker. Positions are resolved first, so the titles that stay never move.
  These options fail on an id the plan does not have. In a version 2 plan, each speaker is
  drawn in its own colours: `--no-gradient` gives each speaker its top colour, and
  `--top`/`--bottom` recolour only the unassigned titles. See "Multiple speakers" below.
- `--over-burned`: allow a plan with `burned: true`. That movie already has its
  captions, so they would appear twice. Use it only for comparisons.

Examples:

```bash
B=edits/work/caption-render/build/release/caption-burn
D=/path/to/delivery/2026-11-02_18-30-45_JST_replay-123-review
# whole movie
"$B" "$D.mp4" "$D.captions.json" edits/work/caption-render/out/whole.mp4
# two ranges, single yellow fill, no nameplate
"$B" "$D.mp4" "$D.captions.json" edits/work/caption-render/out/two.mp4 \
  --range 177:181 --range 1200:1203 --no-nameplate --no-gradient --top '#FFD400'
```

On success it exits 0 and prints one JSON report to stdout:

```json
{"duration":7,"frames":210,"output":"/…/two.mp4","ranges":[
 {"end":181,"endFrame":5430,"outputFrame":0,"start":177,"startFrame":5310},
 {"end":1203,"endFrame":36090,"outputFrame":120,"start":1200,"startFrame":36000}]}
```

`frames` is the number of video frames written. `duration` is `frames / rate`
seconds. Each range lists its delivery frames `[startFrame, endFrame)` and the
output frame it starts at.

On failure it exits 1, prints one line to stderr and leaves no output or
staging file:

- `Invalid request: …` means the inputs are wrong. Examples: bad arguments, a
  plan or movie that does not match, an empty range or one outside the movie,
  an existing output, a nameplate font that is not found or has the wrong
  SHA-256, a burned plan without `--over-burned`, or a file
  whose SHA-256 does not match. Fix the inputs; retrying unchanged fails again.
- `Unavailable: …` means a system step failed: reading, encoding or verifying.
  Check free space and the movie before you retry once.

The Mac viewer and other Swift code call the same export directly:
`CaptionExport.export(movie:plan:ranges:fill:nameplate:output:codec:allowBurned:progress:)`
in `native/caption-render`. It is async and can be cancelled.

Rules:

- Burn only into a movie whose plan says `burned: false`.
- Measure free space on the output volume first (AGENTS.md disk policy).
- Write to a new path; the tool never overwrites the delivery, stages,
  verifies and renames.
- Check the JSON report (frames, ranges, duration) and play or decode the
  result before reporting it.
- Record why the captioned copy was made beside it.

## Pipeline status

Until the pipeline switch lands, `recording_caption_steps` still renders caption
alpha and composes it, and `edit_completion` checks that burned path. The switch
waits for runs pinned to the current caption code to finish; it then writes the
plan instead of composing alpha, and the dynamic-captions gate checks the plan
against the transcript chain.

### The prepared switch (`edits/work/caption-plan/pipeline-switch.patch`, not applied)

The patch applies to the current working tree with `git apply --check`. It changes
`recording_caption_steps`, `recording_pipeline`, `pipeline_completion`,
`recording_retirement`, `vertical_excerpt` and `youtube_master`, together with their tests.

- **Removed:** the `caption-render` step. That includes its Compressor and native alpha render,
  its in-flight override, its gradient audit, and its `compressor-leases` slot.
  `youtube-encode` keeps that lease.
- **compose:** composes the cleaned source with an empty part list
  (`compose-v1/uncaptioned-parts.json`). It lays no alpha and draws no nameplate badge; a
  layout still gives its canvas. compose now depends on `caption-parts` and `cleanup`.
- **New `caption-plan` step:** depends on `publish`, `caption-parts` and `yolo-track`. It
  writes `<stem>.captions.json` (`burned: false`), the nameplate icon and `<stem>.faces.json`
  beside the published delivery, once. `summarize` (the completion gate) and `youtube-master`
  wait for it.
- **Retirement:** the `.moti` parts are kept until `caption-plan` passes, because they are its
  input.
- **`dynamic_captions` gate (`pipeline_completion.caption_failure`):**
  - Every caption must be placed, as before.
  - The plan beside the movie must validate and name this movie and its frame count.
  - It must show every displayed caption chain the SRT was written from, in order:
    `caption_plan.chain_failure` allows 1 frame of tolerance, and a title may grow over several
    chains.
  - The movie must still decode cleanly.
  - A delivery whose evidence pins a `caption-render` receipt and has no plan (burned before
    the switch) still passes on its rendered parts.
  - `publish` pins `caption-render` only when that receipt exists.
  - On the real data, `chain_failure` passes for all 8 deliveries that have plans.
- **Unchanged:** the `source_caption_mask`, `bgm_removal`, `silence_cut` and `transcription`
  checks.
- **Not changed:** `edit_completion` and `edit_config`. Their burned-caption checks belong to
  the older delivery backend, not to pipeline deliveries.

Apply only when all of these hold:

1. No `replay.recording_pipeline run` and no vertical-excerpt run is live. Check with `pgrep -fl
   "recording_pipeline run"`; the runners import these modules.
2. The pinned code is rebased. Repin any receipt or snapshot that hashes these modules
   (`compressor_repin` / the pinned-closure procedure in `repo-jobs`). Run `make check`.
3. Recordings mid-run are finished on the old code first. A recording that has a
   `caption-render` receipt and is re-run sees its compose receipt go stale, because its
   inputs changed. That is a redo, so it needs its own decision.

Effects to expect after applying:

- **youtube-master deletes the published pre-encode delivery.** Its caption plan, face track,
  icon and `caption-layout.json` are copied beside the master *before* the master's completion
  check. The master has the delivery's frames, so the plan holds unchanged. The copy beside the
  deleted delivery goes with it.
- **Vertical excerpts lose their captions.** They run `caption-parts` and `compose` on the
  vertical sibling, so the picture is uncaptioned, and no plan exists for the excerpt's own
  clock. Captions for excerpts need either a plan of the excerpt (offset by the range start)
  or a caption-burn of the range.
- **Burned copies are made on demand.** A burned copy of a new delivery is made only with
  caption-burn.
- **The web-viewer sync picks up the new siblings.** Each new delivery's `.captions.json`,
  `.faces.json` and nameplate PNG are `<stem>.*` siblings, so the next sync uploads them as data.

## Caption position

Live-drawn captions can be moved per delivery. Two more siblings of the delivery movie carry
what is needed. The web viewer, the Mac TranscriptViewer and caption-burn all resolve them
with the same rule. `tests/fixtures/caption-layout-expected-v1.json` gives the expected
results for `caption-plan-v1.json` + `caption-faces-v1.json` under ten layouts, with a
tolerance of 1e-6.

### `<stem>.faces.json` (`replay.caption_faces`, written once, never overwritten)

```json
{"version": 1, "clock": "delivery", "rate": 30, "width": 1080, "height": 1920,
 "source": {"model": "<weights>.pt", "modelSha256": "…", "file": "yolo-track-v2.json", "sha256": "…"},
 "samples": [{"frame": 0, "faceCount": 1, "box": {"x": 704.6, "y": 700.17, "w": 372.48, "h": 459.04, "score": 0.2519}},
             {"frame": 4, "faceCount": 0, "box": null}]}
```

- The YOLO track samples every second decoded source frame. It records only the
  highest-scoring face, plus `faceCount`, the number of faces above the 0.25 floor.
- `faceCount` is authoritative. A `faceCount: 2` sample still carries one box.
- `box: null` means the frame was observed and showed no face. A frame without a sample was
  never observed.
- Boxes are in canvas pixels, with the origin at the top left and y pointing down.
- Each decoded frame's first slot comes from `frame_slots.first_slots` over `video-clock.json`
  (the compose's rule). That slot maps through the pinned cut plan to the delivery frame.
  Samples in removed footage are dropped. No sample is thinned or moved.
- Inputs are pinned: the `yolo-track` and `cut-plan` receipts, the source sha, and the delivery
  recorded by `publish` or `youtube-master` with the cut plan's length and rate.

### `<stem>.caption-layout.json` (user / agent / viewer edited, atomic replace)

```json
{"version": 1, "mode": "aboveFace", "offsetY": -40}
```

`mode` is one of `plan`, `aboveFace`, `bottom`, `top` or `center`. The other keys are
optional; the default for each is in brackets:

- `offsetX` [0]: canvas pixels, +x to the right
- `offsetY` [0]: canvas pixels, +y down
- `margin` [120]
- `faceGap` [60]
- `minScore` [0.5]
- `maxDrift` [150]

A missing file means `{"version": 1, "mode": "plan"}`. These defaults are the burned
placement's own `regionTop`, `clearance` and YOLO rules.

### Resolution (per title, in this order, IEEE doubles)

```
H, W     = plan.height, plan.width
size     = (title.style ?? plan.style).fontSize
lines    = count("\n" in title.text) + 1
baseline = H/2 - title.y                        # first baseline, from the top
top      = baseline - 24 - 1.16*size            # box top: padding + Noto Sans JP ascender
box      = (150/82.95580110497238)*size + (lines-1)*1.448*size
mode plan:      placed = top                                  basis "plan"
mode top:       placed = margin                               basis "mode"
mode bottom:    placed = H - margin - box                     basis "mode"
mode center:    placed = (H - box) / 2                        basis "mode"
mode aboveFace:
  faces  = samples with faceCount == 1 and box.score >= minScore (time = frame)
  inside = faces with startFrame <= frame <= endFrame          (both ends inclusive)
  if inside:  faceTop = min(box.y of inside)                  basis "face"
  else if a face exists both before startFrame and after endFrame, nearest b and a,
          and |b.y - a.y| <= maxDrift: faceTop = min(b.y, a.y) basis "held"
  else:   placed = top                                         basis "plan"
  placed = max(margin, min(H - margin, faceTop - faceGap) - box)
placed = max(0, min(placed + offsetY, H - box))   # inside the canvas; the top wins
x      = max(-W/2, min(title.x + offsetX, W/2))   # anchor only: widths are not measured
y      = H/2 - (placed + 24 + 1.16*size)          # Motion convention again
```

- The constants come from the style-20 calibration: a 24 px padding, a native single-line
  measurement of 150 px at 82.9558 pt, and Noto Sans JP's hhea ascender of 1.16 em. The
  calibration's 120.12 px line advance is 1.448 em.
- Every title is one revealing run, because `motion_reveal_stages` has already merged its
  growing pages into it. So each title gets one y and does not flicker while it reveals, as
  in `caption_fixed_y`.
- On replay-7703504128194611969:
  - `top` reproduces the 1389 region-top titles within 0.0003 px.
  - `aboveFace` reproduces 60 of 65 titles that see a face, and 690 of 986 held titles, within
    1 px.
  - The burned placement read the face over the title's source range, removed footage
    included. This rule reads the delivery window.
- The horizontal clamp keeps only the anchor inside the canvas. A wide line can still cross an
  edge after a large `offsetX`.

### Agent CLI

```sh
.venv/bin/python -m replay.caption_position faces build WORK-DIR MOVIE   # once per delivery
.venv/bin/python -m replay.caption_position show MOVIE                   # with defaults
.venv/bin/python -m replay.caption_position set MOVIE --mode aboveFace --offset-y -40
.venv/bin/python -m replay.caption_position set MOVIE --mode bottom --margin 160
.venv/bin/python -m replay.caption_position resolve MOVIE [--titles]     # basis counts
```

`set` keeps every option it is not given. Each new or changed sibling is a web-viewer data
change on the next sync.

## Multiple speakers

Multiple speakers use the same plan, the same Viewer drawing and the same `caption-burn` path
as one speaker. Version 1 and single-speaker behaviour do not change. The fixtures are:

- `tests/fixtures/caption-plan-v2.json`
- `caption-layout-v2.json`
- `caption-faces-v2.json`
- `caption-layout-expected-v2.json`: expected results with a tolerance of 1e-6. Each case
  has `name`, `layout`, `faces` (the face-track fixture it uses, version 1 or 2) and `titles`
  (`x`, `y` and `basis` per plan title, in plan order). Its top-level `icons` lists the
  expected speaker icon squares (see "Speaker icons").
- `synthetic-multi.speaker-left.png` and `synthetic-multi.speaker-right.png`: the icons of
  the fixture plan's `left` and `right` speakers. `middle` has no icon.

`replay.caption_plan`, `CaptionRender` and the web viewer each check their parity against
these fixtures.

### Decisions (2026-11-03, the agent's recommendations, autonomy granted by the user)

1. **Plan version.** Multiple speakers use plan version 2 with a `speakers` table. Version 1
   is untouched.
2. **Colours.** Each speaker keeps today's lane colour as `colorBottom`: purple `#C48CFF`,
   green `#7CDC8C`, light blue `#6CD4FF`. `colorTop` is that colour mixed halfway to white.
   Each channel is `(c + 255 + 1) // 2`, the rule that made style 20's `#97D8F7` from
   `#2FB0EE`.
3. **Size.** The whole style 20 is scaled down so that a lane holds about 12 characters. The
   font, the outlines and the shadow scale together.
4. **Position.** Fixed lanes at the bottom. The builder writes them as each title's planned
   `x`/`y`. Layouts can move them per speaker.
5. **Unassigned speech.** A title with no speaker is grey, centred, and stacked when it
   overlaps another one in time. It never gets an identity.
6. **Names.** No names are burned in. Speakers are told apart by colour. The viewers show
   chips.
7. **`<transcript>.speakers.json`.** It stays, for web compatibility. It is derived from the
   plan's `speakers` as `{"speakers": {id: {"label": name, "color": colorBottom}}}`. It will
   be retired later.

### Plan version 2

Version 2 is version 1 with `"version": 2` and one more required key, `speakers`. It is a
list in lane order, left to right:

```json
{"id": "left", "name": "…", "colorTop": "#E2C6FF", "colorBottom": "#C48CFF",
 "laneX": -310, "icon": "<stem>.speaker-left.png", "iconSha256": "…"}
```

- `id`:
  - A non-empty string, unique within the plan.
  - It may not be `unassigned`, which is the name used for titles without a speaker.
- `name`: a non-empty display name.
- `colorTop` and `colorBottom`: `#RRGGBB`.
- `laneX`: the lane's Motion anchor x, finite, with the origin at the canvas centre.
- `icon` and `iconSha256`: optional, and given together or not at all.
- Version 2 needs at least two speakers. One speaker is a version 1 plan.

Titles keep the version 1 keys:

- `speaker` is one of the `id`s or absent/null. Absent or null means unassigned.
- `startFrame` never decreases along `titles`.
- Titles of one speaker never overlap.
- Titles of different speakers may overlap.
- Unassigned titles may overlap each other. They are stacked (step 4 below).
- The `style` colours are the unassigned colours: grey `#E4E4E4` over `#C8C8C8`.

The builder writes each title's lane base position unstacked:

- `x` is the speaker's `laneX` (0 for unassigned).
- `y` is the bottom lane's baseline.

Every reader resolves positions with the rule below, including the stacking. A version 2
plan is therefore always resolved, even when it has no layout file.

### Layout version 2 and faces version 2

**Layout version 2** is version 1 plus an optional `"speakers": {id: OPTIONS}`.

- `OPTIONS` holds any of `mode`, `offsetX`, `offsetY`, `margin`, `faceGap`, `minScore` and
  `maxDrift`.
- For that speaker's titles, these replace the top-level values, key by key.
- Unassigned titles use the top-level values.
- Every `id` must be in the plan's `speakers`. A version 1 plan therefore accepts only an
  empty map.
- A version 1 layout is still valid for a version 2 plan, and it applies to every title.

**Faces version 2** records every face of each observed sample, at or above the YOLO 0.25
floor:

```json
{"version": 2, "clock": "delivery", "rate": 30, "width": 1920, "height": 1080, "source": {…},
 "samples": [{"frame": 20, "boxes": [{"x": 550, "y": 500, "w": 200, "h": 300, "score": 0.9}]}]}
```

- Sample keys are exactly `frame` and `boxes`.
- Box keys are as in version 1.
- `boxes` may be empty: the frame was observed and showed no face.
- Frames strictly increase.

### Resolution with speakers (extends "Resolution" above)

1. **Options.** Each title's options are the layout's top-level values, with its speaker's
   entry laid over them. Defaults apply as in version 1.
2. **Face observations** in `aboveFace` mode, as `(frame, top)` pairs:
   - **Version 1 faces**: the version 1 rule for every title. That is, `faceCount == 1` and
     `score >= minScore`.
   - **Version 2 faces, title with a speaker `s`**:
     - Lane centres are `cx = W/2 + laneX`.
     - The speakers sorted by `laneX` split the canvas into bands at the midpoints between
       neighbouring centres. The outer bands are open-ended. A midpoint belongs to the right
       band.
     - In a sample, the qualifying boxes are those with `score >= minScore`, using `s`'s
       `minScore`.
     - If exactly one qualifying box has its centre `x + w/2` in `s`'s band, the observation
       is that box's `y`.
     - Zero such boxes, or two or more, give no observation. Two or more is ambiguous.
   - **Version 2 faces, unassigned title or a version 1 plan**: the sample must have exactly
     one box, and that box must have `score >= minScore`. This is version 1's
     `faceCount == 1` rule.
   - From there `inside`, `held`, `face` and `plan` work as in version 1.
3. **Each title's `placed`** is computed with its options, including `offsetY` and the canvas
   clamp, as in version 1.
4. **Stacking.** This step applies to unassigned titles in a version 2 plan only. Take them in
   plan order:
   - `base = placed + box` and `height = box`.
   - `active` is the unassigned titles already stacked whose `endFrame > startFrame`.
   - The candidates are `{base} ∪ {other.top − 12 for other in active}`, sorted descending.
   - The chosen `bottom` is the first candidate `c` that, for every `other` in `active`,
     meets `c <= other.top − 12` or `c − height >= other.bottom + 12`. If no candidate
     meets that, the last candidate is used.
   - Then `placed = bottom − box`, with no clamp afterwards. This is
     `speaker_subtitles._LaneStack`.
   - The title's `basis` is unchanged.
5. **Result.** `y = H/2 − (placed + 24 + 1.16·size)`, and
   `x = clamp(title.x + offsetX, −W/2, W/2)`.

### Speaker icons

A speaker with `icon` in a version 2 plan shows that icon. Decision 6 still holds: no name
is drawn.

- **Shape.** The icon is scaled to fill a disc inside a white `#FFFFFF` ring, drawn the same
  way as the nameplate's icon.
- **Size and place.** All values are integers in canvas pixels, with the origin at the top
  left. `floor(v + 0.5)` is written `round(v)` below.
  - `size = round(0.08·H)` and `ring = max(2, round(size·4/150))`.
  - `gap = round(0.02·H)`.
  - `x = clamp(round(W/2 + laneX − size/2), 0, W − size)` and `y = H − gap − size`.
  - At 1920x1080 this gives an 86 px icon with a 2 px ring, 22 px above the bottom edge,
    centred on the lane. It sits below the bottom-lane captions, whose box ends `margin`
    (120) above the edge.
- **When.** The icon shows for the whole movie, like the nameplate. It does not follow the
  layout: icons stay at the bottom edge whatever `mode` the captions use.
- **Hiding.**
  - A hidden speaker (`--hide-speaker`, or not among `--speaker`) has its icon hidden too.
  - `--no-nameplate` turns off the nameplate and every speaker icon at once.
  - The Viewer switch does the same.
- **Burned.** Speaker icons are never part of a delivery: the multi-speaker ASS burn draws
  none. So viewers always draw them when they are on, whatever `burned` says. `caption-burn`
  draws them unless `--no-nameplate` is given.
- **Order.** The icons are drawn under the captions, after the nameplate.
- **Files.** Each icon file sits beside the plan and is refused unless its SHA-256 matches
  `iconSha256`.
- **In Swift:**
  - `CaptionSpeakerIcons.placements(plan)` returns the squares.
  - `CaptionSpeakerIcons.bitmap(plan, icons:showing:)` draws the full-canvas layer.
  - `CaptionUnderlay.bitmap(nameplate:icons:…)` combines the nameplate and the icons.
  - `CaptionOverlay(…, underlay:)` takes the combined layer, and `CaptionExport.export` takes
    it through its `nameplate` parameter.

### Viewer, caption-burn and colours

- **Colours.** Each speaker's titles are drawn with
  `CaptionFill(top: colorTop, bottom: colorBottom, gradient: true)`. Unassigned titles use the
  plan's style.
  - `--no-gradient` gives each speaker a single fill in its `colorTop`.
  - `--top` and `--bottom` recolour only the unassigned titles, or every title of a
    version 1 plan.
  - In Swift, `CaptionOverlay` and `CaptionExport` take `speakerFills`. A speaker missing
    from it uses the plan's colours.
- **Show or hide speakers.**
  - `caption-burn --speaker ID` is repeatable and keeps only those speakers.
  - `--hide-speaker ID` is repeatable and drops a speaker.
  - `unassigned` names the titles that have no speaker.
  - In Swift, the call is `plan.showing(only:hiding:)`.
  - Positions are resolved with every title present, then the hidden titles are dropped. A
    shown title therefore never moves when another is hidden.
- **`caption-burn`** reads version 1 and version 2 plans, layouts and face tracks.

### Input contract for the version 2 plan builder (`replay.caption_plan`, Python)

The builder writes version 2 only when at least two speaker ids occur. Otherwise it writes
version 1. Its inputs are:

1. **The validated speaker cue CSV** (`replay.speaker_subtitles.read_cues`). Each title gets
   its cue's `speaker`. `unassigned` becomes no speaker.
2. **Reveal stages per cue** (`replay.motion_reveal_stages`), on the delivery clock.
   Different speakers are never merged.
3. **The speaker table.** It comes from the multi-speaker job config: `speakers: [{id, name,
   color?, icon?, iconSha256?}]` in lane order.
   - `colorBottom` is the given `color`, or the lane fill.
   - `colorTop` is computed by decision 2.
4. **Lane geometry.**
   - `laneX` is `650 − 960`, `1140 − 960` and `1650 − 960` at 1920 px, scaled to the canvas
     width.
   - The lane base is the bottom lane: `mode bottom` with the scaled style's margin. The
     builder writes it as `y`.
5. **Style.** Style 20 scaled by decision 3, with the unassigned grey stops.
6. **The canvas, rate, frames and `burned`**, as in version 1.
7. **`.speakers.json`** is derived by decision 7.

### The version 2 builder as implemented (`replay.speaker_caption_plan`)

```sh
.venv/bin/python -m replay.speaker_caption_plan build CUES.csv MOVIE (--burned | --unburned) \
    [--speakers SPEAKERS.json] [--ffprobe FFPROBE]
```

- **Reveal.**
  - The multi-speaker cues already grow a text stage by stage, as in こん → こんば → こんばんは.
  - A speaker's cue that starts exactly where the previous cue of that speaker ends, and leads
    with that cue's whole text, extends the same title.
  - Each grapheme appears on the first frame of the stage that shows it.
  - A cue shows from frame `ceil(start·rate)` up to, but not including, frame `ceil(end·rate)`,
    as libass shows it.
  - A title that no frame shows is left out, and the CLI reports how many were left out.
- **Text.** Titles are wrapped by `speaker_subtitles.wrap_text`, the ASS burn's wrapping.
- **Lane base.**
  - Each title's `y` is the resolver's `bottom` position with the default 120 px margin, so a
    plan without a layout looks like `mode: bottom`.
  - "The scaled style's margin" in the contract above is read as this default.
  - At 1080 lines `y` is −418.07, as in the fixture case `bottom-all`.
- **Lane style.** The lane style is the fixture's style exactly: `caption_native_request.style`
  at 40 pt for 1920 px, with the style-20 stroke and shadow scales multiplied by
  40/82.9558.
- **Speaker table.**
  - Without `--speakers`, the lanes are named 左, 中央 and 右 (`speaker_subtitles.SPEAKER_NAMES`).
  - `SPEAKERS.json` gives `{"speakers": [{"id", "name", "color"?, "icon"?, "iconSha256"?}]}`.
    The icon is an absolute path. It is pinned beside the plan as `<stem>.speaker-<id>.png`
    after its SHA-256 is checked.
- **Speaker count.** Fewer than two speakers is refused. That case is a single-speaker plan.
  The contract says to write version 1, but that would need the single-speaker pipeline's own
  placement, and its titles may overlap, which version 1 forbids.
- **Outputs.** The plan, the icons and `<stem>.srt.speakers.json` are each written once and
  never overwritten.

`edits/work/caption-plan/multispeaker-switch.patch` is not applied yet. It goes in with
`pipeline-switch.patch`, on the same go-ahead. The order is `pipeline-switch`, then
`multispeaker-switch`, then `faces-v2`.

- It changes `youtube_render.render` and `youtube_subtitles.render_subtitles`. When the cues
  have two or more speakers, these no longer write or burn the ASS. They write the version 2
  plan and `.srt.speakers.json` beside the movie, with `burned: false`.
- Single-speaker cues keep their ASS burn.

**`aboveFace` works only for a video that has a version 2 face track, and none has one yet.**

- The YOLO tracks made so far (`yolo-face-caption-track-v2`) keep only the highest-scoring
  face of each frame and `faceCount`, so `caption_faces` builds version 1 face tracks from
  them.
- The multi-speaker YouTube flow runs no YOLO. Adding it was left out on purpose
  (2026-11-03).
- On these videos, `aboveFace` for a speaker finds no face and falls back to the plan's lane.

`edits/work/caption-plan/faces-v2.patch` is not applied yet. Apply it after
`multispeaker-switch.patch`, because it builds on that patch's `youtube_render`.

- `yolo_track` writes protocol `yolo-face-caption-track-v3`. A v3 track keeps every v2 field
  (`face`, `faceCount`) and adds `faces`: every face box at or above the 0.25 floor, as
  `[x1, y1, x2, y2, score]`, highest score first.
- The readers (`caption_fixed_y_job.load_yolo` and `caption_faces`) accept v2 and v3.
  `caption_faces build` writes a version 2 face track, with every face's `boxes`, from a v3
  track, and a version 1 face track from a v2 track.
- Existing recordings are not run again. A video that needs speaker-aware `aboveFace` gets its
  own `yolo-track` run once the patch is in.
- The output file keeps its name, `yolo-track-v2.json`, so the steps that find it by path need
  no change.
- A render config may give an optional `speakers` table:
  `[{"id", "name", "color"?, "icon"?, "iconSha256"?}]`, with the icon as an absolute path. It is
  checked when the config is loaded. Its names, colours and icons go into the version 2 plan
  and `.srt.speakers.json`. Without it, the lanes are named 左, 中央 and 右.
