#!/usr/bin/env bash
# Drops build intermediates that only speed up rebuilds and grow without limit: incremental caches, stale
# dependency versions, and test leftovers. Finished binaries and the app bundle stay. Safe to run at any time.
set -euo pipefail
app_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$app_dir/src-tauri/target}"
find "$target" -type d -name incremental -prune -exec rm -rf {} + 2>/dev/null || true
# Keep a week of dependency builds; older ones belong to versions no longer in Cargo.lock.
find "$target" -type f -path '*/deps/*' -mtime +7 -delete 2>/dev/null || true
rm -rf "$app_dir/src-tauri/target-test" "$app_dir/node_modules/.cache/playwright-results" "$app_dir/coverage"
du -sh "$target" 2>/dev/null || true
