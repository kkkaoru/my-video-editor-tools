#!/usr/bin/env bash
# Fills src-tauri/runtime with pinned speech and media tools for the current OS.
# Usage: fetch-runtime.sh macos|windows   (run from any directory; needs curl, tar/unzip, cmake on macOS)
set -euo pipefail
os="${1:?usage: fetch-runtime.sh macos|windows}"
app_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
runtime="$app_dir/src-tauri/runtime"
work="${RUNTIME_WORK_DIR:-$app_dir/src-tauri/target-runtime}"
NEMO_VERSION=0.3.0
WHISPER_TAG=v1.9.5
WHISPER_BIN_TAG=b5454
FFMPEG_MAC_VERSION=8.0.1
FFMPEG_MAC_SHA256=05ee0b03119b45c0bdb4df654b96802e909e0a752f72e4fe3794f487229e5a41
FFMPEG_WIN_TAG=autobuild-2026-11-04-12-51
FFMPEG_WIN_ZIP=ffmpeg-n8.1.3-34-ga35157abb9-win64-lgpl-8.1.zip
FFMPEG_WIN_SHA256=c966229f0b033a1cf93b3fb139a804358a4ec27a53b8a6213fe08301868a7668
mkdir -p "$runtime" "$work"

# Git Bash on Windows has sha256sum but not shasum.
verify() { # sha256 file
  if command -v sha256sum >/dev/null; then
    echo "$1  $2" | sha256sum -c - >/dev/null
  else
    echo "$1  $2" | shasum -a 256 -c - >/dev/null
  fi
}

fetch() { # url sha256 output
  if [ ! -f "$3" ] || ! verify "$2" "$3" 2>/dev/null; then
    curl -fsSL --retry 3 -o "$3" "$1"
    verify "$2" "$3"
  fi
}

nemo_url() { echo "https://github.com/NVIDIA/NeMo-Speech.cpp/releases/download/v$NEMO_VERSION/nemo-speech-$NEMO_VERSION-$1"; }

mac_nemo() {
  fetch "$(nemo_url macos-aarch64-metal.tar.gz)" 2b972ed8b5dba07492212acf8cb75cbf34ecb141198ffa1997febe84b7c4d9ed "$work/nemo.tgz"
  rm -rf "$runtime/nemo" "$work/nemo" && mkdir -p "$runtime/nemo/bin" "$runtime/nemo/lib" "$work/nemo"
  tar xzf "$work/nemo.tgz" -C "$work/nemo" --strip-components=1
  cp "$work/nemo/bin/nemo-speech" "$runtime/nemo/bin/"
  # Tauri resources do not keep symlinks, so each name the binary may load becomes a real file.
  (cd "$work/nemo/lib" && find . -maxdepth 1 -name '*.dylib' -exec cp -L {} "$runtime/nemo/lib/" \;)
}

mac_whisper() {
  rm -rf "$work/whisper-src" && git clone --quiet --depth 1 --branch "$WHISPER_TAG" https://github.com/ggml-org/whisper.cpp "$work/whisper-src"
  cmake -S "$work/whisper-src" -B "$work/whisper-build" -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON -DWHISPER_BUILD_TESTS=OFF -DWHISPER_BUILD_EXAMPLES=ON >/dev/null
  cmake --build "$work/whisper-build" --target whisper-cli -j 8 >/dev/null
  rm -rf "$runtime/whisper" && mkdir -p "$runtime/whisper"
  cp "$work/whisper-build/bin/whisper-cli" "$runtime/whisper/"
}

mac_ffmpeg() {
  # LGPL build holding only what subtitle generation needs; hardware-free so it runs anywhere.
  fetch "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_MAC_VERSION.tar.xz" "$FFMPEG_MAC_SHA256" "$work/ffmpeg.tar.xz"
  rm -rf "$work/ffmpeg-src" && mkdir -p "$work/ffmpeg-src" && tar xJf "$work/ffmpeg.tar.xz" -C "$work/ffmpeg-src" --strip-components=1
  (cd "$work/ffmpeg-src" && ./configure --disable-doc --disable-debug --disable-ffplay --disable-network --disable-autodetect --enable-videotoolbox --enable-audiotoolbox >/dev/null && make -j 8 ffmpeg ffprobe >/dev/null)
  rm -rf "$runtime/ffmpeg" && mkdir -p "$runtime/ffmpeg"
  cp "$work/ffmpeg-src/ffmpeg" "$work/ffmpeg-src/ffprobe" "$runtime/ffmpeg/"
}

# whisper.cpp publishes no Vulkan binary. Needs the Vulkan SDK (VULKAN_SDK) at build time only.
windows_whisper_vulkan() {
  rm -rf "$work/whisper-src" "$work/whisper-vulkan-build"
  git clone --quiet --depth 1 --branch "$WHISPER_TAG" https://github.com/ggml-org/whisper.cpp "$work/whisper-src"
  cmake -S "$work/whisper-src" -B "$work/whisper-vulkan-build" -DBUILD_SHARED_LIBS=OFF -DGGML_VULKAN=ON -DWHISPER_BUILD_TESTS=OFF -DWHISPER_BUILD_EXAMPLES=ON -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded >/dev/null
  cmake --build "$work/whisper-vulkan-build" --config Release --target whisper-cli -j 8 >"$work/whisper-vulkan-build.log" ||
    { tail -40 "$work/whisper-vulkan-build.log"; cmake -S "$work/whisper-src/ggml/src/ggml-vulkan/vulkan-shaders" -B "$work/shaders-probe" 2>&1 | tail -40; find "$work/whisper-vulkan-build" -name '*shaders-gen*.log' -exec tail -30 {} +; which glslc; echo "VULKAN_SDK=$VULKAN_SDK"; return 1; }
  rm -rf "$runtime/whisper-vulkan" && mkdir -p "$runtime/whisper-vulkan"
  cp "$work/whisper-vulkan-build/bin/Release/whisper-cli.exe" "$runtime/whisper-vulkan/"
}

windows_all() {
  # Radeon and CUDA-less GeForce run these Vulkan builds; they need the GPU driver's vulkan-1.dll, so CPU builds stay too.
  fetch "$(nemo_url windows-x86_64-vulkan.zip)" 72b3b8b41bfe2be0320eff4e9d310594e4f99deefcde4a157799a039656963ce "$work/nemo-vulkan.zip"
  rm -rf "$runtime/nemo-vulkan" && mkdir -p "$runtime/nemo-vulkan" && unzip -q -o "$work/nemo-vulkan.zip" -d "$runtime/nemo-vulkan"
  find "$runtime/nemo-vulkan" \( -name include -o -name cmake -o -name '*.lib' \) -prune -exec rm -rf {} +
  windows_whisper_vulkan
  fetch "$(nemo_url windows-x86_64-cpu.zip)" 4ed9740e361e4653f56c3dce7d1e81883a1c8b831c1bd7d01567fff1130b5b05 "$work/nemo.zip"
  rm -rf "$runtime/nemo" && mkdir -p "$runtime/nemo" && unzip -q -o "$work/nemo.zip" -d "$runtime/nemo"
  find "$runtime/nemo" \( -name include -o -name cmake -o -name '*.lib' \) -prune -exec rm -rf {} +
  fetch "https://github.com/ggml-org/whisper.cpp/releases/download/$WHISPER_BIN_TAG/whisper-bin-x64.zip" 6ba69e3482d7826214f90a6a9c84ca07782aec1e1d0c6a7c30c994fd5d816ccb "$work/whisper.zip"
  rm -rf "$runtime/whisper" && mkdir -p "$runtime/whisper" && unzip -q -o "$work/whisper.zip" -d "$runtime/whisper"
  find "$runtime/whisper" -name '*.exe' ! -name whisper-cli.exe -delete
  # whisper.cpp's archive omits the VC++ runtime, which clean Windows installs do not have; nemo-speech ships it.
  find "$runtime/nemo" \( -name 'vcruntime140*.dll' -o -name 'msvcp140*.dll' -o -name 'vcomp140.dll' \) -exec cp {} "$runtime/whisper/Release/" \;
  fetch "https://github.com/BtbN/FFmpeg-Builds/releases/download/$FFMPEG_WIN_TAG/$FFMPEG_WIN_ZIP" "$FFMPEG_WIN_SHA256" "$work/ffmpeg.zip"
  rm -rf "$runtime/ffmpeg" "$work/ffmpeg" && mkdir -p "$runtime/ffmpeg" "$work/ffmpeg" && unzip -q -o "$work/ffmpeg.zip" -d "$work/ffmpeg"
  find "$work/ffmpeg" -type f \( -name ffmpeg.exe -o -name ffprobe.exe \) -exec cp {} "$runtime/ffmpeg/" \;
}

case "$os" in
  macos) mac_nemo; mac_whisper; mac_ffmpeg ;;
  windows) windows_all ;;
  *) echo "unknown OS: $os" >&2; exit 2 ;;
esac
du -sh "$runtime"/*
