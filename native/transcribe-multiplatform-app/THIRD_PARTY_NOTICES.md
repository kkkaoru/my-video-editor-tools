# Third-party notices

Bundled in the release builds:

| Component | Use | License |
| --- | --- | --- |
| [FFmpeg](https://ffmpeg.org/) (LGPL build) | audio extraction, frame sampling | LGPL-2.1-or-later |
| [whisper.cpp](https://github.com/ggml-org/whisper.cpp) | transcription runtime | MIT |
| [NeMo-Speech.cpp](https://github.com/NVIDIA/NeMo-Speech.cpp) | speaker separation runtime | Apache-2.0 |
| [YuNet](https://huggingface.co/opencv/face_detection_yunet) | face detection model | MIT |
| [Whisper tokenizer vocabulary](https://huggingface.co/openai/whisper-large-v3-turbo) | model conversion | MIT |

Downloaded by the user in the app, never bundled: Whisper large-v3-turbo (MIT), Nemotron 3 Diarization and
Nemotron 3.5 Transcribe 30B-A4B (OpenMDW-1.1; the GGUF conversions come from the third-party repository
AtomicChat/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-GGUF because NVIDIA publishes only safetensors checkpoints).
