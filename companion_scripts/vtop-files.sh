#!/usr/bin/env bash
set -euo pipefail

# Show the largest tracked/untracked files in the workspace.
# Usage: ./vtop-files.sh [count]

count="${1:-20}"

find . \
  -path '*/.git' -prune -o \
  -path '*/.vegvisir' -prune -o \
  -type f -print0 \
  | xargs -0 stat -c '%s %n' \
  | sort -nr \
  | head -n "$count"
