#!/usr/bin/env bash
# Fails unless the stand-in decoder and the speaker encoder still give the speaker turns of the full
# model, byte for byte.
# Run after updating nemo-speech (NEMO_VERSION in fetch-runtime.sh): its GGUF checks can change.
# Usage: speaker-stand-in.sh NEMO_SPEECH FULL_DECODER FULL_ENCODER SPEAKER_ENCODER WAV WORK_DIR
set -euo pipefail
nemo="${1:?usage: speaker-stand-in.sh NEMO_SPEECH FULL_DECODER FULL_ENCODER SPEAKER_ENCODER WAV WORK_DIR}"
full="${2:?full decoder}"
full_encoder="${3:?full encoder}"
speaker_encoder="${4:?speaker encoder}"
wav="${5:?wav}"
work="${6:?work directory}"
app_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
head_file="$app_dir/src-tauri/resources/speaker-decoder-head.gguf"
stand_in_bytes="$(sed -n 's/^const SPEAKER_DECODER_BYTES: u64 = \([0-9_]*\);$/\1/p' "$app_dir/src-tauri/src/engines.rs" | tr -d _)"
port=18476
mkdir -p "$work"
cp "$head_file" "$work/stand-in.gguf"
dd if=/dev/zero of="$work/stand-in.gguf" bs=1 count=0 seek="$stand_in_bytes" 2>/dev/null

turns() { # decoder encoder output serve-arguments...
  local decoder="$1" encoder="$2" output="$3" server tries=0
  shift 3
  "$nemo" serve --no-ui --host 127.0.0.1 --port "$port" --asr-model "$decoder" --salm-encoder "$encoder" "$@" 2>"$output.err" >/dev/null &
  server=$!
  until curl -fs "http://127.0.0.1:$port/health" >/dev/null 2>&1; do
    tries=$((tries + 1))
    kill -0 "$server" 2>/dev/null && [ "$tries" -lt 1200 ] || { tail -5 "$output.err" >&2; echo "the server did not start with $decoder" >&2; return 1; }
    sleep 0.25
  done
  curl -fsS -o "$output" -F "file=@$wav" "http://127.0.0.1:$port/v1/audio/diarizations"
  kill "$server"
  wait "$server" 2>/dev/null || true
}

turns "$work/stand-in.gguf" "$speaker_encoder" "$work/stand-in.json" --no-warmup --asr.salm.mtp_draft 0
turns "$full" "$full_encoder" "$work/full.json"
cmp "$work/stand-in.json" "$work/full.json"
echo "same speaker turns ($(grep -o '"speaker"' "$work/full.json" | wc -l | tr -d ' '))"
rm -f "$work/stand-in.gguf"
