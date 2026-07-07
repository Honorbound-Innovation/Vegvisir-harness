#!/usr/bin/env bash
set -euo pipefail

# Show the most relevant approvals for a run without exposing sensitive values.
# Usage: ./vapprovals.sh <run-dir>

run_dir="${1:-}"
[[ -n "$run_dir" ]] || { echo "Usage: $0 <run-dir>" >&2; exit 1; }
[[ -f "$run_dir/approvals.json" ]] || { echo "approvals.json not found in: $run_dir" >&2; exit 1; }

sed -n '1,220p' "$run_dir/approvals.json"
