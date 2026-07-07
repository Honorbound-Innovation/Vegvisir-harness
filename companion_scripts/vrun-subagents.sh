#!/usr/bin/env bash
set -euo pipefail

# Show the current run's subagent metadata.
# Usage: ./vrun-subagents.sh <run-dir>

run_dir="${1:-}"
[[ -n "$run_dir" ]] || { echo "Usage: $0 <run-dir>" >&2; exit 1; }
[[ -f "$run_dir/subagents.json" ]] || { echo "subagents.json not found in: $run_dir" >&2; exit 1; }

sed -n '1,240p' "$run_dir/subagents.json"
