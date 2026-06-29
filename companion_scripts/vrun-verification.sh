#!/usr/bin/env bash
set -euo pipefail

# Show the latest verification record for a run.
# Usage: ./vrun-verification.sh <run-dir>

run_dir="${1:-}"
[[ -n "$run_dir" ]] || { echo "Usage: $0 <run-dir>" >&2; exit 1; }
[[ -f "$run_dir/verification.json" ]] || { echo "verification.json not found in: $run_dir" >&2; exit 1; }

sed -n '1,200p' "$run_dir/verification.json"
