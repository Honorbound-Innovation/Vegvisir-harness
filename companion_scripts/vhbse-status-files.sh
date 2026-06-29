#!/usr/bin/env bash
set -euo pipefail

# Show HBSE-related file candidates from git status without exposing contents.
# Usage: ./vhbse-status-files.sh

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "Not inside a git repository." >&2
  exit 1
fi

git status --short | awk '
  BEGIN { IGNORECASE=1 }
  /hbse|secret|token|credential|auth/ { print }
'
