#!/usr/bin/env bash
# Fails unless warping leaves whisper.cpp's text untouched: the same tokens with and without -dtw, on the GPU and the CPU.
# Run after changing scripts/release/whisper-cpp.patch or the pinned whisper.cpp.
# Usage: whisper-dtw-text.sh WHISPER_CLI MODEL WAV WORK_DIR
set -euo pipefail
cli="${1:?usage: whisper-dtw-text.sh WHISPER_CLI MODEL WAV WORK_DIR}"
model="${2:?model}"
wav="${3:?wav}"
work="${4:?work directory}"
mkdir -p "$work"

decoded() { # output-base whisper-cli-arguments...
  local base="$1"
  shift
  "$cli" -m "$model" -f "$wav" -l ja -ojf -np -of "$base" "$@" >/dev/null 2>&1
  # The token lines only; whisper.cpp may split UTF-8 across tokens, so this stays bytes.
  LC_ALL=C grep -E '^[[:space:]]*"(id|text)":' "$base.json" >"$base.tokens"
  [ -s "$base.tokens" ] || { echo "no tokens in $base.json" >&2; return 1; }
}

check() { # label whisper-cli-arguments...
  local label="$1"
  shift
  decoded "$work/$label-plain" "$@"
  decoded "$work/$label-dtw" "$@" -dtw large.v3.turbo
  grep -q '"t_dtw": *[0-9]' "$work/$label-dtw.json" || { echo "$label: no warped times" >&2; return 1; }
  cmp "$work/$label-plain.tokens" "$work/$label-dtw.tokens"
  echo "$label: same text ($(grep -c '"id"' "$work/$label-dtw.tokens") tokens)"
}

check gpu
check cpu -ng
