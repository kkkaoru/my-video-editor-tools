#!/usr/bin/env bash
# The app's quality gate, run before a commit that stages this app's files; other commits pass untouched.
# Install once per clone: git config --local hook.transcribe-app.event pre-commit
#                         git config --local hook.transcribe-app.command "bash native/transcribe-multiplatform-app/scripts/verify/pre-commit.sh"
# shortcut: checks the working tree, not only the staged content; stash-free so shared checkouts stay untouched.
set -euo pipefail
root="$(git rev-parse --show-toplevel)"
cd "$root/native/transcribe-multiplatform-app"
[ -d node_modules ] || bun install --frozen-lockfile
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/edits/work/transcribe-app/target}"
exec bun scripts/verify/changed-checks.ts
