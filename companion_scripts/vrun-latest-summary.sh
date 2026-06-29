#!/usr/bin/env bash
set -euo pipefail

# Show a one-line summary of the latest run with key artifact presence.
# Usage: ./vrun-latest-summary.sh

root=".vegvisir/runs"
[[ -d "$root" ]] || { echo "No .vegvisir/runs directory found." >&2; exit 1; }

latest=$(find "$root" -mindepth 1 -maxdepth 1 -type d -print0 | xargs -0 stat -c '%Y %n' | sort -nr | awk 'NR==1 {print $2}')
[[ -n "${latest:-}" ]] || { echo "No runs found." >&2; exit 1; }

printf 'run=%s ' "${latest##*/}"
for f in result.md verification.json approvals.json diff.patch; do
  if [[ -f "$latest/$f" ]]; then
    printf '%s=1 ' "$f"
  else
    printf '%s=0 ' "$f"
  fi
done
printf '\n'
