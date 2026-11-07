#!/usr/bin/env bash
# Fails unless every Rust file in src-tauri/src reaches the minimum on lines, functions and regions. Run from src-tauri.
# With arguments it covers only a change: comma-separated modules to test (integration tests always run), then modules to measure.
set -euo pipefail
minimum=95
trap 'bash "$(dirname "${BASH_SOURCE[0]}")/prune-build.sh"' EXIT
if [ "$#" -eq 0 ]; then
  cargo llvm-cov --lib --bins --tests --json --summary-only --output-path target-coverage.json
else
  IFS=, read -ra tested <<<"$1"
  cargo llvm-cov clean --workspace
  [ "${#tested[@]}" -eq 0 ] || cargo llvm-cov --no-report --lib -- "${tested[@]/%/::}"
  cargo llvm-cov --no-report --test integration
  cargo llvm-cov report --json --summary-only --output-path target-coverage.json
fi
python3 - "$minimum" "${2-}" "$#" <<'PY'
import json
import sys

minimum = float(sys.argv[1])
measured = {f"/src-tauri/src/{name}.rs" for name in sys.argv[2].split(",") if name}
partial = sys.argv[3] != "0"
report = json.load(open("target-coverage.json"))["data"][0]
METRICS = ("lines", "functions", "regions")
low = [
    (entry["filename"], metric, entry["summary"][metric]["percent"])
    for entry in report["files"]
    for metric in METRICS
    if "/src-tauri/src/" in entry["filename"]
    and entry["summary"][metric]["percent"] < minimum
    and (not partial or any(entry["filename"].endswith(name) for name in measured))
]
print(f"total line coverage: {report['totals']['lines']['percent']:.2f}%")
for name, metric, percent in low:
    print(f"{metric} below {minimum}%: {name}: {percent:.2f}%")
sys.exit(1 if low else 0)
PY
