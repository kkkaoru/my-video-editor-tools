# Transcribe for Resolve

Drop a video; subtitle files for DaVinci Resolve 21 appear next to it, coloured per speaker and outlined (袋文字).
A desktop app for macOS (Apple silicon) and Windows (x64). Download it from the
[GitHub releases](https://github.com/kkkaoru/tiktok-live-archive-downloader/releases) tagged `transcribe-app-v*`:

| Your machine | Download | Transcription | Speakers |
| --- | --- | --- | --- |
| Mac (Apple silicon) | `Transcribe-for-Resolve-macos-arm64.zip` | Metal | Metal |
| Windows with a GeForce | `Transcribe-for-Resolve-windows-geforce-x64-setup.exe` | Vulkan | CUDA once its runtime is downloaded in the app, the CPU until then |
| Windows with a Radeon | `Transcribe-for-Resolve-windows-radeon-x64-setup.exe` | Vulkan | Vulkan |
| Windows with neither | Either Windows download | CPU | CPU |

Each Windows download uses only its own vendor's GPU: the GeForce build never uses a Radeon, nor the reverse. Without
that GPU it runs on the CPU, and the Transcribe tab says so because it is slower. The right build needs an up-to-date GPU
driver, which provides Vulkan.

The builds are not notarized or code-signed with a paid certificate: on macOS open the app, then choose System Settings >
Privacy & Security > Open Anyway; on Windows choose "More info > Run anyway" in SmartScreen.

## What it writes

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
speakers at most; the palette has ten editable slots by default and can be resized.

Models can be downloaded together, each with its own progress bar. Resetting deletes only what the app itself downloaded,
after a confirmation; folders you added, anything overlapping them and the Hugging Face cache are never touched, and it
is refused while a job or a download runs. Videos are refused with a message naming what is missing until the models
for transcription and, unless one speaker is set, speaker separation are present.

## Caption position and size

Above or below the face, or the top or bottom of the video, each with horizontal and vertical fine-tuning in percent of
the frame. Font size and maximum line width follow each video's resolution, so vertical and 4K need no extra settings.

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
(cd src-tauri && cargo test && bash ../scripts/verify/check-rust-coverage.sh)
bunx tauri dev
```

A Windows distribution is chosen when building: `bunx tauri build --bundles nsis --features windows-geforce` or
`--features windows-radeon`, after fetching the matching runtime. macOS builds use neither feature.

`transcribe-app --headless VIDEO` runs the same pipeline without a window. `TRANSCRIBE_DATA_DIR` and
`TRANSCRIBE_RESOURCE_DIR` override where data and bundled files live.

## Verification
