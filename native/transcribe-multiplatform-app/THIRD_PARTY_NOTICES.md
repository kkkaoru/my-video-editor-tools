# Third-party notices

Bundled in the release builds:

| Component | Use | License |
| --- | --- | --- |
| [FFmpeg](https://ffmpeg.org/) (LGPL build) | audio extraction, frame sampling | LGPL-2.1-or-later |
| [whisper.cpp](https://github.com/ggml-org/whisper.cpp) | transcription runtime (Vulkan and CPU in both Windows builds, Metal on macOS) | MIT |
| [NeMo-Speech.cpp](https://github.com/NVIDIA/NeMo-Speech.cpp) | speaker separation runtime (CPU in both Windows builds, Vulkan in the Radeon build, Metal on macOS) | Apache-2.0 |
| [Vibrato](https://github.com/daac-tools/vibrato) with MeCab IPADIC (`resources/ipadic-*`) | Japanese phrase boundaries for caption breaks | MIT/Apache-2.0; IPADIC license in `resources/ipadic-COPYING` |
| [fontdb](https://github.com/RazrFalcon/fontdb) | listing the installed font families | MIT |
| [Silero VAD](https://github.com/snakers4/silero-vad) (ggml conversion from [ggml-org/whisper-vad](https://huggingface.co/ggml-org/whisper-vad)) | when a voice starts, for caption timing | MIT |
| [YuNet](https://huggingface.co/opencv/face_detection_yunet) | face detection model | MIT |
| [Whisper tokenizer vocabulary](https://huggingface.co/openai/whisper-large-v3-turbo) | model conversion | MIT |

Downloaded by the user in the app, never bundled: the CUDA build of NeMo-Speech.cpp (Apache-2.0; GeForce build only,
which also fetches the NVIDIA CUDA libraries it ships with under NVIDIA's own license), Whisper large-v3-turbo (MIT) and
Nemotron 3.5 Transcribe 30B-A4B (OpenMDW-1.1; the GGUF conversions come from the third-party repository
AtomicChat/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-GGUF because NVIDIA publishes only safetensors checkpoints).
