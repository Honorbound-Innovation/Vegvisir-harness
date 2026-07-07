#!/usr/bin/env bash
set -euo pipefail

# Show changed files with status and concise paths.
# Usage: ./vchanged-files.sh

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "Not inside a git repository." >&2
  exit 1
fi

git status --short
