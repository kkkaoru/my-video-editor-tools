#!/usr/bin/env bash
# Builds the release files on this Mac, named as CI names them, without waiting for GitHub.
# Usage: build-local.sh macos|windows-geforce|windows-radeon|all
# Windows builds need `brew install llvm lld nsis shaderc vulkan-headers spirv-headers`, and
# XWIN_ACCEPT_LICENSE=true once Microsoft's CRT and SDK license (https://go.microsoft.com/fwlink/?LinkId=2086102)
# has been read and accepted: cargo-xwin downloads them.
set -euo pipefail
choice="${1:?usage: build-local.sh macos|windows-geforce|windows-radeon|all}"
app_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
local_dir="$(cd "$app_dir/../.." && pwd)/edits/work/transcribe-app"
out="$local_dir/dist-local"
cross="$local_dir/cross"
windows_target=x86_64-pc-windows-msvc
cargo_xwin_version=0.23.1
export CARGO_TARGET_DIR="$local_dir/target"
export RUNTIME_WORK_DIR="$local_dir/runtime-work"
mkdir -p "$out"

# Beside src-tauri/runtime, which the Mac's development builds and running jobs use.
fetch_runtime() { # distribution
  local runtime="$local_dir/runtime-$1" stamp
  stamp="$(shasum -a 256 "$app_dir/scripts/release/fetch-runtime.sh" | cut -d' ' -f1)"
  if [ "$(cat "$runtime/.fetched" 2>/dev/null)" != "$stamp" ]; then
    rm -rf "$runtime"
    RUNTIME_DIR="$runtime" bash "$app_dir/scripts/release/fetch-runtime.sh" "$1"
    echo "$stamp" >"$runtime/.fetched"
  fi
}

resources_config() { # distribution
  printf '{"bundle":{"resources":{"resources/":"resources/","runtime/":null,"%s/":"runtime/"}}}' "$local_dir/runtime-$1"
}

build_macos() {
  fetch_runtime macos
  (cd "$app_dir" && bunx tauri build --bundles app --config "$(resources_config macos)")
  # --norsrc: other unzip tools would otherwise leave ._ files that break the bundle's signature.
  ditto -c -k --norsrc --keepParent "$CARGO_TARGET_DIR/release/bundle/macos/Transcribe Captions.app" "$out/Transcribe-Captions-macos-arm64.zip"
}

prepare_cross_tools() {
  [ "${XWIN_ACCEPT_LICENSE:-}" = true ] || {
    echo "Read Microsoft's license, linked at the top of this script, then set XWIN_ACCEPT_LICENSE=true" >&2
    return 1
  }
  local brew_prefix
  brew_prefix="$(brew --prefix)"
  export XWIN_CACHE_DIR="$cross/xwin"
  export PATH="$brew_prefix/opt/llvm/bin:$brew_prefix/opt/lld/bin:$cross/tools/bin:$PATH"
  command -v makensis clang-cl lld-link glslc >/dev/null || {
    echo "Missing tools: brew install llvm lld nsis shaderc vulkan-headers spirv-headers" >&2
    return 1
  }
  rustup target add "$windows_target" >/dev/null 2>&1
  command -v cargo-xwin >/dev/null ||
    CARGO_TARGET_DIR="$cross/install-target" cargo install cargo-xwin --locked --version "$cargo_xwin_version" --root "$cross/tools"
  rm -rf "$cross/install-target"
  # Downloads the CRT and SDK, and writes the CMake toolchain the Vulkan build needs.
  cargo xwin env --target "$windows_target" --cross-compiler clang-cl >/dev/null
}

build_windows() { # distribution
  (
    prepare_cross_tools
    fetch_runtime "$1"
    local version
    version="$(sed -n 's/^  "version": "\(.*\)",$/\1/p' "$app_dir/src-tauri/tauri.conf.json")"
    cd "$app_dir"
    bunx tauri build --bundles nsis --runner cargo-xwin --target "$windows_target" --features "$1" --config "$(resources_config "$1")"
    cp "$CARGO_TARGET_DIR/$windows_target/release/bundle/nsis/Transcribe Captions_${version}_x64-setup.exe" "$out/Transcribe-Captions-$1-x64-setup.exe"
  )
}

case "$choice" in
  macos) build_macos ;;
  windows-geforce | windows-radeon) build_windows "$choice" ;;
  all)
    build_macos
    build_windows windows-geforce
    build_windows windows-radeon
    ;;
  *) echo "unknown distribution: $choice" >&2; exit 2 ;;
esac
bash "$app_dir/scripts/verify/prune-build.sh"
ls -lh "$out"
