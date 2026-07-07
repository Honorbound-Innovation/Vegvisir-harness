#!/usr/bin/env bash
set -euo pipefail

# Show the latest run result and verification files side by side.
# Usage: ./vrun-inspect.sh <run-dir>

run_dir="${1:-}"
[[ -n "$run_dir" ]] || { echo "Usage: $0 <run-dir>" >&2; exit 1; }
[[ -d "$run_dir" ]] || { echo "Run directory not found: $run_dir" >&2; exit 1; }

for f in result.md verification.json diff.patch; do
  if [[ -f "$run_dir/$f" ]]; then
    echo "== $f =="
    sed -n '1,200p' "$run_dir/$f"
    echo
  fi
done
