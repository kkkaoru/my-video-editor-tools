#!/usr/bin/env bash
# Fills src-tauri/runtime with pinned speech and media tools for one distribution.
# Usage: fetch-runtime.sh macos|windows-geforce|windows-radeon
# (run from any directory; needs curl, tar/unzip, cmake on macOS, and the Vulkan SDK on Windows;
# a Windows distribution fetched on a Mac cross-builds through scripts/release/build-local.sh)
set -euo pipefail
distribution="${1:?usage: fetch-runtime.sh macos|windows-geforce|windows-radeon}"
app_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
runtime="${RUNTIME_DIR:-$app_dir/src-tauri/runtime}"
work="${RUNTIME_WORK_DIR:-$app_dir/src-tauri/target-runtime}"
NEMO_VERSION=0.3.0
WHISPER_TAG=v1.9.5
WHISPER_COMMIT=d1be6fde11ac6e0407606b4e42fe72d34add8037
VOICE_MODEL=ggml-silero-v6.2.0.bin
VOICE_MODEL_SHA256=2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987
FFMPEG_MAC_VERSION=8.0.1
FFMPEG_MAC_SHA256=05ee0b03119b45c0bdb4df654b96802e909e0a752f72e4fe3794f487229e5a41
# BtbN keeps daily tags about two weeks but month-end ones for good.
FFMPEG_WIN_TAG=autobuild-2026-10-31-12-59
FFMPEG_WIN_ZIP=ffmpeg-n8.1.3-27-g8dca1586f4-win64-lgpl-8.1.zip
FFMPEG_WIN_SHA256=d4c2a42673917b4a97104a3e9f082729a9730796b98664e37c0b7eba6c6c613d
ONNXRUNTIME_VERSION=1.31.1
ONNXRUNTIME_MAC_SHA256=2e1344a1926ee50eb25b38153cb6992c3567515aef0e677a1a520a1798896d6f
ONNXRUNTIME_WIN_SHA256=5869e26bb25314375f2d5ef1877e3ea06b1543d6c710e6e4a1ec2f7177ab4b2c
VULKAN_LOADER_DEF_TAG=vulkan-sdk-1.4.335.0
VULKAN_LOADER_DEF_SHA256=9ba339b7f5ee2df28487698a6840ecf095fac58b415c2c89ad9f161e9d316378
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

# The model file whisper-vad-speech-segments reads, beside that tool. MIT, from Silero VAD.
voice_model_into() { # folder
  fetch "https://huggingface.co/ggml-org/whisper-vad/resolve/main/$VOICE_MODEL" "$VOICE_MODEL_SHA256" "$work/$VOICE_MODEL"
  cp "$work/$VOICE_MODEL" "$1/"
}

clone_whisper() { # folder
  git clone --quiet --depth 1 --branch "$WHISPER_TAG" https://github.com/ggml-org/whisper.cpp "$1"
  [ "$(git -C "$1" rev-parse HEAD)" = "$WHISPER_COMMIT" ] || { echo "whisper.cpp $WHISPER_TAG is not the pinned commit" >&2; return 1; }
  # Token times from the text pass itself, so no second pass; see docs/issues/whisper-fixed-text-alignment.md. Also -mavxvnni for clang-cl.
  git -C "$1" apply "$app_dir/scripts/release/whisper-cpp.patch"
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
  rm -rf "$work/whisper-src" && clone_whisper "$work/whisper-src"
  cmake -S "$work/whisper-src" -B "$work/whisper-build" -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON -DWHISPER_BUILD_TESTS=OFF -DWHISPER_BUILD_EXAMPLES=ON >/dev/null
  cmake --build "$work/whisper-build" --target whisper-cli whisper-vad-speech-segments -j 8 >/dev/null
  rm -rf "$runtime/whisper" && mkdir -p "$runtime/whisper"
  cp "$work/whisper-build/bin/whisper-cli" "$work/whisper-build/bin/whisper-vad-speech-segments" "$runtime/whisper/"
  voice_model_into "$runtime/whisper"
}

mac_ffmpeg() {
  # LGPL build holding only what subtitle generation needs; hardware-free so it runs anywhere.
  fetch "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_MAC_VERSION.tar.xz" "$FFMPEG_MAC_SHA256" "$work/ffmpeg.tar.xz"
  rm -rf "$work/ffmpeg-src" && mkdir -p "$work/ffmpeg-src" && tar xJf "$work/ffmpeg.tar.xz" -C "$work/ffmpeg-src" --strip-components=1
  (cd "$work/ffmpeg-src" && ./configure --disable-doc --disable-debug --disable-ffplay --disable-network --disable-autodetect --enable-videotoolbox --enable-audiotoolbox >/dev/null && make -j 8 ffmpeg ffprobe >/dev/null)
  rm -rf "$runtime/ffmpeg" && mkdir -p "$runtime/ffmpeg"
  cp "$work/ffmpeg-src/ffmpeg" "$work/ffmpeg-src/ffprobe" "$work/ffmpeg-src/COPYING.LGPLv2.1" "$runtime/ffmpeg/"
  printf 'FFmpeg %s, built from https://ffmpeg.org/releases/ffmpeg-%s.tar.xz (SHA-256 %s).\nConfigured with: %s\n' \
    "$FFMPEG_MAC_VERSION" "$FFMPEG_MAC_VERSION" "$FFMPEG_MAC_SHA256" "$("$runtime/ffmpeg/ffmpeg" -hide_banner -buildconf | sed -n 's/^ *--/--/p' | tr '\n' ' ')" \
    > "$runtime/ffmpeg/BUILD.txt"
}

# whisper.cpp publishes no Vulkan binary. Needs the Vulkan SDK (VULKAN_SDK) at build time only.
windows_whisper_vulkan() {
  # The shader tool is a nested CMake project; under the deep checkout its paths pass the Windows limit.
  local short="${RUNNER_TEMP:-$work}/wv"
  rm -rf "$short"
  clone_whisper "$short/src"
  # GGML_NATIVE would target the build machine: a runner with AVX-512 makes a binary that crashes on PCs without it.
  cmake -S "$short/src" -B "$short/b" -DBUILD_SHARED_LIBS=OFF -DGGML_VULKAN=ON -DGGML_NATIVE=OFF -DGGML_AVX=ON -DGGML_AVX2=ON -DGGML_FMA=ON -DGGML_F16C=ON -DGGML_BMI2=ON -DWHISPER_BUILD_TESTS=OFF -DWHISPER_BUILD_EXAMPLES=ON -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded >/dev/null
  cmake --build "$short/b" --config Release --target whisper-cli -j 8 >"$short/build.log" ||
    { tail -40 "$short/build.log"; return 1; }
  rm -rf "$runtime/whisper-vulkan" && mkdir -p "$runtime/whisper-vulkan"
  cp "$short/b/bin/Release/whisper-cli.exe" "$runtime/whisper-vulkan/"
  rm -rf "$short"
}

# One ggml-cpu-*.dll per instruction set, loaded by what the CPU has: this build is the fallback for a CPU the Vulkan build crashes on.
WHISPER_CPU_FLAGS=(-DBUILD_SHARED_LIBS=ON -DGGML_BACKEND_DL=ON -DGGML_NATIVE=OFF -DGGML_CPU_ALL_VARIANTS=ON -DWHISPER_BUILD_TESTS=OFF -DWHISPER_BUILD_EXAMPLES=ON)

# What whisper-cli and the voice detector load; a CMake build also leaves import libraries and other tools there.
whisper_cpu_into_runtime() { # build-bin-folder
  rm -rf "$runtime/whisper" && mkdir -p "$runtime/whisper/Release"
  cp "$1"/whisper-cli.exe "$1"/whisper-vad-speech-segments.exe "$1"/whisper.dll "$1"/ggml.dll "$1"/ggml-base.dll "$1"/ggml-cpu-*.dll "$runtime/whisper/Release/"
}

windows_whisper_cpu() {
  local short="${RUNNER_TEMP:-$work}/wc"
  rm -rf "$short"
  clone_whisper "$short/src"
  cmake -S "$short/src" -B "$short/b" "${WHISPER_CPU_FLAGS[@]}" >/dev/null
  # No --target: nothing links the ggml-cpu-*.dll, so a target list would have to name each one.
  cmake --build "$short/b" --config Release -j 8 >"$short/build.log" ||
    { tail -40 "$short/build.log"; return 1; }
  whisper_cpu_into_runtime "$short/b/bin/Release"
  rm -rf "$short"
}

# The toolchain for building from a Mac: clang-cl and the Windows SDK cargo-xwin downloaded (XWIN_CACHE_DIR).
cross_toolchain_file() { # file
  # cargo-xwin's toolchain clears the standard libraries; ggml reads the registry through advapi32.
  cat >"$1" <<EOF
include($XWIN_CACHE_DIR/cmake/clang-cl/x86_64-pc-windows-msvc-toolchain.cmake)
string(APPEND CMAKE_EXE_LINKER_FLAGS " advapi32.lib")
string(APPEND CMAKE_SHARED_LINKER_FLAGS " advapi32.lib")
string(APPEND CMAKE_MODULE_LINKER_FLAGS " advapi32.lib")
EOF
}

windows_whisper_cpu_from_mac() {
  local short="$work/wc"
  rm -rf "$short" && mkdir -p "$short"
  clone_whisper "$short/src"
  cross_toolchain_file "$short/target.cmake"
  cmake -S "$short/src" -B "$short/b" -G Ninja "-DCMAKE_TOOLCHAIN_FILE=$short/target.cmake" -DCMAKE_BUILD_TYPE=Release "${WHISPER_CPU_FLAGS[@]}" -DGGML_OPENMP=OFF >"$short/configure.log" 2>&1 ||
    { tail -40 "$short/configure.log"; return 1; }
  cmake --build "$short/b" -j 8 >"$short/build.log" ||
    { tail -40 "$short/build.log"; return 1; }
  whisper_cpu_into_runtime "$short/b/bin"
  rm -rf "$short"
}

# The Vulkan build from a Mac: the same toolchain, and Homebrew's headers and glslc for the Vulkan SDK.
# shortcut: those Homebrew versions are not pinned and OpenMP is off (ggml's own threads run instead), so this
# differs slightly from CI's binary; pin them if a local build ever replaces CI for a tagged release.
windows_whisper_vulkan_from_mac() {
  local short="$work/wv" brew_prefix
  brew_prefix="$(brew --prefix)"
  rm -rf "$short" && mkdir -p "$short/include"
  clone_whisper "$short/src"
  cp -R "$brew_prefix/opt/vulkan-headers/include/." "$brew_prefix/opt/spirv-headers/include/." "$short/include/"
  fetch "https://raw.githubusercontent.com/KhronosGroup/Vulkan-Loader/$VULKAN_LOADER_DEF_TAG/loader/vulkan-1.def" "$VULKAN_LOADER_DEF_SHA256" "$work/vulkan-1.def"
  llvm-lib "/def:$work/vulkan-1.def" /machine:x64 "/out:$short/vulkan-1.lib" >/dev/null
  cross_toolchain_file "$short/target.cmake"
  # Runs on the Mac during the build, to embed the shaders.
  cat >"$short/host.cmake" <<EOF
set(CMAKE_C_COMPILER /usr/bin/clang)
set(CMAKE_CXX_COMPILER /usr/bin/clang++)
EOF
  cmake -S "$short/src" -B "$short/b" -G Ninja "-DCMAKE_TOOLCHAIN_FILE=$short/target.cmake" -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF \
    -DGGML_VULKAN=ON -DGGML_NATIVE=OFF -DGGML_AVX=ON -DGGML_AVX2=ON -DGGML_FMA=ON -DGGML_F16C=ON -DGGML_BMI2=ON -DGGML_OPENMP=OFF \
    -DWHISPER_BUILD_TESTS=OFF -DWHISPER_BUILD_EXAMPLES=ON "-DVulkan_INCLUDE_DIR=$short/include" "-DVulkan_LIBRARY=$short/vulkan-1.lib" \
    "-DVulkan_GLSLC_EXECUTABLE=$brew_prefix/bin/glslc" "-DCMAKE_PREFIX_PATH=$brew_prefix" "-DGGML_VULKAN_SHADERS_GEN_TOOLCHAIN=$short/host.cmake" >"$short/configure.log" 2>&1 ||
    { tail -40 "$short/configure.log"; return 1; }
  cmake --build "$short/b" --target whisper-cli -j 8 >"$short/build.log" ||
    { tail -40 "$short/build.log"; return 1; }
  rm -rf "$runtime/whisper-vulkan" && mkdir -p "$runtime/whisper-vulkan"
  cp "$short/b/bin/whisper-cli.exe" "$runtime/whisper-vulkan/"
  rm -rf "$short"
}

nemo_zip() { # sha256 asset folder
  fetch "$(nemo_url "$2")" "$1" "$work/$3.zip"
  rm -rf "${runtime:?}/${3:?}" && mkdir -p "$runtime/$3" && unzip -q -o "$work/$3.zip" -d "$runtime/$3"
  find "$runtime/$3" \( -name include -o -name cmake -o -name '*.lib' \) -prune -exec rm -rf {} +
}

# whisper.cpp's builds need the VC++ runtime, which clean Windows installs do not have; nemo-speech ships it.
# OpenMP's vcomp140.dll stays a separate file even in the statically linked Vulkan build.
vc_runtime_into() { # folder
  find "$runtime/nemo" \( -name 'vcruntime140*.dll' -o -name 'msvcp140*.dll' -o -name 'vcomp140.dll' \) -exec cp {} "$1/" \;
}

onnxruntime_url() { # asset
  echo "https://github.com/microsoft/onnxruntime/releases/download/v$ONNXRUNTIME_VERSION/$1"
}

# ONNX Runtime for MossFormer2, on the CPU; the app loads the library at run time.
mac_onnxruntime() {
  local name="onnxruntime-osx-arm64-$ONNXRUNTIME_VERSION"
  fetch "$(onnxruntime_url "$name.tgz")" "$ONNXRUNTIME_MAC_SHA256" "$work/onnxruntime.tgz"
  rm -rf "$runtime/onnxruntime" "$work/onnxruntime" && mkdir -p "$runtime/onnxruntime" "$work/onnxruntime"
  tar -xzf "$work/onnxruntime.tgz" -C "$work/onnxruntime"
  cp "$work/onnxruntime/$name/lib/libonnxruntime.$ONNXRUNTIME_VERSION.dylib" "$runtime/onnxruntime/libonnxruntime.dylib"
  cp "$work/onnxruntime/$name/LICENSE" "$work/onnxruntime/$name/ThirdPartyNotices.txt" "$runtime/onnxruntime/"
}

# MossFormer2 on Core ML: the app's own runner, signed ad hoc like the other tools.
mac_separate_coreml() {
  swift build -c release --arch arm64 --package-path "$app_dir/separate-coreml" --scratch-path "$work/separate-coreml"
  rm -rf "$runtime/separate-coreml" && mkdir -p "$runtime/separate-coreml"
  cp "$work/separate-coreml/arm64-apple-macosx/release/separate-coreml" "$runtime/separate-coreml/"
  codesign --force --sign - "$runtime/separate-coreml/separate-coreml"
}

windows_onnxruntime() {
  local name="onnxruntime-win-x64-$ONNXRUNTIME_VERSION"
  fetch "$(onnxruntime_url "$name.zip")" "$ONNXRUNTIME_WIN_SHA256" "$work/onnxruntime.zip"
  rm -rf "$runtime/onnxruntime" "$work/onnxruntime" && mkdir -p "$runtime/onnxruntime" "$work/onnxruntime"
  unzip -q -o "$work/onnxruntime.zip" -d "$work/onnxruntime"
  cp "$work/onnxruntime/$name/lib/onnxruntime.dll" "$work/onnxruntime/$name/LICENSE" "$work/onnxruntime/$name/ThirdPartyNotices.txt" "$runtime/onnxruntime/"
  vc_runtime_into "$runtime/onnxruntime"
}

# Both Windows distributions: whisper on Vulkan, and CPU builds for machines without a usable GPU.
windows_common() {
  if [ "$(uname -s)" = Darwin ]; then
    windows_whisper_vulkan_from_mac
    windows_whisper_cpu_from_mac
  else
    windows_whisper_vulkan
    windows_whisper_cpu
  fi
  nemo_zip 4ed9740e361e4653f56c3dce7d1e81883a1c8b831c1bd7d01567fff1130b5b05 windows-x86_64-cpu.zip nemo
  vc_runtime_into "$runtime/whisper/Release"
  voice_model_into "$runtime/whisper/Release"
  vc_runtime_into "$runtime/whisper-vulkan"
  windows_onnxruntime
  fetch "https://github.com/BtbN/FFmpeg-Builds/releases/download/$FFMPEG_WIN_TAG/$FFMPEG_WIN_ZIP" "$FFMPEG_WIN_SHA256" "$work/ffmpeg.zip"
  rm -rf "$runtime/ffmpeg" "$work/ffmpeg" && mkdir -p "$runtime/ffmpeg" "$work/ffmpeg" && unzip -q -o "$work/ffmpeg.zip" -d "$work/ffmpeg"
  find "$work/ffmpeg" -type f \( -name ffmpeg.exe -o -name ffprobe.exe -o -name LICENSE.txt \) -exec cp {} "$runtime/ffmpeg/" \;
  printf 'FFmpeg built by BtbN/FFmpeg-Builds: https://github.com/BtbN/FFmpeg-Builds/releases/tag/%s\nFile: %s (SHA-256 %s)\n' \
    "$FFMPEG_WIN_TAG" "$FFMPEG_WIN_ZIP" "$FFMPEG_WIN_SHA256" > "$runtime/ffmpeg/BUILD.txt"
}

case "$distribution" in
  macos) mac_nemo; mac_whisper; mac_ffmpeg; mac_onnxruntime; mac_separate_coreml ;;
  # A GeForce runs nemo-speech on the CUDA runtime the app downloads, so no Vulkan nemo-speech ships.
  windows-geforce) windows_common; rm -rf "$runtime/nemo-vulkan" ;;
  windows-radeon) windows_common; nemo_zip 72b3b8b41bfe2be0320eff4e9d310594e4f99deefcde4a157799a039656963ce windows-x86_64-vulkan.zip nemo-vulkan ;;
  *) echo "unknown distribution: $distribution" >&2; exit 2 ;;
esac
du -sh "$runtime"/*
