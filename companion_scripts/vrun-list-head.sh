#!/usr/bin/env bash
set -euo pipefail

# Summarize the newest run directories with names only.
# Usage: ./vrun-list-head.sh [count]

count="${1:-10}"
root=".vegvisir/runs"
[[ -d "$root" ]] || { echo "No .vegvisir/runs directory found." >&2; exit 1; }

find "$root" -mindepth 1 -maxdepth 1 -type d -printf '%T@ %p\n' 2>/dev/null   | sort -nr   | head -n "$count"   | awk '{print $2}'
