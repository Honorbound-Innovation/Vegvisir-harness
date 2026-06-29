#!/usr/bin/env bash
set -euo pipefail

# Show approval files linked to recent runs by name only.
# Usage: ./vapprovals-linked-runs.sh

root=".vegvisir/runs"
[[ -d "$root" ]] || { echo "No .vegvisir/runs directory found." >&2; exit 1; }

find "$root" -mindepth 1 -maxdepth 1 -type d | sort | while read -r run; do
  [[ -f "$run/approvals.json" ]] || continue
  echo "${run##*/}/approvals.json"
done
