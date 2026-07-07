#!/usr/bin/env bash
set -euo pipefail

# Show a concise diff summary for tracked changes.
# Usage: ./vdiff-summary.sh [path]

path="${1:-.}"

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "Not inside a git repository." >&2
  exit 1
fi

git diff --stat --summary -- "$path"
