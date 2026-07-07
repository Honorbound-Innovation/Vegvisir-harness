#!/usr/bin/env bash
set -euo pipefail

# Show the current run's memory usage records if present.
# Usage: ./vrun-memory.sh <run-dir>

run_dir="${1:-}"
[[ -n "$run_dir" ]] || { echo "Usage: $0 <run-dir>" >&2; exit 1; }
[[ -d "$run_dir" ]] || { echo "Run directory not found: $run_dir" >&2; exit 1; }

for f in memory-used.json memory-written.json; do
  [[ -f "$run_dir/$f" ]] || continue
  echo "== $f =="
  sed -n '1,200p' "$run_dir/$f"
  echo

done
