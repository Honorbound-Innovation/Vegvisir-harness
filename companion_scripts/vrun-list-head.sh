#!/usr/bin/env bash
set -euo pipefail

# Summarize the newest run directories with names only.
# Usage: ./vrun-list-head.sh [count]

count="${1:-10}"
root=".vegvisir/runs"
[[ -d "$root" ]] || { echo "No .vegvisir/runs directory found." >&2; exit 1; }

find "$root" -mindepth 1 -maxdepth 1 -type d -print0 \
  | xargs -0 stat -c '%Y %n' \
  | sort -nr \
  | head -n "$count" \
  | awk '{print $2}'
