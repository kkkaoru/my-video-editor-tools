#!/usr/bin/env bash
# Caps each Rust build directory, because cargo never removes outdated artifacts by itself. Over the cap, the
# coverage, debug and Windows builds go first, then the release build's intermediates (cargo rebuilds them on
# demand); the finished app stays. Test scratch, stray release builds and earlier versions' zips always go.
set -euo pipefail
app_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
shared_target="${CARGO_TARGET_DIR:-$app_dir/../../edits/work/transcribe-app/target}"
# rust-analyzer checks into src-tauri/target, apart from the shared directory, so it never waits on its lock.
editor_target="$app_dir/src-tauri/target"
budget_kib="${TARGET_BUDGET_KIB:-8388608}"

cap_target() {
  local target="$1" part
  for part in llvm-cov-target debug x86_64-pc-windows-msvc release/incremental release/deps; do
    [ -d "$target" ] && [ "$(du -sk "$target" | cut -f1)" -gt "$budget_kib" ] || return 0
    rm -rf "${target:?}/$part"
  done
}

# A running cargo may be reading these files; the next build prunes instead.
if pgrep -x cargo >/dev/null; then
  echo "prune-build: cargo is running, skipped"
  exit 0
fi
cap_target "$shared_target"
cap_target "$editor_target"
# The app is built into the shared directory, so a release build beside rust-analyzer's is a leftover.
rm -rf "$editor_target/release" "$editor_target/tmp" "$shared_target/tmp"
rm -rf "$app_dir/src-tauri/target-test" "$app_dir/node_modules/.cache/playwright-results" "$app_dir/coverage"
# Every release stays on GitHub, so only the current version's zips are kept here.
version="$(sed -n 's/^  "version": "\(.*\)",$/\1/p' "$app_dir/src-tauri/tauri.conf.json" 2>/dev/null || true)"
booth_zips="$(dirname "$shared_target")/dist-local/booth"
if [ -n "$version" ] && [ -d "$booth_zips" ]; then
  find "$booth_zips" -maxdepth 1 -name '*.zip' ! -name "*-v$version-*" -delete
fi
du -sh "$shared_target" "$editor_target" 2>/dev/null || true
