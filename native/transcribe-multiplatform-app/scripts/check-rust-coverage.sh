#!/usr/bin/env bash
# Fails unless every Rust file in src-tauri/src reaches the minimum line coverage. Run from src-tauri.
set -euo pipefail
minimum=95
cargo llvm-cov --lib --bins --tests --json --summary-only --output-path target-coverage.json
python3 - "$minimum" <<'PY'
import json
import sys

minimum = float(sys.argv[1])
report = json.load(open("target-coverage.json"))["data"][0]
low = [
    (entry["filename"], entry["summary"]["lines"]["percent"])
    for entry in report["files"]
    if "/src-tauri/src/" in entry["filename"] and entry["summary"]["lines"]["percent"] < minimum
]
print(f"total line coverage: {report['totals']['lines']['percent']:.2f}%")
for name, percent in low:
    print(f"below {minimum}%: {name}: {percent:.2f}%")
sys.exit(1 if low else 0)
PY
