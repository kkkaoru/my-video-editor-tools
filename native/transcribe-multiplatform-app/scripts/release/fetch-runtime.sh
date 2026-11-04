#!/usr/bin/env bash
# Fills src-tauri/runtime with pinned speech and media tools for one distribution.
# Usage: fetch-runtime.sh macos|windows-geforce|windows-radeon
# (run from any directory; needs curl, tar/unzip, cmake on macOS, and the Vulkan SDK on Windows)
set -euo pipefail
distribution="${1:?usage: fetch-runtime.sh macos|windows-geforce|windows-radeon}"
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
  # The shader tool is a nested CMake project; under the deep checkout its paths pass the Windows limit.
  local short="${RUNNER_TEMP:-$work}/wv"
  rm -rf "$short"
  git clone --quiet --depth 1 --branch "$WHISPER_TAG" https://github.com/ggml-org/whisper.cpp "$short/src"
  cmake -S "$short/src" -B "$short/b" -DBUILD_SHARED_LIBS=OFF -DGGML_VULKAN=ON -DWHISPER_BUILD_TESTS=OFF -DWHISPER_BUILD_EXAMPLES=ON -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded >/dev/null
  cmake --build "$short/b" --config Release --target whisper-cli -j 8 >"$short/build.log" ||
    { tail -40 "$short/build.log"; return 1; }
  rm -rf "$runtime/whisper-vulkan" && mkdir -p "$runtime/whisper-vulkan"
  cp "$short/b/bin/Release/whisper-cli.exe" "$runtime/whisper-vulkan/"
  rm -rf "$short"
}

# The archive holds every example and test of whisper.cpp; these are all whisper-cli's imports reach.
# ggml-cpu-*.dll are loaded at run time by CPU features, so no import table lists them.
prune_whisper_bin() { # folder
  find "$1" -type f ! \( -name whisper-cli.exe -o -name whisper.dll -o -name ggml.dll -o -name ggml-base.dll -o -name 'ggml-cpu-*.dll' \) -delete
  [ "$(find "$1" -name '*.exe' | wc -l | tr -d ' ')" = 1 ] || { echo "whisper-bin kept unexpected executables" >&2; return 1; }
}

nemo_zip() { # sha256 asset folder
  fetch "$(nemo_url "$2")" "$1" "$work/$3.zip"
  rm -rf "${runtime:?}/${3:?}" && mkdir -p "$runtime/$3" && unzip -q -o "$work/$3.zip" -d "$runtime/$3"
  find "$runtime/$3" \( -name include -o -name cmake -o -name '*.lib' \) -prune -exec rm -rf {} +
}

# Both Windows distributions: whisper on Vulkan, and CPU builds for machines without a usable GPU.
windows_common() {
  windows_whisper_vulkan
  nemo_zip 4ed9740e361e4653f56c3dce7d1e81883a1c8b831c1bd7d01567fff1130b5b05 windows-x86_64-cpu.zip nemo
  fetch "https://github.com/ggml-org/whisper.cpp/releases/download/$WHISPER_BIN_TAG/whisper-bin-x64.zip" 6ba69e3482d7826214f90a6a9c84ca07782aec1e1d0c6a7c30c994fd5d816ccb "$work/whisper.zip"
  rm -rf "$runtime/whisper" && mkdir -p "$runtime/whisper" && unzip -q -o "$work/whisper.zip" -d "$runtime/whisper"
  prune_whisper_bin "$runtime/whisper"
  # whisper.cpp's archive omits the VC++ runtime, which clean Windows installs do not have; nemo-speech ships it.
  find "$runtime/nemo" \( -name 'vcruntime140*.dll' -o -name 'msvcp140*.dll' -o -name 'vcomp140.dll' \) -exec cp {} "$runtime/whisper/Release/" \;
  fetch "https://github.com/BtbN/FFmpeg-Builds/releases/download/$FFMPEG_WIN_TAG/$FFMPEG_WIN_ZIP" "$FFMPEG_WIN_SHA256" "$work/ffmpeg.zip"
  rm -rf "$runtime/ffmpeg" "$work/ffmpeg" && mkdir -p "$runtime/ffmpeg" "$work/ffmpeg" && unzip -q -o "$work/ffmpeg.zip" -d "$work/ffmpeg"
  find "$work/ffmpeg" -type f \( -name ffmpeg.exe -o -name ffprobe.exe \) -exec cp {} "$runtime/ffmpeg/" \;
}

case "$distribution" in
  macos) mac_nemo; mac_whisper; mac_ffmpeg ;;
  # A GeForce runs nemo-speech on the CUDA runtime the app downloads, so no Vulkan nemo-speech ships.
  windows-geforce) windows_common; rm -rf "$runtime/nemo-vulkan" ;;
  windows-radeon) windows_common; nemo_zip 72b3b8b41bfe2be0320eff4e9d310594e4f99deefcde4a157799a039656963ce windows-x86_64-vulkan.zip nemo-vulkan ;;
  *) echo "unknown distribution: $distribution" >&2; exit 2 ;;
esac
du -sh "$runtime"/*
