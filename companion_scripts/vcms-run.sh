#!/usr/bin/env bash
set -euo pipefail

# Show the latest CMS memory files when they exist in a run directory.
# Usage: ./vcms-run.sh <run-dir>

run_dir="${1:-}"
[[ -n "$run_dir" ]] || { echo "Usage: $0 <run-dir>" >&2; exit 1; }
[[ -d "$run_dir" ]] || { echo "Run directory not found: $run_dir" >&2; exit 1; }

for f in memory-used.json memory-written.json context.md context-sources.json; do
  if [[ -f "$run_dir/$f" ]]; then
    echo "== $f =="
    sed -n '1,200p' "$run_dir/$f"
    echo
  fi
done
