#!/usr/bin/env bash
set -euo pipefail

# Show which run artifact types exist in each .vegvisir run.
# Usage: ./vrun-kinds.sh

root=".vegvisir/runs"
[[ -d "$root" ]] || { echo "No .vegvisir/runs directory found." >&2; exit 1; }

find "$root" -mindepth 1 -maxdepth 1 -type d | sort | while read -r run; do
  echo "== ${run##*/} =="
  find "$run" -maxdepth 1 -type f -printf '%f\n' | sort | paste -sd ', ' -
  echo
done
