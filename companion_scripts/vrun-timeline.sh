#!/usr/bin/env bash
set -euo pipefail

# Build a quick timeline from one run directory.
# Usage: ./vrun-timeline.sh <run-dir>

run_dir="${1:-}"
[[ -n "$run_dir" ]] || { echo "Usage: $0 <run-dir>" >&2; exit 1; }
[[ -d "$run_dir" ]] || { echo "Run directory not found: $run_dir" >&2; exit 1; }

find "$run_dir" -type f -printf '%T@ %p\n' | sort -n
