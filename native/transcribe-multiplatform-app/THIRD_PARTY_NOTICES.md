# Third-party notices

Bundled in the release builds:

| Component | Use | License |
| --- | --- | --- |
| [FFmpeg](https://ffmpeg.org/) (LGPL build; `runtime/ffmpeg/` holds its license and `BUILD.txt`, which says how it was built; the macOS source is in each GitHub release as `ffmpeg-macos-source.tar.xz`, the Windows build names its [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds) release) | audio extraction, frame sampling | LGPL-2.1-or-later |
| [whisper.cpp](https://github.com/ggml-org/whisper.cpp), built with `scripts/release/whisper-cpp.patch` | transcription runtime (Vulkan and CPU in both Windows builds, Metal on macOS) | MIT |
| [NeMo-Speech.cpp](https://github.com/NVIDIA/NeMo-Speech.cpp) | speaker separation runtime (CPU in both Windows builds, Vulkan in the Radeon build, Metal on macOS) | Apache-2.0 |
| [Vibrato](https://github.com/daac-tools/vibrato) with MeCab IPADIC (`resources/ipadic-*`) | Japanese phrase boundaries for caption breaks | MIT/Apache-2.0; IPADIC license in `resources/ipadic-COPYING` |
| [fontdb](https://github.com/RazrFalcon/fontdb) | listing the installed font families | MIT |
| [ONNX Runtime](https://github.com/microsoft/onnxruntime) (its notices are beside it in `runtime/onnxruntime/`) and [ort](https://github.com/pykeio/ort) | running MossFormer2 on the CPU | MIT; ort: MIT or Apache-2.0 |
| [tauri-plugin-updater](https://github.com/tauri-apps/plugins-workspace), with [reqwest](https://github.com/seanmonstar/reqwest) and [minisign-verify](https://github.com/jedisct1/rust-minisign-verify) | checking for, verifying and applying app updates | MIT or Apache-2.0 (minisign-verify: MIT) |
| [Silero VAD](https://github.com/snakers4/silero-vad) (ggml conversion from [ggml-org/whisper-vad](https://huggingface.co/ggml-org/whisper-vad)) | when a voice starts, for caption timing | MIT |
| [YuNet](https://huggingface.co/opencv/face_detection_yunet) | face detection model | MIT |
| [Whisper tokenizer vocabulary](https://huggingface.co/openai/whisper-large-v3-turbo) | model conversion | MIT |

Downloaded by the user in the app, never bundled: MossFormer2_SS_16K (Apache-2.0; our ONNX conversion
[kkk4oru/mossformer2-ss-16k-onnx](https://huggingface.co/kkk4oru/mossformer2-ss-16k-onnx) of
[alibabasglab/MossFormer2_SS_16K](https://huggingface.co/alibabasglab/MossFormer2_SS_16K)), the CUDA build of NeMo-Speech.cpp (Apache-2.0; GeForce build only,
which also fetches the NVIDIA CUDA libraries it ships with under NVIDIA's own license), Whisper large-v3-turbo (MIT) and
the speaker branch of [NVIDIA Nemotron 3.5 Transcribe 30B-A4B](https://huggingface.co/nvidia/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B) (OpenMDW-1.1), converted from NVIDIA's weights with NVIDIA's [NeMo-Speech.cpp](https://github.com/NVIDIA/NeMo-Speech.cpp) tools by `scripts/release/make-speaker-models.py`; its other weights are replaced by zeros of smaller shapes. `src-tauri/resources/speaker-decoder-head.gguf` holds the vocabulary and settings of that NVIDIA model (OpenMDW-1.1); it has no weights, and the app fills the rest with zeros.
