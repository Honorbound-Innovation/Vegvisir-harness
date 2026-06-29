#!/usr/bin/env bash
set -euo pipefail

# Show the newest Vegvisir run directory.
# Usage: ./vrun-latest.sh

root=".vegvisir/runs"
[[ -d "$root" ]] || { echo "No .vegvisir/runs directory found." >&2; exit 1; }

latest="$(find "$root" -mindepth 1 -maxdepth 1 -type d -printf '%T@ %p\n' 2>/dev/null | sort -nr | awk 'NR==1 {print $2}')"
[[ -n "$latest" ]] || { echo "No Vegvisir run directories found." >&2; exit 1; }
printf '%s\n' "$latest"
