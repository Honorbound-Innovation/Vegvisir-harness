#!/usr/bin/env bash
set -euo pipefail

# Show readable file sizes for files under a path, excluding Vegvisir generated internals.
# Usage: ./vfilesize.sh [path] [count]

path="${1:-.}"
count="${2:-40}"

find "$path" \
  -path '*/.git' -prune -o \
  -path '*/.vegvisir' -prune -o \
  -type f -print0 \
  | xargs -0 --no-run-if-empty du -h 2>/dev/null \
  | sort -h \
  | tail -n "$count"
