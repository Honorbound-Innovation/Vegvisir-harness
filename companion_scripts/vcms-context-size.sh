#!/usr/bin/env bash
set -euo pipefail

# Estimate CMS-related context footprint by counting local context/memory artifacts.
# Usage: ./vcms-context-size.sh [path]

root="${1:-.}"
if [[ ! -d "$root" ]]; then
  echo "Path not found: $root" >&2
  exit 1
fi

find "$root" \
  -path '*/.git' -prune -o \
  -path '*/.vegvisir' -prune -o \
  -type f \( -name 'context.md' -o -name 'context-sources.json' -o -name 'memory-used.json' -o -name 'memory-written.json' \) \
  -print0 \
  | xargs -0 -r stat -c '%s %n' \
  | awk '{sum+=$1; print}' \
  | sort -n \
  | awk 'BEGIN{count=0} {count++; print} END{printf "TOTAL_BYTES=%d FILES=%d\n", sum, count}'
