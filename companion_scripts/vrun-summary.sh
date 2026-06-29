#!/usr/bin/env bash
set -euo pipefail

# Summarize a single .vegvisir run directory.
# Usage: ./vrun-summary.sh <run-dir>

run_dir="${1:-}"
[[ -n "$run_dir" ]] || { echo "Usage: $0 <run-dir>" >&2; exit 1; }
[[ -d "$run_dir" ]] || { echo "Run directory not found: $run_dir" >&2; exit 1; }

for f in manifest.json result.md verification.json approvals.json; do
  if [[ -f "$run_dir/$f" ]]; then
    echo "== $f =="
    sed -n '1,120p' "$run_dir/$f"
    echo
  fi
done
