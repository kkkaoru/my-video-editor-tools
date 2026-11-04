# Transcribe for Resolve

Drop a video; subtitle files for DaVinci Resolve 21 appear next to it, coloured per speaker and outlined (袋文字).
A single desktop app for macOS (Apple silicon) and Windows (x64). Download it from the
[GitHub releases](https://github.com/kkkaoru/tiktok-live-archive-downloader/releases) tagged `transcribe-app-v*`.

The builds are not notarized or code-signed with a paid certificate: on macOS open the app with Control-click > Open,
on Windows choose "More info > Run anyway" in SmartScreen.

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
| Speakers | [Nemotron 3.5 Transcribe 30B-A4B](https://huggingface.co/nvidia/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B) | NVIDIA publishes safetensors only; GGUF builds come from AtomicChat. Chosen per machine: Mac and GPUs ≥ 24 GB IQ4_NL, Blackwell GeForce NVFP4, 16 GB GPUs Q2_0_M, 24 GB Macs Q2_0_L, 64 GB Macs Q8_0. |
| Speakers (small) | [Nemotron 3 Diarization](https://huggingface.co/nvidia/Nemotron-3-Diarization) | 107 MB, runs anywhere. Used when the large model does not fit. |
| Faces | YuNet (bundled), `avatar-face-caption-v2.onnx` | The same model and engine (`tract`) on both systems. Put the avatar ONNX in a model folder to use it. |

On a Windows PC with an NVIDIA GPU the model manager offers a CUDA runtime download. Nemotron speaker models reach
eight speakers at most; the palette has ten editable slots by default and can be resized.

## Caption position and size

Above or below the face, or the top or bottom of the video, each with horizontal and vertical fine-tuning in percent of
the frame. Font size and maximum line width follow each video's resolution, so vertical and 4K need no extra settings.

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
bash scripts/release/fetch-runtime.sh macos   # or windows
bun run tsc && bun run lint && bun run test
bun run test:a11y                     # needs: bunx playwright install webkit chromium
(cd src-tauri && cargo test && bash ../scripts/verify/check-rust-coverage.sh)
bunx tauri dev
```

`transcribe-app --headless VIDEO` runs the same pipeline without a window. `TRANSCRIBE_DATA_DIR` and
`TRANSCRIBE_RESOURCE_DIR` override where data and bundled files live.

## Verification
