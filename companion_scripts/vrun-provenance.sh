#!/usr/bin/env bash
set -euo pipefail

# Show provenance hints for a run using filenames and timestamps only.
# Usage: ./vrun-provenance.sh <run-dir>

run_dir="${1:-}"
[[ -n "$run_dir" ]] || { echo "Usage: $0 <run-dir>" >&2; exit 1; }
[[ -d "$run_dir" ]] || { echo "Run directory not found: $run_dir" >&2; exit 1; }

find "$run_dir" -maxdepth 1 -type f -printf '%TY-%Tm-%Td %TH:%TM %f\n' | sort
