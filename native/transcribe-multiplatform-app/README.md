# Transcribe Captions

Drop a video; subtitle files for DaVinci Resolve 21 appear next to it, coloured per speaker and outlined (袋文字).
A desktop app for macOS (Apple Silicon) and Windows (x64). Download it from the
[GitHub releases](https://github.com/kkkaoru/my-video-editor-tools/releases) tagged `transcribe-app-v*`:

| Your machine | Download | Transcription | Speakers |
| --- | --- | --- | --- |
| Mac (Apple Silicon) | `Transcribe-Captions-macos-arm64.zip` | Metal | Metal |
| Windows with a GeForce | `Transcribe-Captions-windows-geforce-x64-setup.exe` | Vulkan | CUDA once its runtime is downloaded in the app, the CPU until then |
| Windows with a Radeon | `Transcribe-Captions-windows-radeon-x64-setup.exe` | Vulkan | Vulkan |
| Windows with neither | Either Windows download | CPU | CPU |

Each Windows download uses only its own vendor's GPU: the GeForce build never uses a Radeon, nor the reverse. Without
that GPU it runs on the CPU, and the Transcribe tab says so because it is slower. The right build needs an up-to-date GPU
driver, which provides Vulkan. If the Vulkan build of `whisper-cli` fails, the job goes on with the CPU build, which picks
the fastest code for the processor when it starts.

The builds are not notarized or code-signed with a paid certificate: on macOS open the app, then choose System Settings >
Privacy & Security > Open Anyway; on Windows choose "More info > Run anyway" in SmartScreen.

## What it writes

Choose the format in the Transcribe tab; FCPXML is the default. Each run writes that one subtitle file plus the
`.transcript.json`.

| File | Use in Resolve |
| --- | --- |
| `<video>.ttml` | Media Pool > Import Subtitle. Holds per-span fill, `tts:textOutline` and position. |
| `<video>.srt` | Plain subtitle track, for editing text and timing. |
| `<video>.fcpxml` | File > Import > Timeline. The video plus one title per caption. |
| `<video>.transcript.json` | Source of truth; "write again" lays captions out from it without running the models. |

Existing files are never overwritten: a second run writes `<video>-2.*`. Which attributes Resolve keeps per format is
recorded in [the verification notes](#verification).

## Models

Models are managed in the app (download, cancel, resume, delete), and anything already on disk is reused: the app's own
folder, the Hugging Face cache, and any folder you add. Downloads are pinned to a revision and SHA-256.

| Purpose | Model | Notes |
| --- | --- | --- |
| Transcription | [openai/whisper-large-v3-turbo](https://huggingface.co/openai/whisper-large-v3-turbo) | Same weights everywhere. Converted once to whisper.cpp's ggml format. A local MLX copy converts too. |
| Speakers | [Nemotron 3.5 Transcribe 30B-A4B](https://huggingface.co/nvidia/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B) | NVIDIA publishes safetensors only; GGUF builds come from AtomicChat. Chosen per machine: GeForce ≥ 24 GB IQ4_NL (Blackwell NVFP4), 16 GB GeForce Q2_0_M, Radeon ≥ 48 GB Q8_0, Radeon ≥ 24 GB IQ4_NL, Macs and CPU-only PCs by system memory: 64 GB Q8_0, 32 GB IQ4_NL, 24 GB Q2_0_L. |
| Speakers (small) | [Nemotron 3 Diarization](https://huggingface.co/nvidia/Nemotron-3-Diarization) | 107 MB, runs anywhere. Used when the large model does not fit. |
| Faces | YuNet (bundled), `avatar-face-caption-v2.onnx` | The same model and engine (`tract`) on both systems. Put the avatar ONNX in a model folder to use it. |

In the GeForce build, on a PC with a GeForce, the model manager offers the CUDA runtime download (509 MB) for speaker
separation; no other build or machine lists it. NVFP4 and Q2_0_M run only on CUDA. Nemotron speaker models reach eight
speakers at most, so the palette has eight editable slots at most; it starts with eight and can be shortened.

Models can be downloaded together, each with its own progress bar. Resetting deletes only what the app itself downloaded,
after a confirmation; folders you added, anything overlapping them and the Hugging Face cache are never touched, and it
is refused while a job or a download runs. Videos are refused with a message naming what is missing until the models
for transcription and, unless one speaker is set, speaker separation are present.

## Caption position and size

Above or below the face, or the top or bottom of the video, each with horizontal and vertical fine-tuning in percent of
the frame. Safe zones (5% on each side by default, up to 40%) keep captions off the left and right edges. Font size and maximum line width follow each video's resolution, so vertical and 4K
need no extra settings.

## Profiles, fonts and rewriting subtitle files

A profile saves the caption look and position under a name (`profiles.json` in the app data folder); models and the
spoken language are not part of it. One font family, chosen from those installed on the computer, applies to every
speaker in the FCPXML, the TTML and the preview.

Dropping `.fcpxml`, `.srt`, `.ttml` or `.transcript.json` files writes them again from the transcript of the same name,
with the selected profile's saved look, or the current settings when none is selected. The results are new files named
like a second run (`clip-2.ttml`); existing files are never replaced by a drop.

## Layout

| Path | Contents |
| --- | --- |
| `src/` | Preact + Base UI + Tailwind UI, translated into the OS language (`src/i18n`) |
| `src-tauri/src/` | Rust core; `shell.rs` is the only Tauri wiring |
| `scripts/release/` | What builds the distribution: pinned ffmpeg, whisper.cpp and nemo-speech for the bundle |
| `scripts/verify/` | Development and verification only (auto rebuild, coverage, disk pruning, Resolve and Parallels checks); never bundled |
| `tests/a11y/` | WCAG AAA sweep over every UI state, and Core Web Vitals |

## Develop

```sh
bun install --frozen-lockfile
bash scripts/release/fetch-runtime.sh macos   # or windows-geforce, windows-radeon (both need the Vulkan SDK)
bun run tsc && bun run lint && bun run test
bun run test:a11y                     # needs: bunx playwright install webkit chromium
bun run test:vitals                   # Core Web Vitals, alone: timings beside other work mean nothing
bun run help:images                   # redraws the help page's pictures in public/help after a UI change
(cd src-tauri && cargo test && bash ../scripts/verify/check-rust-coverage.sh)
bunx tauri dev
```

To make the release files on a Mac without waiting for GitHub (for example to register them on BOOTH):

```sh
bash scripts/release/build-local.sh all   # or macos, windows-geforce, windows-radeon
```

They land in `edits/work/transcribe-app/dist-local/` under the same names CI gives them. The Windows builds cross-compile
with `cargo-xwin` and need `brew install llvm lld nsis shaderc vulkan-headers spirv-headers`, plus
`XWIN_ACCEPT_LICENSE=true` once you have read and accepted Microsoft's CRT and SDK license. Runtimes, cargo-xwin and the
SDK stay under `edits/work/transcribe-app/`; only the Rust target and the Homebrew packages sit elsewhere.

A Windows distribution is chosen when building: `bunx tauri build --bundles nsis --features windows-geforce` or
`--features windows-radeon`, after fetching the matching runtime. macOS builds use neither feature.

`transcribe-app --headless VIDEO` runs the same pipeline without a window. `TRANSCRIBE_DATA_DIR` and
`TRANSCRIBE_RESOURCE_DIR` override where data and bundled files live.

Quality checks run before a commit, not in CI, and only for what changed: `bun run check:changed` runs what the staged files need, and `bun run test:a11y:changed` only the WCAG part. Install the hook once per clone:

```sh
git config --local hook.transcribe-app.event pre-commit
git config --local hook.transcribe-app.command "bash native/transcribe-multiplatform-app/scripts/verify/pre-commit.sh"
```

To see captions in DaVinci Resolve, `scripts/verify/resolve-export.sh <file.fcpxml> <out.mp4>` imports the FCPXML into a temporary project and writes an MP4 through the Deliver page's YouTube preset, without uploading. It needs Resolve closed and works only while the Mac is idle. It is a development tool and is not shipped.

## Verification

Evidence levels differ: tests, a real run and a look in DaVinci Resolve are separate claims.

Checked on an Apple Silicon Mac with DaVinci Resolve 21.1.2 and two real one-speaker recordings (40 minutes landscape 1920x1080, 85 minutes portrait 720x1280):

- Both full recordings produce all four files. In the FCPXML no captions overlap in time or lane, none is longer than 7 seconds and none is shorter than 5/6 of a second.
- FCPXML imports as a timeline. Viewed in Resolve: white text with a black edge, a blue speaker colour with a white edge, one and two lines, a larger font, an offset, and positions above the face, below the face, at the top and at the bottom.
- Caption starts against Nemotron 3.5 word times, which the app does not use, on the captions that could be matched: more than 200 ms early on 7 of 287 (full landscape) and 36 of 1044 (the part of the portrait video measured before the run was stopped). Nemotron is not ground truth.
- Resolve draws FCPXML text at height / 1080 x 0.974 of `fontSize` (measured at 720x1280, 1280x720, 1920x1080 and 3840x2160) and half of `strokeWidth` in frame pixels. It ignores shadow and `kerning`, so the app offers neither.
- A real download of whisper-large-v3-turbo, Nemotron 3 Diarization and Nemotron 3.5 Transcribe IQ4 (21 GB, with a resume after a dropped connection).
- Real Nemotron 3.5 inference through Metal on a 30-second clip: one speaker found, word times returned.

Windows: the Radeon installer on Windows 11 ARM in Parallels, without a GPU: a 30-second clip through the command line and the first screen. Not checked there: drag and drop, the second Whisper pass on Vulkan (a failed pass falls back to coarser times).

Windows, Ryzen 5 3600 with a Radeon RX 6900 XT: v0.1.1's Vulkan `whisper-cli` contains AVX-512 and crashes on this CPU with `0xc000001d`. With that binary in place, a 30-second clip through the command line crashed once, then finished on the CPU build.

Not checked: two or more real speakers, a real GeForce or Radeon GPU, TTML and SRT placed on a Resolve timeline. The bulk download, the reset and the missing-model error have tests only.

Avatars with hair reaching the top of the frame leave no room above the head. The caption then goes below the face, because not covering the face comes first.
