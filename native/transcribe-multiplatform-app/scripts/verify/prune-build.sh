#!/usr/bin/env bash
# Caps each Rust build directory, because cargo never removes outdated artifacts by itself. Over the cap, the
# coverage and debug builds go first (cargo rebuilds them on demand); release keeps the finished app.
set -euo pipefail
app_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
shared_target="${CARGO_TARGET_DIR:-$app_dir/../../edits/work/transcribe-app/target}"
# rust-analyzer checks into src-tauri/target, apart from the shared directory, so it never waits on its lock.
editor_target="$app_dir/src-tauri/target"
budget_kib="${TARGET_BUDGET_KIB:-8388608}"

cap_target() {
  local target="$1" part
  for part in llvm-cov-target debug; do
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
rm -rf "$app_dir/src-tauri/target-test" "$app_dir/node_modules/.cache/playwright-results" "$app_dir/coverage"
du -sh "$shared_target" "$editor_target" 2>/dev/null || true
